use std::{path::Path, sync::Arc};
use anyhow::Result;
use futures_util::StreamExt;
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use telemesh_protocol::{DialogDto, Event, MeResponse, MessageDto, SendMessageResponse};
use tokio::sync::broadcast;
use tracing::{error, info};

pub struct LoginState { pub token: grammers_client::client::LoginToken, pub password: Option<grammers_client::client::PasswordToken> }

#[derive(Clone)]
pub struct TelegramService { client: Client, events: broadcast::Sender<Event> }

impl TelegramService {
    pub async fn connect(api_id: i32, api_hash: String, session_path: &str) -> Result<Self> {
        let session = Arc::new(SqliteSession::open(Path::new(session_path)).await?);
        let grammers_mtsender::SenderPool { runner, updates, handle } = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(handle);
        tokio::spawn(async move { if let Err(e) = runner.run().await { error!("Telegram runner stopped: {e}"); } });
        let (events, _) = broadcast::channel(512);
        let service = Self { client, events };
        if service.client.is_authorized().await? { info!("Telegram session authorized"); } else { info!("Telegram session not authorized"); }
        let c = service.client.clone();
        let tx = service.events.clone();
        tokio::spawn(async move {
            match c.stream_updates(updates, Default::default()).await {
                Ok(mut stream) => while let Some(update) = stream.next().await {
                    if let Ok(grammers_client::update::Update::NewMessage(message)) = update {
                        let _ = tx.send(Event::NewMessage(MessageDto { id: message.id(), peer_id: message.peer_id().value(), text: message.text().to_owned(), outgoing: message.outgoing(), date: None }));
                    }
                },
                Err(e) => error!("Telegram update stream stopped: {e}"),
            }
        });
        Ok(service)
    }
    pub async fn authorized(&self) -> Result<bool> { Ok(self.client.is_authorized().await?) }
    pub async fn me(&self) -> Result<MeResponse> {
        let u = self.client.get_me().await?;
        Ok(MeResponse { id:u.id().value(), username:u.username().map(str::to_owned), first_name:u.first_name().map(str::to_owned), last_name:u.last_name().map(str::to_owned) })
    }
    pub async fn dialogs(&self, limit: usize) -> Result<Vec<DialogDto>> {
        let mut iter = self.client.iter_dialogs();
        let mut out = Vec::new();
        while out.len() < limit {
            let Some(d) = iter.next().await? else { break };
            let p = d.peer();
            let last_message = d.last_message.as_ref().map(|m| MessageDto { id:m.id(), peer_id:m.peer_id().value(), text:m.text().to_owned(), outgoing:m.outgoing(), date:None });
            out.push(DialogDto {
                id:p.id().value(), name:p.name().unwrap_or_default().to_owned(),
                username: match p { grammers_client::peer::Peer::User(ref u)=>u.username().map(str::to_owned), grammers_client::peer::Peer::Channel(ref c)=>c.username().map(str::to_owned), _=>None },
                kind: match p { grammers_client::peer::Peer::User(_)=>"user", grammers_client::peer::Peer::Group(_)=>"group", grammers_client::peer::Peer::Channel(_)=>"channel" }.into(),
                last_message
            });
        }
        Ok(out)
    }
    pub async fn send_message(&self, peer:&str, text:&str)->Result<SendMessageResponse>{
        let p=self.client.resolve_username(peer.trim_start_matches('@')).await?.ok_or_else(||anyhow::anyhow!("username not found"))?;
        let r=p.to_ref().await?.ok_or_else(||anyhow::anyhow!("peer cannot be represented"))?;
        let m=self.client.send_message(&r,text).await?;
        Ok(SendMessageResponse{message_id:m.id(),peer_id:m.peer_id().value()})
    }
    pub async fn login_start(&self, phone:&str, api_hash:&str)->Result<grammers_client::client::LoginToken>{ Ok(self.client.request_login_code(phone,api_hash).await?) }
    pub async fn login_code(&self,t:&grammers_client::client::LoginToken,code:&str)->std::result::Result<grammers_client::peer::User,SignInError>{ self.client.sign_in(t,code).await }
    pub async fn login_password(&self,t:grammers_client::client::PasswordToken,password:&str)->std::result::Result<grammers_client::peer::User,SignInError>{ self.client.check_password(t,password).await }
}
