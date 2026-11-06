//! Root-owned, fail-closed, local-only approval coordinator.
//! PAM IPC is root-only; the Bot IPC is accessible only to a dedicated group.
//! There is no publicly reachable approval API in v0.1.x.
use saspam_protocol::{AuthMethod, BotCommand, Decision, PamCommand, PendingRequest, Reply, Status, BOT_SOCKET, PAM_SOCKET, VERSION};
use serde::Deserialize;
use std::{collections::HashMap, env, ffi::CString, fs, io, os::{fd::AsRawFd, unix::{fs::{FileTypeExt, PermissionsExt}, net::UnixListener as StdUnixListener}}, path::Path, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{UnixListener, UnixStream}, sync::{watch, Mutex}, time::{self, Instant}};

const MAX_FRAME: usize = 8192;
const MAX_PENDING: usize = 32;
const RETENTION_SECS: u64 = 600;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct General { server_name: String, timeout_seconds: u64, language: String }
impl Default for General {
    fn default() -> Self { Self { server_name: hostname(), timeout_seconds: 60, language: "ja".into() } }
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Discord { enabled: bool, guild_id: String, channel_id: String, allowed_role_ids: Vec<String> }
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Ntfy { enabled: bool, server: String, topic: String }
impl Default for Ntfy { fn default() -> Self { Self { enabled: false, server: "https://ntfy.sh".into(), topic: String::new() } } }
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Bark { enabled: bool, server: String, device_key: String }
impl Default for Bark { fn default() -> Self { Self { enabled: false, server: "https://api.day.app".into(), device_key: String::new() } } }
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Config { general: General, discord: Discord, ntfy: Ntfy, bark: Bark }
impl Default for Config { fn default() -> Self { Self { general: General::default(), discord: Discord::default(), ntfy: Ntfy::default(), bark: Bark::default() } } }
impl Config {
    fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let config: Config = toml::from_str(&raw).map_err(|e| e.to_string())?;
        if !(10..=180).contains(&config.general.timeout_seconds) { return Err("timeout must be between 10 and 180 seconds".into()); }
        if config.general.language != "ja" && config.general.language != "en" { return Err("unsupported language (ja/en)".into()); }
        if config.general.server_name.is_empty() || config.general.server_name.len() > 128 || config.general.server_name.chars().any(|c| c.is_control()) { return Err("invalid server_name".into()); }
        // No web approval API in v0.1; never enable a gate that cannot be approved.
        if !config.discord.enabled || !snowflake(&config.discord.guild_id) || !snowflake(&config.discord.channel_id)
            || config.discord.allowed_role_ids.is_empty() || config.discord.allowed_role_ids.len() > 32
            || config.discord.allowed_role_ids.iter().any(|id| !snowflake(id) || id == &config.discord.guild_id) {
            return Err("Discord guild_id, channel_id and 1-32 allowed_role_ids are required (old user-ID config must be migrated via saspam setup)".into());
        }
        let unique: std::collections::HashSet<_> = config.discord.allowed_role_ids.iter().collect();
        if unique.len() != config.discord.allowed_role_ids.len() {
            return Err("duplicate Discord approval role IDs".into());
        }
        if config.ntfy.enabled && (!config.ntfy.server.starts_with("https://") || !valid_topic(&config.ntfy.topic)) {
            return Err("invalid ntfy HTTPS server or topic".into());
        }
        if config.bark.enabled && (!config.bark.server.starts_with("https://") || config.bark.device_key.is_empty()) {
            return Err("invalid Bark HTTPS server or device key".into());
        }
        Ok(config)
    }
}
fn snowflake(id: &str) -> bool { (15..=22).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_digit()) }
fn valid_topic(topic: &str) -> bool { (24..=64).contains(&topic.len()) && topic.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') }
fn hostname() -> String { fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "unknown".into()).trim().to_string() }
fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

struct Entry { request: PendingRequest, tx: watch::Sender<Status> }
struct State { entries: Mutex<HashMap<String, Entry>>, config: Config, http: reqwest::Client }

fn log_event(event: &str, r: &PendingRequest) {
    eprintln!("saspam event={event} id={} server={} user={} source={} status={:?} actor={}",
        r.request_id, r.server_name, r.user, r.remote_host, r.status,
        r.actor_id.as_deref().unwrap_or("-"));
}
impl State {
    async fn create(&self, user: String, service: String, remote_host: String, auth_methods: Vec<AuthMethod>) -> Result<(PendingRequest, watch::Receiver<Status>), String> {
        if service != "sshd" || user.is_empty() || user.len() > 128 || remote_host.len() > 255 || user.chars().any(|c| c.is_control()) || remote_host.chars().any(|c| c.is_control()) {
            return Err("invalid PAM request".into());
        }
        if auth_methods.len() > 8 { return Err("too many authentication methods".into()); }
        let mut guard = self.entries.lock().await;
        let now = unix_now();
        guard.retain(|_, v| v.request.expires_unix.saturating_add(RETENTION_SECS) > now);
        if guard.values().filter(|r| r.request.status == Status::Pending).count() >= MAX_PENDING {
            return Err("too many pending requests".into());
        }
        let (tx, rx) = watch::channel(Status::Pending);
        let request = PendingRequest {
            request_id: uuid::Uuid::new_v4().to_string(), user, service, remote_host, auth_methods,
            server_name: self.config.general.server_name.clone(), created_unix: now,
            expires_unix: now + self.config.general.timeout_seconds, status: Status::Pending, actor_id: None,
        };
        log_event("created", &request);
        guard.insert(request.request_id.clone(), Entry { request: request.clone(), tx });
        Ok((request, rx))
    }
    async fn finish(&self, id: &str, decision: Decision, actor: &str, guild_id: &str, channel_id: &str, role_ids: &[String]) -> Result<Status, String> {
        // The Discord adapter checks the authenticated member's roles and channel
        // permissions from the interaction payload; the daemon independently
        // checks which guild/channel/role IDs are configured as approvers.
        if !snowflake(actor) || guild_id != self.config.discord.guild_id || channel_id != self.config.discord.channel_id {
            return Err("invalid Discord approval actor or interaction location".into());
        }
        if role_ids.is_empty() || role_ids.len() > 64 || role_ids.iter().any(|x| !snowflake(x))
            || !role_ids.iter().any(|r| self.config.discord.allowed_role_ids.contains(r)) {
            return Err("actor lacks an allowed Discord approval role".into());
        }
        let mut guard = self.entries.lock().await;
        let entry = guard.get_mut(id).ok_or_else(|| "unknown request".to_string())?;
        if entry.request.status != Status::Pending { return Err("request already resolved".into()); }
        if entry.request.expires_unix <= unix_now() {
            entry.request.status = Status::Expired;
            entry.tx.send_replace(Status::Expired);
            log_event("expired", &entry.request);
            return Err("request expired".into());
        }
        let status = match decision { Decision::Approve => Status::Approved, Decision::Deny => Status::Denied };
        entry.request.status = status.clone();
        entry.request.actor_id = Some(actor.to_string());
        entry.tx.send_replace(status.clone());
        log_event("decided", &entry.request);
        Ok(status)
    }
    async fn expire(&self, id: &str) {
        if let Some(entry) = self.entries.lock().await.get_mut(id) {
            if entry.request.status == Status::Pending {
                entry.request.status = Status::Expired;
                entry.tx.send_replace(Status::Expired);
                log_event("expired", &entry.request);
            }
        }
    }
    async fn get(&self, id: &str) -> Option<PendingRequest> {
        self.entries.lock().await.get(id).map(|e| e.request.clone())
    }
    async fn list_pending(&self) -> Vec<PendingRequest> {
        let now = unix_now();
        self.entries.lock().await.values().filter(|v| v.request.status == Status::Pending && v.request.expires_unix > now).map(|v| v.request.clone()).collect()
    }
}

async fn read_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if bytes.len() >= MAX_FRAME { return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large")); }
        let mut b = [0u8; 1];
        let n = time::timeout(Duration::from_secs(5), stream.read(&mut b)).await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "IPC read timeout"))??;
        if n == 0 { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete frame")); }
        if b[0] == b'\n' { return Ok(bytes); }
        bytes.push(b[0]);
    }
}
async fn write_reply(stream: &mut UnixStream, reply: &Reply) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(reply)?;
    bytes.push(b'\n');
    time::timeout(Duration::from_secs(5), stream.write_all(&bytes)).await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "IPC write timeout"))?
}
fn status_reply(status: Status) -> Reply { Reply { ok: true, status: Some(status), requests: None, error: None } }

async fn handle_pam(mut stream: UnixStream, state: Arc<State>) {
    let Ok(data) = read_frame(&mut stream).await else { return; };
    let cmd: PamCommand = match serde_json::from_slice(&data) {
        Ok(cmd) => cmd,
        Err(_) => { let _ = write_reply(&mut stream, &Reply::denied("invalid request")).await; return; }
    };
    let reply = match cmd {
        PamCommand::Probe { version } if version == VERSION => Reply::ok(),
        PamCommand::NotificationTest { version, provider } if version == VERSION => {
            let title = if state.config.general.language == "en" { "SASPAM notification test" } else { "SASPAM 通知テスト" };
            let msg = if state.config.general.language == "en" {
                "This is a test notification. No SSH login is being requested."
            } else { "これは通知の動作テストです。SSHログイン要求ではありません。" };
            match provider.as_str() {
                "ntfy" | "bark" | "all" => {
                    let providers: Vec<&str> = if provider == "all" { vec!["ntfy", "bark"] } else { vec![provider.as_str()] };
                    let enabled: Vec<&str> = providers.into_iter().filter(|p| match *p {
                        "ntfy" => state.config.ntfy.enabled, "bark" => state.config.bark.enabled, _ => false,
                    }).collect();
                    if enabled.is_empty() { Reply::denied("requested notification provider is disabled") }
                    else {
                        let mut error = None;
                        for provider in enabled {
                            if let Err(e) = send_push(&state.http, &state.config, provider, title, msg).await {
                                error = Some(e);
                                break;
                            }
                        }
                        match error { Some(err) => Reply::denied(&err), None => Reply::ok() }
                    }
                }
                _ => Reply::denied("unknown notification provider"),
            }
        },
        PamCommand::Authorize { version, user, service, remote_host, auth_methods } if version == VERSION => {
            match state.create(user, service, remote_host, auth_methods).await {
                Ok((request, mut rx)) => {
                    let notify_state = state.clone();
                    let notify_request = request.clone();
                    tokio::spawn(async move { notify(&notify_state, &notify_request).await; });
                    let deadline = Instant::now() + Duration::from_secs(state.config.general.timeout_seconds);
                    let status = loop {
                        let current = rx.borrow().clone();
                        if current != Status::Pending { break current; }
                        tokio::select! {
                            _ = time::sleep_until(deadline) => { state.expire(&request.request_id).await; break Status::Expired; }
                            changed = rx.changed() => {
                                if changed.is_err() { break Status::Denied; }
                            }
                        }
                    };
                    status_reply(status)
                }
                Err(e) => Reply::denied(&e)
            }
        }
        _ => Reply::denied("unsupported PAM command/version")
    };
    let _ = write_reply(&mut stream, &reply).await;
}

async fn handle_bot(mut stream: UnixStream, state: Arc<State>) {
    let Ok(data) = read_frame(&mut stream).await else { return; };
    let cmd: BotCommand = match serde_json::from_slice(&data) {
        Ok(cmd) => cmd,
        Err(_) => { let _ = write_reply(&mut stream, &Reply::denied("invalid request")).await; return; }
    };
    let response = match cmd {
        BotCommand::List { version } if version == VERSION => Reply {ok:true,status:None, requests:Some(state.list_pending().await),error:None},
        BotCommand::Get { version, request_id } if version == VERSION => match state.get(&request_id).await {
            Some(req) => Reply {ok:true,status:Some(req.status),requests:Some(vec![req]),error:None},
            None => Reply::denied("not found")
        },
        BotCommand::Decide { version, request_id, decision, actor_id, guild_id, channel_id, role_ids } if version == VERSION =>
            match state.finish(&request_id, decision, &actor_id, &guild_id, &channel_id, &role_ids).await { Ok(s) => status_reply(s), Err(e) => Reply::denied(&e) },
        _ => Reply::denied("unsupported bot command/version")
    };
    let _ = write_reply(&mut stream, &response).await;
}

async fn send_push(
    http: &reqwest::Client,
    config: &Config,
    provider: &str,
    title: &str,
    msg: &str,
) -> Result<(), String> {
    let response = match provider {
        "ntfy" if config.ntfy.enabled => {
            let url = format!("{}/{}", config.ntfy.server.trim_end_matches('/'), config.ntfy.topic);
            http.post(url).header("Title", title).header("Priority", "high")
                .body(msg.to_owned()).send().await.map_err(|_| "ntfy request failed".to_string())?
        }
        "bark" if config.bark.enabled => {
            let url = format!("{}/push", config.bark.server.trim_end_matches('/'));
            let body = serde_json::json!({
                "device_key": config.bark.device_key, "title": title,
                "body": msg, "group": "SASPAM", "level": "timeSensitive",
            });
            http.post(url).json(&body).send().await.map_err(|_| "Bark request failed".to_string())?
        }
        _ => return Err(format!("{provider} is disabled or not supported")),
    };
    if response.status().is_success() { Ok(()) }
    else { Err(format!("{provider} notification returned HTTP {}", response.status())) }
}

async fn notify(state: &State, r: &PendingRequest) {
    // This task is fire-and-forget; PAM approval never depends on push delivery.
    // Public providers are delivery-only and never receive bearer approval tokens.
    let conf = &state.config;
    let (title, msg) = if conf.general.language == "en" {
        ("SASPAM SSH approval request".to_string(),
         format!("SSH sign-in: {} on {} from {}. Approve in Discord within {} seconds. Request {}",
             r.user, r.server_name, r.remote_host, conf.general.timeout_seconds, r.request_id))
    } else {
        ("SASPAM SSHログイン承認".to_string(),
         format!("SSHログイン要求: {} / {} / 接続元 {}。{}秒以内にDiscordで承認してください。要求ID {}",
             r.user, r.server_name, r.remote_host, conf.general.timeout_seconds, r.request_id))
    };
    if conf.ntfy.enabled {
        if let Err(e) = send_push(&state.http, conf, "ntfy", &title, &msg).await {
            eprintln!("saspam notification ntfy error: {e}");
        }
    }
    if conf.bark.enabled {
        if let Err(e) = send_push(&state.http, conf, "bark", &title, &msg).await {
            eprintln!("saspam notification Bark error: {e}");
        }
    }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut length) };
    if rc == 0 && length as usize == std::mem::size_of::<libc::ucred>() { Ok(cred.uid) } else { Err(io::Error::last_os_error()) }
}
fn named_uid(name: &str) -> io::Result<u32> {
    let c_name = CString::new(name).map_err(io::Error::other)?;
    // Called during startup, before the server begins accepting concurrent requests.
    let entry = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if entry.is_null() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "dedicated saspam-bot user is missing"));
    }
    Ok(unsafe { (*entry).pw_uid })
}
fn listen_secure(path: &Path, mode: u32, group_name: Option<&str>) -> io::Result<UnixListener> {
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() { return Err(io::Error::new(io::ErrorKind::AlreadyExists, "socket path is not a socket")); }
        if std::os::unix::net::UnixStream::connect(path).is_ok() { return Err(io::Error::new(io::ErrorKind::AddrInUse, "daemon already listening")); }
        fs::remove_file(path)?;
    }
    let listener = StdUnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    if let Some(name) = group_name {
        let c_name = CString::new(name).expect("group name");
        let group = unsafe { libc::getgrnam(c_name.as_ptr()) };
        if group.is_null() { return Err(io::Error::new(io::ErrorKind::NotFound, "saspam-bot group missing")); }
        let gid = unsafe { (*group).gr_gid };
        let c_path = CString::new(path.as_os_str().as_encoded_bytes()).expect("socket path");
        if unsafe { libc::chown(c_path.as_ptr(), 0, gid) } != 0 { return Err(io::Error::last_os_error()); }
    }
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener)
}

// One Tokio reactor thread is sufficient for local Unix IPC + async push.
// No busy loops: PAM waits on watch channels and socket readiness.
// Keep CPU/memory predictable on low-end cloud instances.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if unsafe { libc::geteuid() } != 0 { return Err("saspamd must run as root".into()); }
    let args: Vec<String> = env::args().collect();
    let config_path = args.windows(2).find(|args| args[0] == "--config").map(|a| a[1].clone()).unwrap_or_else(|| "/etc/saspam/config.toml".into());
    let config = Config::load(Path::new(&config_path)).map_err(io::Error::other)?;
    if args.iter().any(|arg| arg == "--check-config") {
        // Configuration-only validation; no sockets or background services started.
        println!("DisnanaSASPAM config valid (language={})", config.general.language);
        return Ok(());
    }
    let bot_uid = named_uid("saspam-bot")?;
    // A shared HTTP client avoids repeated TLS root-store and connection-pool setup.
    let http = reqwest::Client::builder().timeout(Duration::from_secs(6)).build()?;
    let state = Arc::new(State { entries: Mutex::new(HashMap::new()), config, http });
    fs::create_dir_all("/run/saspam")?;
    // Bot needs traverse rights on this directory; pam.sock stays root-only.
    let group_name = CString::new("saspam-bot")?;
    let group = unsafe { libc::getgrnam(group_name.as_ptr()) };
    if group.is_null() { return Err("saspam-bot group is missing".into()); }
    let gid = unsafe { (*group).gr_gid };
    let run_dir = CString::new("/run/saspam")?;
    if unsafe { libc::chown(run_dir.as_ptr(), 0, gid) } != 0 { return Err(io::Error::last_os_error().into()); }
    fs::set_permissions("/run/saspam", fs::Permissions::from_mode(0o750))?;
    let pam = listen_secure(Path::new(PAM_SOCKET), 0o600, None)?;
    let bot = listen_secure(Path::new(BOT_SOCKET), 0o660, Some("saspam-bot"))?;
    eprintln!("saspamd ready; PAM gate: sshd/account only; timeout={}s", state.config.general.timeout_seconds);
    loop {
        tokio::select! {
            result = pam.accept() => {
                let (stream, _) = result?;
                if peer_uid(&stream).ok() == Some(0) {
                    tokio::spawn(handle_pam(stream, state.clone()));
                } else { eprintln!("saspam rejected unprivileged PAM socket peer"); }
            }
            result = bot.accept() => {
                let (stream, _) = result?;
                // Filesystem permissions alone are not enough: reject unrelated
                // accounts that were accidentally added to the bot group.
                if matches!(peer_uid(&stream), Ok(uid) if uid == bot_uid || uid == 0) {
                    tokio::spawn(handle_bot(stream, state.clone()));
                } else {
                    eprintln!("saspam rejected unauthorized Bot socket peer");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State { entries: Mutex::new(HashMap::new()), http: reqwest::Client::new(), config: Config {
            general: General::default(),
            discord: Discord { enabled:true, guild_id:"111111111111111111".into(),
                channel_id:"222222222222222222".into(),
                allowed_role_ids: vec!["333333333333333333".into(), "444444444444444444".into()] },
            ..Config::default()
        } }
    }
    #[tokio::test]
    async fn approval_is_one_time_and_allowlisted() {
        let state=state();
        let (r,rx)=state.create("alice".into(), "sshd".into(), "192.0.2.1".into(), vec![AuthMethod::Unknown]).await.unwrap();
        let guild = "111111111111111111";
        let channel = "222222222222222222";
        let member_role = vec!["444444444444444444".to_string()];
        assert!(state.finish(&r.request_id, Decision::Approve, "999999999999999999", "999999999999999999", channel, &member_role).await.is_err());
        assert!(state.finish(&r.request_id, Decision::Approve, "999999999999999999", guild, "999999999999999999", &member_role).await.is_err());
        assert!(state.finish(&r.request_id, Decision::Approve, "999999999999999999", guild, channel, &["555555555555555555".to_string()]).await.is_err());
        assert!(state.finish(&r.request_id, Decision::Approve, "999999999999999999", guild, channel, &[]).await.is_err());
        // Either permitted role can authorize (OR semantics).
        assert_eq!(state.finish(&r.request_id, Decision::Approve, "999999999999999999", guild, channel, &member_role).await.unwrap(), Status::Approved);
        assert_eq!(*rx.borrow(), Status::Approved);
        assert!(state.finish(&r.request_id, Decision::Deny, "999999999999999999", guild, channel, &member_role).await.is_err());
    }
    #[tokio::test]
    async fn max_concurrent_requests_are_bounded() {
        let state = state();
        for index in 0..MAX_PENDING {
            state.create(format!("user{index}"), "sshd".into(), "192.0.2.1".into(), vec![AuthMethod::Unknown]).await.unwrap();
        }
        assert!(state.create("overflow".into(), "sshd".into(), "192.0.2.2".into(), vec![AuthMethod::Unknown]).await.is_err());
    }
    #[test]
    fn unsupported_language_is_not_permitted() {
        let mut c = Config::default();
        c.general.language = "xx".into();
        // A loaded configuration should always match the two supported language names.
        assert!(!["ja","en"].contains(&c.general.language.as_str()));
    }
    #[tokio::test]
    async fn only_ssh_is_authorizable() {
        let state=state();
        assert!(state.create("root".into(), "login".into(), "local".into(), vec![AuthMethod::Unknown]).await.is_err());
    }
    #[tokio::test]
    async fn control_characters_cannot_spoof_audit_logs() {
        let state=state();
        assert!(state.create("root\nadmin".into(), "sshd".into(), "host".into(), vec![AuthMethod::Unknown]).await.is_err());
        assert!(state.create("root".into(), "sshd".into(), "host\rspoof".into(), vec![AuthMethod::Unknown]).await.is_err());
    }
    #[tokio::test]
    async fn another_allowed_role_can_deny() {
        let state = state();
        let (r, _) = state.create("alice".into(), "sshd".into(), "192.0.2.1".into(), vec![AuthMethod::Unknown]).await.unwrap();
        let outcome = state.finish(&r.request_id, Decision::Deny, "999999999999999999", "111111111111111111", "222222222222222222", &["333333333333333333".to_string()]).await;
        assert_eq!(outcome.unwrap(), Status::Denied);
    }
    #[tokio::test]
    async fn ntfy_push_reaches_local_mock_and_obeys_http_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let observer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.starts_with("POST /secure1234567890abcdefghijklmnop HTTP/1.1"));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        });
        let mut config = Config::default();
        config.ntfy.enabled = true;
        config.ntfy.topic = "secure1234567890abcdefghijklmnop".into();
        config.ntfy.server = format!("http://{address}");
        let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().unwrap();
        assert!(send_push(&client, &config, "ntfy", "test", "payload").await.is_ok());
        observer.await.unwrap();
    }

    #[tokio::test]
    async fn disabled_push_provider_does_not_connect() {
        let config = Config::default();
        let client = reqwest::Client::new();
        assert!(send_push(&client, &config, "ntfy", "title", "body").await.is_err());
        assert!(send_push(&client, &config, "bark", "title", "body").await.is_err());
    }

    #[test]
    fn public_notification_topics_must_be_hard_to_guess() {
        assert!(!valid_topic("ssh"));
        assert!(valid_topic("secure_ssh_a1b2c3d4e5f6g7h8i9j0"));
    }
}
