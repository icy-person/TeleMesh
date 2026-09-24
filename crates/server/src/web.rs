use std::{sync::Arc, time::{Duration, Instant}};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use serde::Deserialize;
use telemesh_protocol::{
    ApiError, DeleteMessagesRequest, EditMessageRequest, Event, ForwardMessagesRequest,
    HealthResponse, LoginCompleteRequest, LoginStartRequest, MarkReadRequest, MeResponse,
    PasswordRequest, ReactRequest, SendMessageRequest,
};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::AppState;

const DEFAULT_MAX_MEDIA_BYTES: u64 = 500 * 1024 * 1024;

fn max_media_bytes() -> u64 {
    std::env::var("TELEMESH_MAX_MEDIA_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .map(|mb| mb.saturating_mul(1024 * 1024))
        .unwrap_or(DEFAULT_MAX_MEDIA_BYTES)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/events/ticket", post(ticket))
        .route("/api/v1/me", get(me))
        .route("/api/v1/dialogs", get(dialogs))
        .route("/api/v1/messages", get(messages))
        .route("/api/v1/messages/send", post(send_message))
        .route("/api/v1/messages/media", post(send_media))
        .route("/api/v1/messages/media/download", get(download_media))
        .route("/api/v1/messages/edit", post(edit_message))
        .route("/api/v1/messages/delete", post(delete_messages))
        .route("/api/v1/messages/forward", post(forward_messages))
        .route("/api/v1/messages/react", post(react_message))
        .route("/api/v1/messages/read", post(mark_read))
        .route("/api/v1/messages/search", get(search_messages))
        .route("/api/v1/auth/start", post(auth_start))
        .route("/api/v1/auth/complete", post(auth_complete))
        .route("/api/v1/auth/password", post(auth_password))
        .route("/api/v1/events", get(events))
        .with_state(state)
        .layer(DefaultBodyLimit::max(max_media_bytes() as usize))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::very_permissive())
}

fn authorized(h: &HeaderMap, s: &AppState) -> bool {
    h.get("x-telemesh-token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == s.token)
}

fn deny() -> Response {
    (StatusCode::UNAUTHORIZED, Json(ApiError { error: "unauthorized".into() })).into_response()
}

async fn ticket(State(s): State<Arc<AppState>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) { return deny(); }
    let t = Uuid::new_v4().to_string();
    s.ws_tickets.lock().await.insert(t.clone(), Instant::now() + Duration::from_secs(60));
    Json(serde_json::json!({"ticket": t})).into_response()
}

async fn health(State(s): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        telegram_authorized: s.telegram.authorized().await.unwrap_or(false),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn me(State(s): State<Arc<AppState>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.me().await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn dialogs(State(s): State<Arc<AppState>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.dialogs(100).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

#[derive(Deserialize)]
struct HistoryQuery { peer: String, limit: Option<usize>, offset_id: Option<i32> }

async fn messages(State(s): State<Arc<AppState>>, h: HeaderMap, Query(q): Query<HistoryQuery>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.messages(&q.peer, q.limit.unwrap_or(50), q.offset_id.unwrap_or(0)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn send_message(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<SendMessageRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.send_message(&r.peer, &r.text, r.reply_to).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

#[derive(Deserialize)]
struct DownloadQuery { peer: String, message_id: i32 }

async fn download_media(State(s): State<Arc<AppState>>, h: HeaderMap, Query(q): Query<DownloadQuery>) -> Response {
    if !authorized(&h, &s) { return deny(); }

    let path = std::env::temp_dir().join(format!("telemesh-download-{}", Uuid::new_v4()));
    let result = s.telegram.download_media_to_file(&q.peer, q.message_id, &path).await;

    let downloaded = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            return (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response();
        }
    };

    let file = match tokio::fs::File::open(&downloaded.path).await {
        Ok(file) => file,
        Err(e) => {
            let _ = tokio::fs::remove_file(&downloaded.path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response();
        }
    };

    let filename = downloaded.metadata.filename.clone().unwrap_or_else(|| format!("telegram-{}.bin", q.message_id));
    let mime = downloaded.metadata.mime.clone().unwrap_or_else(|| "application/octet-stream".into());
    let size = downloaded.metadata.size.unwrap_or(0);
    let path_for_cleanup = downloaded.path.clone();

    let stream = async_stream::stream! {
        let mut reader = ReaderStream::new(file);
        while let Some(chunk) = reader.next().await {
            yield chunk;
        }
        let _ = tokio::fs::remove_file(path_for_cleanup).await;
    };

    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, mime.parse().unwrap_or_else(|_| "application/octet-stream".parse().unwrap()));
    if size > 0 {
        if let Ok(value) = header::HeaderValue::from_str(&size.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, value);
        }
    }
    if let Ok(value) = header::HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", urlencoding::encode(&filename))) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

async fn send_media(State(s): State<Arc<AppState>>, h: HeaderMap, mut multipart: Multipart) -> Response {
    if !authorized(&h, &s) { return deny(); }

    let mut peer = String::new();
    let mut caption = String::new();
    let mut name = "upload.bin".to_string();
    let mut mime = "application/octet-stream".to_string();
    let mut path: Option<std::path::PathBuf> = None;
    let mut total = 0u64;

    while let Ok(Some(mut field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default().to_string();

        if field_name == "peer" {
            peer = field.text().await.unwrap_or_default();
            continue;
        }
        if field_name == "caption" {
            caption = field.text().await.unwrap_or_default();
            continue;
        }
        if field_name != "file" {
            continue;
        }

        if let Some(n) = field.file_name() {
            name = n.to_string();
        }
        if let Some(ct) = field.content_type() {
            mime = ct.to_string();
        }

        let safe_name: String = name
            .chars()
            .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
            .take(240)
            .collect();

        let file_path = std::env::temp_dir().join(format!("telemesh-upload-{}-{}", Uuid::new_v4(), safe_name));
        let mut file = match tokio::fs::File::create(&file_path).await {
            Ok(file) => file,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
        };

        while let Ok(Some(chunk)) = field.chunk().await {
            total = total.saturating_add(chunk.len() as u64);
            if total > max_media_bytes() {
                let _ = tokio::fs::remove_file(&file_path).await;
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(ApiError { error: format!("file too large ({} MiB limit)", max_media_bytes() / (1024 * 1024)) }),
                ).into_response();
            }
            if let Err(e) = file.write_all(&chunk).await {
                let _ = tokio::fs::remove_file(&file_path).await;
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response();
            }
        }

        if let Err(e) = file.flush().await {
            let _ = tokio::fs::remove_file(&file_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response();
        }

        path = Some(file_path);
        break;
    }

    let Some(path) = path else {
        return (StatusCode::BAD_REQUEST, Json(ApiError { error: "peer and file are required".into() })).into_response();
    };
    if peer.trim().is_empty() {
        let _ = tokio::fs::remove_file(&path).await;
        return (StatusCode::BAD_REQUEST, Json(ApiError { error: "peer and file are required".into() })).into_response();
    }
    if total == 0 {
        let _ = tokio::fs::remove_file(&path).await;
        return (StatusCode::BAD_REQUEST, Json(ApiError { error: "empty files are not allowed".into() })).into_response();
    }

    let result = s.telegram.send_file(&peer, &path, &caption, &name, &mime).await;
    let _ = tokio::fs::remove_file(&path).await;

    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn edit_message(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<EditMessageRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.edit_message(&r.peer, r.message_id, &r.text).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn delete_messages(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<DeleteMessagesRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.delete_messages(&r.peer, &r.message_ids).await {
        Ok(count) => Json(serde_json::json!({"deleted": count})).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn forward_messages(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<ForwardMessagesRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.forward_messages(&r.source, &r.destination, &r.message_ids).await {
        Ok(messages) => Json(messages).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn react_message(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<ReactRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.react(&r.peer, r.message_id, r.reaction.as_deref()).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn mark_read(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<MarkReadRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.mark_read(&r.peer).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

#[derive(Deserialize)]
struct SearchQuery { peer: Option<String>, q: String, limit: Option<usize> }

async fn search_messages(State(s): State<Arc<AppState>>, h: HeaderMap, Query(q): Query<SearchQuery>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.search(q.peer.as_deref(), &q.q, q.limit.unwrap_or(50)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn auth_start(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<LoginStartRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match std::env::var("TELEGRAM_API_HASH") {
        Ok(hash) => match s.telegram.login_start(&r.phone, &hash).await {
            Ok(t) => {
                *s.login.lock().await = Some(crate::telegram::LoginState { token: t, password: None });
                Json(telemesh_protocol::LoginStartResponse { status: "code_sent".into(), requires_code: true }).into_response()
            }
            Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response(),
        },
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: "TELEGRAM_API_HASH missing".into() })).into_response(),
    }
}

async fn auth_complete(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<LoginCompleteRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    let mut l = s.login.lock().await;
    let Some(x) = l.as_mut() else { return (StatusCode::CONFLICT, Json(ApiError { error: "no pending login".into() })).into_response(); };

    match s.telegram.login_code(&x.token, &r.code).await {
        Ok(u) => {
            *l = None;
            Json(MeResponse { id: u.raw.id().bare_id(), username: u.username().map(str::to_owned), first_name: u.first_name().map(str::to_owned), last_name: u.last_name().map(str::to_owned) }).into_response()
        }
        Err(grammers_client::SignInError::PasswordRequired(p)) => {
            x.password = Some(p);
            (StatusCode::ACCEPTED, Json(ApiError { error: "2FA password required".into() })).into_response()
        }
        Err(e) => (StatusCode::UNAUTHORIZED, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

async fn auth_password(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<PasswordRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    let mut l = s.login.lock().await;
    let Some(x) = l.as_mut() else { return (StatusCode::CONFLICT, Json(ApiError { error: "no pending 2FA login".into() })).into_response(); };
    let Some(p) = x.password.take() else { return (StatusCode::CONFLICT, Json(ApiError { error: "2FA not pending".into() })).into_response(); };

    match s.telegram.login_password(p, &r.password).await {
        Ok(u) => {
            *l = None;
            Json(MeResponse { id: u.raw.id().bare_id(), username: u.username().map(str::to_owned), first_name: u.first_name().map(str::to_owned), last_name: u.last_name().map(str::to_owned) }).into_response()
        }
        Err(e) => (StatusCode::UNAUTHORIZED, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

#[derive(Deserialize)]
struct TicketQuery { ticket: String }

async fn events(State(s): State<Arc<AppState>>, Query(q): Query<TicketQuery>, ws: WebSocketUpgrade) -> Response {
    let mut tickets = s.ws_tickets.lock().await;
    let valid = tickets.remove(&q.ticket).is_some_and(|expiry| expiry > Instant::now());
    drop(tickets);
    if !valid { return deny(); }
    ws.on_upgrade(move |socket| websocket(socket, s.events.subscribe()))
}

async fn websocket(mut socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Event>) {
    let _ = socket.send(Message::Text(serde_json::to_string(&Event::Connected).unwrap().into())).await;

    loop {
        tokio::select! {
            e = rx.recv() => match e {
                Ok(e) => {
                    if socket.send(Message::Text(serde_json::to_string(&e).unwrap().into())).await.is_err() { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let _ = socket.send(Message::Text(serde_json::to_string(&Event::Status { authorized: true }).unwrap().into())).await;
                    tracing::warn!("WebSocket client lagged by {skipped} events; client should resync");
                }
                Err(_) => break,
            },
            m = socket.next() => match m {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(v))) => { let _ = socket.send(Message::Pong(v)).await; }
                Some(Ok(_)) => {},
                Some(Err(_)) => break,
            }
        }
    }
}
