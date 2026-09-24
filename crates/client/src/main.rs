use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use reqwest::Client;
use telemesh_protocol::{DialogDto, HealthResponse, MeResponse, MessagesResponse, SendMessageRequest};

#[derive(Parser)]
#[command(name="telemesh")]
struct Cli {
    #[arg(long, env="TELEMESH_SERVER", default_value="http://127.0.0.1:8787")]
    server: String,
    #[arg(long, env="TELEMESH_CLIENT_TOKEN")]
    token: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Health,
    Me,
    Dialogs,
    History { peer: String, #[arg(long, default_value_t=50)] limit: usize, #[arg(long, default_value_t=0)] offset_id: i32 },
    Send { peer: String, text: String },
    Events,
}

async fn get<T: serde::de::DeserializeOwned>(c: &Cli, path: &str) -> Result<T> {
    Ok(Client::new()
        .get(format!("{}{}", c.server.trim_end_matches('/'), path))
        .header("x-telemesh-token", &c.token)
        .send().await?.error_for_status()?.json().await?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let c = Cli::parse();
    match &c.command {
        Command::Health => println!("{}", serde_json::to_string_pretty(&get::<HealthResponse>(&c, "/health").await?)?),
        Command::Me => println!("{}", serde_json::to_string_pretty(&get::<MeResponse>(&c, "/api/v1/me").await?)?),
        Command::Dialogs => println!("{}", serde_json::to_string_pretty(&get::<Vec<DialogDto>>(&c, "/api/v1/dialogs").await?)?),
        Command::History { peer, limit, offset_id } => {
            let path = format!("/api/v1/messages?peer={}&limit={}&offset_id={}", urlencoding::encode(peer), limit, offset_id);
            let v: MessagesResponse = get(&c, &path).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Command::Send { peer, text } => {
            let v = Client::new()
                .post(format!("{}/api/v1/messages/send", c.server.trim_end_matches('/')))
                .header("x-telemesh-token", &c.token)
                .json(&SendMessageRequest { peer: peer.clone(), text: text.clone(), reply_to: None })
                .send().await?.error_for_status()?.json::<telemesh_protocol::SendMessageResponse>().await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Command::Events => {
            let ticket: serde_json::Value = Client::new()
                .post(format!("{}/api/v1/events/ticket", c.server.trim_end_matches('/')))
                .header("x-telemesh-token", &c.token)
                .send().await?.error_for_status()?.json().await?;
            let ticket = ticket["ticket"].as_str().context("missing websocket ticket")?;
            let ws = format!("{}/api/v1/events?ticket={}", c.server.trim_end_matches('/').replace("http://","ws://").replace("https://","wss://"), urlencoding::encode(ticket));
            let (mut stream, _) = tokio_tungstenite::connect_async(ws).await?;
            while let Some(m) = stream.next().await { println!("{:?}", m?); }
        }
    }
    Ok(())
}
