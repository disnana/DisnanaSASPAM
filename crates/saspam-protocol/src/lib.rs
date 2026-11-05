//! Versioned newline-delimited JSON over local UNIX sockets.
//! No PAM credentials or SSH private key material ever cross this boundary.
use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;
pub const PAM_SOCKET: &str = "/run/saspam/pam.sock";
pub const BOT_SOCKET: &str = "/run/saspam/bot.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PamCommand {
    Authorize { version: u8, user: String, service: String, remote_host: String },
    Probe { version: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BotCommand {
    List { version: u8 },
    Get { version: u8, request_id: String },
    Decide { version: u8, request_id: String, decision: Decision, actor_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision { Approve, Deny }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status { Pending, Approved, Denied, Expired }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub request_id: String,
    pub user: String,
    pub service: String,
    pub remote_host: String,
    pub server_name: String,
    pub created_unix: u64,
    pub expires_unix: u64,
    pub status: Status,
    pub actor_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub requests: Option<Vec<PendingRequest>>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub error: Option<String>,
}
impl Reply {
    pub fn ok() -> Self { Self { ok:true, status: None, requests:None, error:None } }
    pub fn denied(reason: &str) -> Self { Self { ok:false, status:None, requests:None, error:Some(reason.to_string()) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_protocol_roundtrip() {
        let value = PamCommand::Authorize { version: VERSION, user:"ubuntu".into(), service:"sshd".into(), remote_host:"192.0.2.50".into() };
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("\"action\":\"authorize\""));
        assert!(matches!(serde_json::from_str::<PamCommand>(&json).unwrap(), PamCommand::Authorize{version:1,..}));
    }
}
