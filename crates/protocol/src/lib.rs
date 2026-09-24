use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse { pub status: String, pub telegram_authorized: bool, pub version: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResponse { pub id: i64, pub username: Option<String>, pub first_name: Option<String>, pub last_name: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogDto { pub id: i64, pub name: String, pub username: Option<String>, pub kind: String, pub last_message: Option<MessageDto> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDto { pub id: i32, pub peer_id: i64, pub text: String, pub outgoing: bool, pub date: Option<DateTime<Utc>> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest { pub peer: String, pub text: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse { pub message_id: i32, pub peer_id: i64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginStartRequest { pub phone: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginStartResponse { pub status: String, pub requires_code: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginCompleteRequest { pub code: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordRequest { pub password: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError { pub error: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Event { NewMessage(MessageDto), Connected, Reconnecting, Status { authorized: bool } }
