use std::{path::Path, sync::Arc};
use anyhow::Result;
use futures_util::StreamExt;
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use telemesh_protocol::{DialogDto, Event, MeResponse, MessageDto, MessagesResponse, SearchResponse, SendMessageResponse};
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
        info!("Telegram authorized: {}", service.client.is_authorized().await?);
        let c = service.client.clone();
        let tx = service.events.clone();
        tokio::spawn(async move {
            match c.stream_updates(updates, Default::default()).await {
                Ok(mut stream) => while let Some(update) = stream.next().await {
                    if let Ok(grammers_client::update::Update::NewMessage(message)) = update {
                        let _ = tx.send(Event::NewMessage(Self::message_dto(&message)));
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

    fn message_dto(m: &grammers_client::message::Message) -> MessageDto {
        MessageDto {
            id:m.id(), peer_id:m.peer_id().value(), text:m.text().to_owned(), outgoing:m.outgoing(), date:Some(m.date()),
            reply_to:m.reply_to_message_id(), edited:m.edit_date().is_some(), reaction_count:m.reaction_count(), media:None,
        }
    }

    async fn peer_ref(&self, peer: &str) -> Result<grammers_session::types::PeerRef> {
        let value=peer.trim().trim_start_matches('@');
        if let Ok(id)=value.parse::<i64>() {
            let mut iter=self.client.iter_dialogs();
            while let Some(d)=iter.next().await? { if d.peer_id().value()==id { return Ok(d.peer_ref()); } }
            return Err(anyhow::anyhow!("peer is not in the dialog cache"));
        }
        let p=self.client.resolve_username(value).await?.ok_or_else(||anyhow::anyhow!("username not found"))?;
        p.to_ref().await?.ok_or_else(||anyhow::anyhow!("peer cannot be represented"))
    }

    pub async fn dialogs(&self, limit: usize) -> Result<Vec<DialogDto>> {
        let mut iter=self.client.iter_dialogs(); let mut out=Vec::new();
        while out.len()<limit {
            let Some(d)=iter.next().await? else { break };
            let p=d.peer();
            out.push(DialogDto {
                id:p.id().value(), name:p.name().unwrap_or_default().to_owned(),
                username:match p { grammers_client::peer::Peer::User(ref u)=>u.username().map(str::to_owned), grammers_client::peer::Peer::Channel(ref c)=>c.username().map(str::to_owned), _=>None },
                kind:match p { grammers_client::peer::Peer::User(_)=>"user", grammers_client::peer::Peer::Group(_)=>"group", grammers_client::peer::Peer::Channel(_)=>"channel" }.into(),
                last_message:d.last_message.as_ref().map(Self::message_dto), unread_count:0,
            });
        }
        Ok(out)
    }

    pub async fn messages(&self, peer:&str, limit:usize, offset_id:i32)->Result<MessagesResponse>{
        let peer_ref=self.peer_ref(peer).await?; let take=limit.clamp(1,100);
        let mut iter=self.client.iter_messages(peer_ref).offset_id(offset_id).limit(take); let mut messages=Vec::with_capacity(take);
        while let Some(m)=iter.next().await? { messages.push(Self::message_dto(&m)); }
        messages.reverse(); Ok(MessagesResponse{has_more:messages.len()==take,messages})
    }

    pub async fn send_message(&self, peer:&str, text:&str, reply_to:Option<i32>)->Result<SendMessageResponse>{
        let peer_ref=self.peer_ref(peer).await?;
        let input=grammers_client::message::InputMessage::new().text(text).reply_to(reply_to);
        let m=self.client.send_message(peer_ref,input).await?;
        Ok(SendMessageResponse{message_id:m.id(),peer_id:m.peer_id().value()})
    }

    pub async fn edit_message(&self,peer:&str,message_id:i32,text:&str)->Result<()>{
        let peer_ref=self.peer_ref(peer).await?; self.client.edit_message(peer_ref,message_id,text).await?; Ok(())
    }
    pub async fn delete_messages(&self,peer:&str,ids:&[i32])->Result<usize>{
        let peer_ref=self.peer_ref(peer).await?; Ok(self.client.delete_messages(peer_ref,ids).await?)
    }
    pub async fn forward_messages(&self,source:&str,destination:&str,ids:&[i32])->Result<Vec<MessageDto>>{
        let src=self.peer_ref(source).await?; let dst=self.peer_ref(destination).await?;
        Ok(self.client.forward_messages(dst,ids,src).await?.into_iter().flatten().map(|m|Self::message_dto(&m)).collect())
    }
    pub async fn react(&self,peer:&str,message_id:i32,reaction:Option<&str>)->Result<()>{
        let peer_ref=self.peer_ref(peer).await?;
        match reaction { Some(r) if !r.trim().is_empty()=>self.client.send_reactions(peer_ref,message_id,r).await?, _=>self.client.send_reactions(peer_ref,message_id,grammers_client::message::InputReactions::remove()).await? }
        Ok(())
    }
    pub async fn mark_read(&self,peer:&str)->Result<()>{
        let peer_ref=self.peer_ref(peer).await?; self.client.mark_as_read(peer_ref).await?; Ok(())
    }
    pub async fn search(&self,peer:Option<&str>,query:&str,limit:usize)->Result<SearchResponse>{
        let take=limit.clamp(1,100); let mut out=Vec::new();
        if let Some(p)=peer {
            let peer_ref=self.peer_ref(p).await?; let mut iter=self.client.search_messages(peer_ref).query(query).limit(take);
            while out.len()<take { let Some(m)=iter.next().await? else {break}; out.push(Self::message_dto(&m)); }
        } else {
            let mut iter=self.client.search_all_messages().query(query);
            while out.len()<take { let Some(m)=iter.next().await? else {break}; out.push(Self::message_dto(&m)); }
        }
        Ok(SearchResponse{messages:out})
    }
    pub async fn send_file(&self, peer:&str, path:&std::path::Path, caption:&str)->Result<SendMessageResponse>{
        let peer_ref=self.peer_ref(peer).await?;
        let uploaded=self.client.upload_file(path).await?;
        let name=path.file_name().and_then(|x|x.to_str()).unwrap_or("file");
        let input=grammers_client::message::InputMessage::new().text(caption).mime_type(mime_guess::from_path(name).first_or_octet_stream().essence_str()).file(uploaded);
        let m=self.client.send_message(peer_ref,input).await?;
        Ok(SendMessageResponse{message_id:m.id(),peer_id:m.peer_id().value()})
    }

    pub async fn login_start(&self,phone:&str,api_hash:&str)->Result<grammers_client::client::LoginToken>{ Ok(self.client.request_login_code(phone,api_hash).await?) }
    pub async fn login_code(&self,t:&grammers_client::client::LoginToken,code:&str)->std::result::Result<grammers_client::peer::User,SignInError>{ self.client.sign_in(t,code).await }
    pub async fn login_password(&self,t:grammers_client::client::PasswordToken,password:&str)->std::result::Result<grammers_client::peer::User,SignInError>{ self.client.check_password(t,password).await }
}
