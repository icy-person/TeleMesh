mod telegram;
mod web;

use std::{collections::HashMap, env, net::SocketAddr, sync::Arc, time::Instant};
use anyhow::{Context, Result};
use tokio::sync::{broadcast, Mutex};
use tracing::info;
use telemesh_protocol::Event;

pub struct AppState {
    pub telegram: telegram::TelegramService,
    pub token: String,
    pub events: broadcast::Sender<Event>,
    pub login: Mutex<Option<telegram::LoginState>>,
    pub ws_tickets: Mutex<HashMap<String, Instant>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "telemesh=info,tower_http=info".into())).init();
    let api_id: i32 = env::var("TELEGRAM_API_ID").context("TELEGRAM_API_ID missing")?.parse().context("TELEGRAM_API_ID must be integer")?;
    let api_hash = env::var("TELEGRAM_API_HASH").context("TELEGRAM_API_HASH missing")?;
    let session = env::var("TELEMESH_SESSION").unwrap_or_else(|_| "data/telemesh.session".into());
    let token = env::var("TELEMESH_TOKEN").context("TELEMESH_TOKEN missing")?;
    let bind: SocketAddr = env::var("TELEMESH_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into()).parse().context("invalid TELEMESH_BIND")?;
    if let Some(parent) = std::path::Path::new(&session).parent() { tokio::fs::create_dir_all(parent).await?; }
    let telegram = telegram::TelegramService::connect(api_id, api_hash, &session).await?;
    let (events, _) = broadcast::channel(512);
    let state = Arc::new(AppState { telegram, token, events, login: Mutex::new(None), ws_tickets: Mutex::new(HashMap::new()) });
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!("TeleMesh server listening on {bind}");
    axum::serve(listener, web::router(state)).await?;
    Ok(())
}
