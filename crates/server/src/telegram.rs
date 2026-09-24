use std::{path::{Path, PathBuf}, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use grammers_client::{
    types::{InputMessage, InputReactions, LoginToken, Media, Message, PasswordToken, Peer},
    Client, SignInError, Update, UpdatesConfiguration,
};
use grammers_mtsender::SenderPool;
use grammers_session::{defs::PeerRef, storages::SqliteSession, Session};
use tokio::{io::AsyncWriteExt, sync::broadcast, time::sleep};
use telemesh_protocol::{
    DialogDto, Event, MediaDto, MeResponse, MessageDto, MessagesResponse, SearchResponse,
    SendMessageResponse,
};
use tracing::{error, info, warn};

pub struct LoginState {
    pub token: LoginToken,
    pub password: Option<PasswordToken>,
}

pub struct DownloadedMedia {
    pub path: PathBuf,
    pub metadata: MediaDto,
}

#[derive(Clone)]
pub struct TelegramService {
    client: Client,
    session: Arc<SqliteSession>,
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

        let (events, _) = broadcast::channel(2048);
        let service = Self { client, session, events };

        let authorized = service.client.is_authorized().await?;
        info!("Telegram authorized: {authorized}");

        if authorized {
            // Persist dialog peers before update catch-up. grammers requires this
            // cache for reliable getDifference/gap recovery.
            let mut dialogs = service.client.iter_dialogs();
            while dialogs.next().await?.is_some() {}
        }

        let client = service.client.clone();
        let tx = service.events.clone();

        tokio::spawn(async move {
            let mut stream = match client.stream_updates(
                updates,
                UpdatesConfiguration {
                    catch_up: true,
                    update_queue_limit: Some(4096),
                },
            );

            let mut failures = 0u32;
            loop {
                match stream.next().await {
                    Ok(update) => {
                        failures = 0;
                        match update {
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
                                if !message_ids.is_empty() {
                                    let _ = tx.send(Event::MessagesDeleted { peer_id: 0, message_ids });
                                }
                            }
                            Update::Raw(raw) => {
                                if let grammers_client::grammers_tl_types::enums::Update::MessageReactions(reaction) = raw.raw {
                                    let peer_id = match reaction.peer {
                                        grammers_client::grammers_tl_types::enums::Peer::User(p) => p.user_id,
                                        grammers_client::grammers_tl_types::enums::Peer::Chat(p) => -p.chat_id,
                                        grammers_client::grammers_tl_types::enums::Peer::Channel(p) => -1000000000000_i64 - p.channel_id,
                                    };
                                    let _ = tx.send(Event::ReactionUpdated { peer_id, message_id: reaction.msg_id });
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        let delay = Duration::from_millis((250u64.saturating_mul(1u64 << failures.min(5))).min(15_000));
                        warn!("Telegram update stream error (attempt {failures}): {error}; retrying in {delay:?}");
                        let _ = tx.send(Event::Reconnecting);
                        sleep(delay).await;
                        let _ = tx.send(Event::Status { authorized: true });
                    }
                }
            }
        });

        let _ = api_hash;
        Ok(service)
    }

    pub async fn authorized(&self) -> Result<bool> {
        Ok(self.client.is_authorized().await?)
    }

    pub async fn me(&self) -> Result<MeResponse> {
        let u = self.client.get_me().await?;
        Ok(MeResponse {
            id: u.raw.id(),
            username: u.username().map(str::to_owned),
            first_name: u.first_name().map(str::to_owned),
            last_name: u.last_name().map(str::to_owned),
        })
    }

    fn media_dto(media: Media) -> MediaDto {
        match media {
            Media::Document(document) => MediaDto {
                kind: "document".into(),
                filename: (!document.name().is_empty()).then(|| document.name().to_owned()),
                mime: document.mime_type().map(str::to_owned),
                size: None,
            },
            Media::Sticker(sticker) => MediaDto {
                kind: "sticker".into(),
                filename: (!sticker.document.name().is_empty()).then(|| sticker.document.name().to_owned()),
                mime: sticker.document.mime_type().map(str::to_owned),
                size: None,
            },
            Media::Photo(_) => MediaDto { kind: "photo".into(), filename: None, mime: Some("image/jpeg".into()), size: None },
            Media::Contact(_) => MediaDto { kind: "contact".into(), filename: None, mime: Some("text/vcard".into()), size: None },
            Media::Poll(_) => MediaDto { kind: "poll".into(), filename: None, mime: None, size: None },
            Media::Geo(_) | Media::GeoLive(_) => MediaDto { kind: "geo".into(), filename: None, mime: None, size: None },
            Media::Dice(_) => MediaDto { kind: "dice".into(), filename: None, mime: None, size: None },
            Media::Venue(_) => MediaDto { kind: "venue".into(), filename: None, mime: None, size: None },
            Media::WebPage(_) => MediaDto { kind: "webpage".into(), filename: None, mime: None, size: None },
            _ => MediaDto { kind: "media".into(), filename: None, mime: None, size: None },
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

        let peer_id = if let Ok(id) = value.parse::<i64>() {
            let mut iter = self.client.iter_dialogs();
            let mut found = None;
            while let Some(d) = iter.next().await? {
                if d.peer().id().bare_id() == id {
                    found = Some(d.peer().id());
                    break;
                }
            }
            found.ok_or_else(|| anyhow!("peer is not in the dialog cache"))?
        } else {
            let p = self.client.resolve_username(value).await?
                .ok_or_else(|| anyhow!("username not found"))?;
            p.id()
        };

        self.session
            .peer(peer_id)
            .await?
            .map(PeerRef::from)
            .ok_or_else(|| anyhow!("peer reference is not cached"))
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

    pub async fn messages(&self, peer: &str, limit: usize, offset_id: i32) -> Result<MessagesResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let take = limit.clamp(1, 100);
        let mut iter = self.client.iter_messages(peer_ref).offset_id(offset_id).limit(take);
        let mut messages = Vec::with_capacity(take);

        while let Some(m) = iter.next().await? {
            messages.push(Self::message_dto(&m));
        }

        messages.reverse();
        Ok(MessagesResponse { has_more: messages.len() == take, messages })
    }

    pub async fn send_message(&self, peer: &str, text: &str, reply_to: Option<i32>) -> Result<SendMessageResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let input = InputMessage::new().text(text).reply_to(reply_to);
        let m = self.client.send_message(peer_ref, input).await?;
        Ok(SendMessageResponse { message_id: m.id(), peer_id: m.peer_id().bare_id() })
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

    pub async fn forward_messages(&self, source: &str, destination: &str, ids: &[i32]) -> Result<Vec<MessageDto>> {
        let src = self.peer_ref(source).await?;
        let dst = self.peer_ref(destination).await?;
        Ok(self.client.forward_messages(dst, ids, src).await?.into_iter().flatten().map(|m| Self::message_dto(&m)).collect())
    }

    pub async fn react(&self, peer: &str, message_id: i32, reaction: Option<&str>) -> Result<()> {
        let peer_ref = self.peer_ref(peer).await?;
        match reaction {
            Some(r) if !r.trim().is_empty() => self.client.send_reactions(peer_ref, message_id, r).await?,
            _ => self.client.send_reactions(peer_ref, message_id, InputReactions::remove()).await?,
        }
        Ok(())
    }

    pub async fn mark_read(&self, peer: &str) -> Result<()> {
        let peer_ref = self.peer_ref(peer).await?;
        self.client.mark_as_read(peer_ref).await?;
        Ok(())
    }

    pub async fn search(&self, peer: Option<&str>, query: &str, limit: usize) -> Result<SearchResponse> {
        let take = limit.clamp(1, 100);
        let mut out = Vec::new();

        if let Some(p) = peer {
            let peer_ref = self.peer_ref(p).await?;
            let mut iter = self.client.search_messages(peer_ref).query(query).limit(take);
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

    pub async fn download_media_to_file(&self, peer: &str, message_id: i32, path: &Path) -> Result<DownloadedMedia> {
        let peer_ref = self.peer_ref(peer).await?;
        let found = self.client.get_messages_by_id(peer_ref, &[message_id]).await?;
        let message = found.into_iter().next().flatten().ok_or_else(|| anyhow!("message not found"))?;
        let media = message.media().ok_or_else(|| anyhow!("message has no media"))?;

        let metadata = Self::media_dto(media.clone());
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let _ = tokio::fs::remove_file(path).await;
            let mut file = tokio::fs::File::create(path).await?;
            let mut stream = self.client.iter_download(&media);
            let mut total = 0u64;
            let result: Result<()> = async {
                while let Some(chunk) = stream.next().await? {
                    total += chunk.len() as u64;
                    tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
                }
                file.flush().await?;
                Ok(())
            }.await;

            match result {
                Ok(()) => {
                    let mut metadata = metadata;
                    metadata.size = Some(total as i64);
                    return Ok(DownloadedMedia { path: path.to_path_buf(), metadata });
                }
                Err(e) if attempt < 4 => {
                    let delay = Duration::from_millis((250u64.saturating_mul(1u64 << attempt.min(5))).min(8_000));
                    warn!("media download failed (attempt {attempt}): {e}; retrying in {delay:?}");
                    sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub async fn send_file(&self, peer: &str, path: &Path, caption: &str, name: &str, mime: &str) -> Result<SendMessageResponse> {
        let peer_ref = self.peer_ref(peer).await?;
        let size = tokio::fs::metadata(path).await?.len() as usize;
        let mut attempt = 0u32;

        let uploaded = loop {
            attempt += 1;
            let mut file = tokio::fs::File::open(path).await?;
            match self.client.upload_stream(&mut file, size, name.to_owned()).await {
                Ok(uploaded) => break uploaded,
                Err(e) if attempt < 4 => {
                    let delay = Duration::from_millis((250u64.saturating_mul(1u64 << attempt.min(5))).min(8_000));
                    warn!("media upload failed (attempt {attempt}): {e}; retrying in {delay:?}");
                    sleep(delay).await;
                }
                Err(e) => return Err(anyhow!(e)),
            }
        };

        let input = InputMessage::new().text(caption).mime_type(mime).file(uploaded);
        let m = self.client.send_message(peer_ref, input).await?;
        Ok(SendMessageResponse { message_id: m.id(), peer_id: m.peer_id().bare_id() })
    }

    pub async fn login_start(&self, phone: &str, api_hash: &str) -> Result<LoginToken> {
        Ok(self.client.request_login_code(phone, api_hash).await?)
    }

    pub async fn login_code(&self, token: &LoginToken, code: &str) -> std::result::Result<grammers_client::types::User, SignInError> {
        self.client.sign_in(token, code).await
    }

    pub async fn login_password(&self, token: PasswordToken, password: &str) -> std::result::Result<grammers_client::types::User, SignInError> {
        self.client.check_password(token, password.as_bytes()).await
    }
}
