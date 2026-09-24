use std::{path::Path, sync::Arc};
use anyhow::Result;
use futures_util::StreamExt;
use grammers_client::{
    types::{InputMessage, InputReactions, LoginToken, Media, Message, PasswordToken, Peer},
    Client, SignInError, Update, UpdatesConfiguration,
};
use grammers_mtsender::SenderPool;
use grammers_session::{defs::PeerRef, storages::SqliteSession};
use telemesh_protocol::{
    DialogDto, Event, MeResponse, MessageDto, MessagesResponse, SearchResponse,
    SendMessageResponse,
};
use tokio::sync::broadcast;
use tracing::{error, info};

pub struct LoginState {
    pub token: LoginToken,
    pub password: Option<PasswordToken>,
}

#[derive(Clone)]
pub struct TelegramService {
    client: Client,
    events: broadcast::Sender<Event>,
}

impl TelegramService {
    pub async fn connect(api_id: i32, api_hash: String, session_path: &str) -> Result<Self> {
        let session = Arc::new(SqliteSession::open(Path::new(session_path))?);
        let pool = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(&pool);
        let grammers_mtsender::SenderPool { runner, updates, .. } = pool;

        tokio::spawn(async move {
            runner.run().await;
        });

        let (events, _) = broadcast::channel(512);
        let service = Self { client, events };

        info!("Telegram authorized: {}", service.client.is_authorized().await?);

        // Bootstrap the session cache before starting the update processor.
        // grammers requires peers to be present in the session cache for gap recovery.
        if service.client.is_authorized().await? {
            let mut dialogs = service.client.iter_dialogs();
            while dialogs.next().await?.is_some() {}
        }

        let client = service.client.clone();
        let tx = service.events.clone();

        tokio::spawn(async move {
            let mut stream = client.stream_updates(
                updates,
                UpdatesConfiguration {
                    catch_up: true,
                    ..Default::default()
                },
            );

            loop {
                match stream.next().await {
                    Some(Ok(update)) => match update {
                        Update::NewMessage(message) => {
                            let _ = tx.send(Event::NewMessage(Self::message_dto(&message)));
                        }
                        Update::MessageEdited(message) => {
                            let _ = tx.send(Event::MessageEdited(Self::message_dto(&message)));
                        }
                        Update::MessageDeleted(deletion) => {
                            let message_ids = match deletion.raw {
                                grammers_client::grammers_tl_types::enums::Update::DeleteMessages(d) => d.messages,
                                grammers_client::grammers_tl_types::enums::Update::DeleteChannelMessages(d) => d.messages,
                                _ => Vec::new(),
                            };
                            let _ = tx.send(Event::MessagesDeleted { peer_id: 0, message_ids });
                        }
                        _ => {}
                    },
                    Some(Err(error)) => {
                        error!("Telegram update stream error: {error}");
                        let _ = tx.send(Event::Reconnecting);
                        break;
                    }
                    None => {
                        let _ = tx.send(Event::Reconnecting);
                        break;
                    }
                }
            }
        });

        // Keep api_hash in the signature for configuration compatibility.
        let _ = api_hash;
        Ok(service)
    }

    pub async fn authorized(&self) -> Result<bool> {
        Ok(self.client.is_authorized().await?)
    }

    pub async fn me(&self) -> Result<MeResponse> {
        let u = self.client.get_me().await?;
        Ok(MeResponse {
            id: u.id().bare_id(),
            username: u.username().map(str::to_owned),
            first_name: u.first_name().map(str::to_owned),
            last_name: u.last_name().map(str::to_owned),
        })
    }

    fn media_dto(media: &Media) -> telemesh_protocol::MediaDto {
        match media {
            Media::Document(document) => telemesh_protocol::MediaDto {
                kind: "document".into(),
                filename: (!document.name().is_empty()).then(|| document.name().to_owned()),
                mime: document.mime_type().map(str::to_owned),
                size: None,
            },
            Media::Sticker(sticker) => telemesh_protocol::MediaDto {
                kind: "sticker".into(),
                filename: (!sticker.document.name().is_empty())
                    .then(|| sticker.document.name().to_owned()),
                mime: sticker.document.mime_type().map(str::to_owned),
                size: None,
            },
            Media::Photo(_) => telemesh_protocol::MediaDto {
                kind: "photo".into(),
                filename: None,
                mime: Some("image/jpeg".into()),
                size: None,
            },
            Media::Contact(_) => telemesh_protocol::MediaDto {
                kind: "contact".into(),
                filename: None,
                mime: Some("text/vcard".into()),
                size: None,
            },
            Media::Poll(_) => telemesh_protocol::MediaDto {
                kind: "poll".into(),
                filename: None,
                mime: None,
                size: None,
            },
            Media::Geo(_) | Media::GeoLive(_) => telemesh_protocol::MediaDto {
                kind: "geo".into(),
                filename: None,
                mime: None,
                size: None,
            },
            Media::Dice(_) => telemesh_protocol::MediaDto {
                kind: "dice".into(),
                filename: None,
                mime: None,
                size: None,
            },
            Media::Venue(_) => telemesh_protocol::MediaDto {
                kind: "venue".into(),
                filename: None,
                mime: None,
                size: None,
            },
            Media::WebPage(_) => telemesh_protocol::MediaDto {
                kind: "webpage".into(),
                filename: None,
                mime: None,
                size: None,
            },
            _ => telemesh_protocol::MediaDto {
                kind: "media".into(),
                filename: None,
                mime: None,
                size: None,
            },
        }
    }

    fn message_dto(m: &Message) -> MessageDto {
        MessageDto {
            id: m.id(),
            peer_id: m.peer_id().bare_id(),
            text: m.text().to_owned(),
            outgoing: m.outgoing(),
            date: Some(m.date()),
            reply_to: m.reply_to_message_id(),
            edited: m.edit_date().is_some(),
            reaction_count: m.reaction_count(),
            media: m.media().map(Self::media_dto),
        }
    }

    async fn peer_ref(&self, peer: &str) -> Result<PeerRef> {
        let value = peer.trim().trim_start_matches('@');

        if let Ok(id) = value.parse::<i64>() {
            let mut iter = self.client.iter_dialogs();
            while let Some(d) = iter.next().await? {
                if d.peer().id().bare_id() == id {
                    return d
                        .peer()
                        .to_ref()
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("peer cannot be represented"));
                }
            }
            return Err(anyhow::anyhow!("peer is not in the dialog cache"));
        }

        let p = self
            .client
            .resolve_username(value)
            .await?
            .ok_or_else(|| anyhow::anyhow!("username not found"))?;

        match p {
            Peer::User(user) => user
                .to_ref()
                .await?
                .ok_or_else(|| anyhow::anyhow!("peer cannot be represented")),
            Peer::Group(group) => group
                .to_ref()
                .await?
                .ok_or_else(|| anyhow::anyhow!("peer cannot be represented")),
            Peer::Channel(channel) => channel
                .to_ref()
                .await?
                .ok_or_else(|| anyhow::anyhow!("peer cannot be represented")),
        }
    }

    pub async fn dialogs(&self, limit: usize) -> Result<Vec<DialogDto>> {
        let mut iter = self.client.iter_dialogs();
        let mut out = Vec::new();

        while out.len() < limit {
            let Some(d) = iter.next().await? else { break };
            let p = d.peer();

            let (username, kind) = match p {
                Peer::User(u) => (u.username().map(str::to_owned), "user"),
                Peer::Group(_) => (None, "group"),
                Peer::Channel(c) => (c.username().map(str::to_owned), "channel"),
            };

            out.push(DialogDto {
                id: p.id().bare_id(),
                name: p.name().unwrap_or_default().to_owned(),
                username,
                kind: kind.into(),
                last_message: d.last_message.as_ref().map(Self::message_dto),
                unread_count: match &d.raw {
                    grammers_client::grammers_tl_types::enums::Dialog::Dialog(raw) => raw.unread_count,
                    _ => 0,
                },
            });
        }

        Ok(out)
    }

    pub async fn messages(
        &self,
        peer: &str,
        limit: usize,
        offset_id: i32,
    ) -> Result<MessagesResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let take = limit.clamp(1, 100);
        let mut iter = self
            .client
            .iter_messages(peer_ref)
            .offset_id(offset_id)
            .limit(take);
        let mut messages = Vec::with_capacity(take);

        while let Some(m) = iter.next().await? {
            messages.push(Self::message_dto(&m));
        }

        messages.reverse();
        Ok(MessagesResponse {
            has_more: messages.len() == take,
            messages,
        })
    }

    pub async fn send_message(
        &self,
        peer: &str,
        text: &str,
        reply_to: Option<i32>,
    ) -> Result<SendMessageResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let input = InputMessage::new().text(text).reply_to(reply_to);
        let m = self.client.send_message(peer_ref, input).await?;

        Ok(SendMessageResponse {
            message_id: m.id(),
            peer_id: m.peer_id().bare_id(),
        })
    }

    pub async fn edit_message(&self, peer: &str, message_id: i32, text: &str) -> Result<()> {
        let peer_ref = self.peer_ref(peer).await?;
        self.client.edit_message(peer_ref, message_id, text).await?;
        Ok(())
    }

    pub async fn delete_messages(&self, peer: &str, ids: &[i32]) -> Result<usize> {
        let peer_ref = self.peer_ref(peer).await?;
        Ok(self.client.delete_messages(peer_ref, ids).await?)
    }

    pub async fn forward_messages(
        &self,
        source: &str,
        destination: &str,
        ids: &[i32],
    ) -> Result<Vec<MessageDto>> {
        let src = self.peer_ref(source).await?;
        let dst = self.peer_ref(destination).await?;

        Ok(self
            .client
            .forward_messages(dst, ids, src)
            .await?
            .into_iter()
            .flatten()
            .map(|m| Self::message_dto(&m))
            .collect())
    }

    pub async fn react(
        &self,
        peer: &str,
        message_id: i32,
        reaction: Option<&str>,
    ) -> Result<()> {
        let peer_ref = self.peer_ref(peer).await?;

        match reaction {
            Some(r) if !r.trim().is_empty() => {
                self.client
                    .send_reactions(peer_ref, message_id, r)
                    .await?
            }
            _ => {
                self.client
                    .send_reactions(peer_ref, message_id, InputReactions::remove())
                    .await?
            }
        }

        Ok(())
    }

    pub async fn mark_read(&self, peer: &str) -> Result<()> {
        let peer_ref = self.peer_ref(peer).await?;
        self.client.mark_as_read(peer_ref).await?;
        Ok(())
    }

    pub async fn search(
        &self,
        peer: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<SearchResponse> {
        let take = limit.clamp(1, 100);
        let mut out = Vec::new();

        if let Some(p) = peer {
            let peer_ref = self.peer_ref(p).await?;
            let mut iter = self
                .client
                .search_messages(peer_ref)
                .query(query)
                .limit(take);

            while out.len() < take {
                let Some(m) = iter.next().await? else { break };
                out.push(Self::message_dto(&m));
            }
        } else {
            let mut iter = self.client.search_all_messages().query(query);
            while out.len() < take {
                let Some(m) = iter.next().await? else { break };
                out.push(Self::message_dto(&m));
            }
        }

        Ok(SearchResponse { messages: out })
    }

    pub async fn download_media(&self, peer: &str, message_id: i32) -> Result<Vec<u8>> {
        let peer_ref = self.peer_ref(peer).await?;
        let mut found = self.client.get_messages_by_id(peer_ref, &[message_id]).await?;
        let message = found
            .pop()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("message not found"))?;
        let media = message
            .media()
            .ok_or_else(|| anyhow::anyhow!("message has no media"))?;

        let mut stream = self.client.iter_download(&media);
        let mut out = Vec::new();

        while let Some(chunk) = stream.next().await? {
            out.extend_from_slice(&chunk);
            if out.len() > 50 * 1024 * 1024 {
                return Err(anyhow::anyhow!("media exceeds 50 MiB limit"));
            }
        }

        Ok(out)
    }

    pub async fn send_file(
        &self,
        peer: &str,
        path: &std::path::Path,
        caption: &str,
    ) -> Result<SendMessageResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let uploaded = self.client.upload_file(path).await?;
        let name = path.file_name().and_then(|x| x.to_str()).unwrap_or("file");
        let input = InputMessage::new()
            .text(caption)
            .mime_type(
                mime_guess::from_path(name)
                    .first_or_octet_stream()
                    .essence_str(),
            )
            .file(uploaded);

        let m = self.client.send_message(peer_ref, input).await?;

        Ok(SendMessageResponse {
            message_id: m.id(),
            peer_id: m.peer_id().bare_id(),
        })
    }

    pub async fn login_start(&self, phone: &str, api_hash: &str) -> Result<LoginToken> {
        Ok(self.client.request_login_code(phone, api_hash).await?)
    }

    pub async fn login_code(
        &self,
        token: &LoginToken,
        code: &str,
    ) -> std::result::Result<grammers_client::types::User, SignInError> {
        self.client.sign_in(token, code).await
    }

    pub async fn login_password(
        &self,
        token: PasswordToken,
        password: &str,
    ) -> std::result::Result<grammers_client::types::User, SignInError> {
        self.client.check_password(token, password.as_bytes()).await
    }
}
