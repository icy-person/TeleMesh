use std::{sync::Arc, time::{Duration, Instant}};
use axum::{extract::{State, WebSocketUpgrade, Query}, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use telemesh_protocol::{ApiError, Event, HealthResponse, LoginCompleteRequest, LoginStartRequest, MeResponse, PasswordRequest, SendMessageRequest};
use crate::AppState;
use uuid::Uuid;
use tower_http::{trace::TraceLayer, cors::CorsLayer};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/events/ticket", post(ticket))
        .route("/api/v1/me", get(me))
        .route("/api/v1/dialogs", get(dialogs))
        .route("/api/v1/messages", get(messages))
        .route("/api/v1/messages/send", post(send_message))
        .route("/api/v1/auth/start", post(auth_start))
        .route("/api/v1/auth/complete", post(auth_complete))
        .route("/api/v1/auth/password", post(auth_password))
        .route("/api/v1/events", get(events))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::very_permissive())
}

fn authorized(h: &HeaderMap, s: &AppState) -> bool {
    h.get("x-telemesh-token").and_then(|v| v.to_str().ok()).is_some_and(|v| v == s.token)
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
    Json(HealthResponse { status: "ok".into(), telegram_authorized: s.telegram.authorized().await.unwrap_or(false), version: env!("CARGO_PKG_VERSION").into() })
}
async fn me(State(s): State<Arc<AppState>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.me().await { Ok(v) => Json(v).into_response(), Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response() }
}
async fn dialogs(State(s): State<Arc<AppState>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) { return deny(); }
    match s.telegram.dialogs(100).await { Ok(v) => Json(v).into_response(), Err(e) => (StatusCode::BAD_GATEWAY, Json(ApiError { error: e.to_string() })).into_response() }
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
    match s.telegram.send_message(&r.peer, &r.text).await {
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
        Ok(u) => { *l = None; Json(MeResponse { id: u.id().value(), username: u.username().map(str::to_owned), first_name: u.first_name().map(str::to_owned), last_name: u.last_name().map(str::to_owned) }).into_response() }
        Err(grammers_client::SignInError::PasswordRequired(p)) => { x.password = Some(p); (StatusCode::ACCEPTED, Json(ApiError { error: "2FA password required".into() })).into_response() }
        Err(e) => (StatusCode::UNAUTHORIZED, Json(ApiError { error: e.to_string() })).into_response(),
    }
}
async fn auth_password(State(s): State<Arc<AppState>>, h: HeaderMap, Json(r): Json<PasswordRequest>) -> Response {
    if !authorized(&h, &s) { return deny(); }
    let mut l = s.login.lock().await;
    let Some(x) = l.as_mut() else { return (StatusCode::CONFLICT, Json(ApiError { error: "no pending 2FA login".into() })).into_response(); };
    let Some(p) = x.password.take() else { return (StatusCode::CONFLICT, Json(ApiError { error: "2FA not pending".into() })).into_response(); };
    match s.telegram.login_password(p, &r.password).await {
        Ok(u) => { *l = None; Json(MeResponse { id: u.id().value(), username: u.username().map(str::to_owned), first_name: u.first_name().map(str::to_owned), last_name: u.last_name().map(str::to_owned) }).into_response() }
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
                Ok(e) => { if socket.send(Message::Text(serde_json::to_string(&e).unwrap().into())).await.is_err() { break } }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
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
