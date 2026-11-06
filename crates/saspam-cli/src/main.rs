//! Administrative CLI: only the sshd PAM stack is modified.
mod interactive;
mod settings;
use saspam_protocol::{PamCommand, Reply, PAM_SOCKET, VERSION};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, IsTerminal, Write},
    os::unix::{fs::{MetadataExt, PermissionsExt, OpenOptionsExt}, net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, SystemTime}
};

const PAM_FILE: &str = "/etc/pam.d/sshd";
const PAM_MODULE: &str = "/usr/local/lib/security/pam_saspam.so";
const LAST_TEST: &str = "/var/lib/saspam/approved-test";
const MARKER: &str = "account required /usr/local/lib/security/pam_saspam.so # DisnanaSASPAM-managed";
const HARDENING_FILE: &str = "/etc/ssh/sshd_config.d/00-saspam-hardening.conf";
const HARDENING: &str = "# DisnanaSASPAM-managed; remove with: saspam security rollback\nMaxAuthTries 4\nLoginGraceTime 45\nMaxStartups 10:30:60\n";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("saspam: {e}"); ExitCode::FAILURE }
    }
}
fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() && io::stdin().is_terminal() { return interactive::main_menu(); }
    match args.first().map(String::as_str).unwrap_or("help") {
        "help" | "--help" | "-h" => { help(); Ok(()) }
        "setup" => setup(),
        "menu" => interactive::main_menu(),
        "config" => settings::config_command(&args[1..]),
        "notify" => settings::notify_command(&args[1..]),
        "status" => status(),
        "doctor" => doctor(),
        "test" => test_login(),
        "enable" => enable(args),
        "disable" => disable(),
        "uninstall" => uninstall(args),
        "security" => security(&args[1..]),
        cmd => Err(format!("unknown subcommand: {cmd}; run saspam help")),
    }
}
fn help() {
    let lang = settings::current_language();
    let common = "\n  sudo saspam doctor / test / status\n  sudo saspam enable --ack-recovery --ack-tested\n  sudo saspam disable\n  sudo saspam security audit|apply|rollback\n  sudo saspam uninstall --yes";
    if lang == "en" {
        println!("DisnanaSASPAM v{} | SSH post-authentication approval\n  saspam / saspam menu            Interactive dashboard\n  sudo saspam setup               Initial setup / language\n  sudo saspam config              Partial settings menu\n  sudo saspam config show         Display current settings\n  sudo saspam config set language ja|en [--ack-interruption]\n  sudo saspam config set mentions roles|everyone|none\n  sudo saspam config set ntfy on|off|topic [VALUE]\n  sudo saspam config set bark on|off|key\n  sudo saspam notify test ntfy|bark|all{}\nConsole, sudo, su are not modified.",env!("CARGO_PKG_VERSION"),common);
    } else {
        println!("DisnanaSASPAM v{} | SSH認証成功後の追加承認\n  saspam / saspam menu            対話形式の管理メニュー\n  sudo saspam setup               初期設定・言語選択\n  sudo saspam config              一部設定だけを変更\n  sudo saspam config show         現在の設定\n  sudo saspam config set language ja|en [--ack-interruption]\n  sudo saspam config set mentions roles|everyone|none\n  sudo saspam config set ntfy on|off|topic [値]\n  sudo saspam config set bark on|off|key\n  sudo saspam notify test ntfy|bark|all{}\n物理コンソール・sudo・suは変更しません",env!("CARGO_PKG_VERSION"),common);
    }
}
fn root() -> Result<(), String> {
    let output = Command::new("id").arg("-u").output().map_err(|e| e.to_string())?;
    if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0" { Ok(()) }
    else { Err("run as root: sudo saspam ...".into()) }
}
fn ask(prompt: &str, default: &str) -> Result<String, String> {
    print!("{prompt} [{default}]: ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    let answer = line.trim();
    Ok(if answer.is_empty() { default.to_string() } else { answer.to_string() })
}
fn ask_yes(prompt: &str) -> Result<bool, String> { Ok(ask(prompt, "no")?.eq_ignore_ascii_case("yes")) }
fn snowflake(id: &str) -> bool { (15..=22).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_digit()) }
fn parse_role_ids(input: &str) -> Result<Vec<String>, String> {
    let roles: Vec<String> = input.split(',').map(|id| id.trim().to_string()).collect();
    if roles.is_empty() || roles.len() > 32 || roles.iter().any(|id| !snowflake(id)) {
        return Err("enter 1-32 valid Discord role IDs, separated by commas".into());
    }
    let distinct: std::collections::HashSet<_> = roles.iter().collect();
    if roles.len() != distinct.len() {
        return Err("duplicate Discord role IDs are not allowed".into());
    }
    Ok(roles)
}
fn toml_string(s: &str) -> String { serde_json::to_string(s).unwrap_or_default() }
fn write_private(path: &Path, data: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("invalid path")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp).map_err(|e| e.to_string())?;
    let result = (|| -> io::Result<()> { file.write_all(data.as_bytes())?; file.sync_all()?; fs::rename(&temp, path)?; Ok(()) })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result.map_err(|e| e.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(())
}
// Preserves the original root-owned PAM file mode. Does not modify common-* files.
fn write_pam(content: &str) -> Result<(), String> {
    let path = Path::new(PAM_FILE);
    let md = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !md.file_type().is_file() || md.uid() != 0 || md.gid() != 0 {
        return Err("refusing to rewrite non-regular or non-root-owned PAM file".into());
    }
    let dir = path.parent().ok_or("invalid PAM path")?;
    let temp = dir.join(".sshd.saspam.new");
    let mut file = OpenOptions::new().create_new(true).write(true).mode(md.mode() & 0o777).open(&temp).map_err(|e| e.to_string())?;
    let result = (|| -> io::Result<()> {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result.map_err(|e| e.to_string())
}
// Dedicated writer: never chmod /etc/ssh/sshd_config.d like a secrets directory.
fn write_hardening(path: &Path, data: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("invalid hardening path")?;
    let metadata = fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.uid() != 0 {
        return Err("SSH config directory is not an ordinary root-owned directory".into());
    }
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o644).open(path)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(data.as_bytes()).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(e.to_string());
    }
    Ok(())
}
fn recent_approval_test() -> bool {
    let test_time = fs::metadata(LAST_TEST).and_then(|m| m.modified());
    let config_time = fs::metadata("/etc/saspam/config.toml").and_then(|m| m.modified());
    match (test_time, config_time, SystemTime::now()) {
        (Ok(test), Ok(config), now) =>
            test >= config && now.duration_since(test).is_ok_and(|elapsed| elapsed <= Duration::from_secs(900)),
        _ => false,
    }
}
fn setup() -> Result<(), String> {
    root()?;
    if fs::read_to_string(PAM_FILE).unwrap_or_default().contains("pam_saspam.so") {
        return Err("disable SASPAM before changing its configuration; preserve a recovery session".into());
    }
    let lang=interactive::initial_language(&settings::current_language())?;
    println!("{}",interactive::t(&lang,"SASPAM初期設定 — Discord承認・通知の設定","SASPAM setup — Discord approval and notifications"));
    let server = ask(interactive::t(&lang,"サーバー名","Server name"), "ubuntu-server")?;
    if server.len() > 128 || server.trim().is_empty() || server.contains(['\n','\r']) { return Err("invalid server name".into()); }
    let timeout = ask(interactive::t(&lang,"承認待機秒数 (10-180秒)","Approval timeout (10-180 seconds)"), "60")?.parse::<u64>().map_err(|_| "invalid timeout")?;
    if !(10..=180).contains(&timeout) { return Err("timeout must be 10-180".into()); }
    let guild = ask(interactive::t(&lang,"DiscordサーバーID（数字）","Discord server/guild ID (numeric)"), "")?;
    if !snowflake(&guild) { return Err("Discord guild ID must be numeric".into()); }
    let channel = ask(interactive::t(&lang,"Discord通知チャンネルID（数字）","Discord notification channel ID (numeric)"), "")?;
    if !snowflake(&channel) { return Err("Discord channel ID must be numeric".into()); }
    let roles = parse_role_ids(&ask(interactive::t(&lang,"承認ロールID（カンマ区切り・いずれか一致）","Approval role IDs (comma separated, ANY match)"), "")?)?;
    if roles.contains(&guild) { return Err("Discord @everyone role cannot approve SSH logins; choose a dedicated role".into()); }
    let roles_toml = roles.iter().map(|id| toml_string(id)).collect::<Vec<_>>().join(", ");
    let roles_env = roles.join(",");
    println!("{}",interactive::t(&lang,"通知時にメンションする対象を選んでください","Select notification mentions"));
    println!("{}",interactive::t(&lang,"  1: 承認ロールすべて（推奨）","  1: All approval roles (recommended)"));
    println!("{}",interactive::t(&lang,"  2: @everyone（明示的な許可が必要）","  2: @everyone (explicit confirmation)"));
    println!("{}",interactive::t(&lang,"  3: メンションなし","  3: No mention"));
    let mention_mode = match ask(interactive::t(&lang,"メンション方式 (1/2/3)","Mention mode (1/2/3)"), "1")?.as_str() {
        "1" => "roles",
        "2" => {
            println!("警告: @everyoneはこのチャンネルの閲覧可能な全員に通知を送信します");
            if !ask_yes("@everyoneの通知を明示的に有効化しますか? (yes/no)")? {
                return Err("@everyoneメンションは有効化されませんでした".into());
            }
            "everyone"
        }
        "3" => "none",
        _ => return Err("メンション方式は1, 2, 3から選択してください".into()),
    };
    let token = rpassword::prompt_password(interactive::t(&lang,"Discord Botトークン（非表示）: ","Discord Bot Token (hidden): ")).map_err(|e| e.to_string())?;
    if token.len() < 30 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_') { return Err("invalid Discord Bot token format".into()); }
    let ntfy = ask_yes(interactive::t(&lang,"ntfy通知を有効にしますか？ (yes/no)","Enable ntfy notifications? (yes/no)"))?;
    let mut ntfy_topic = String::new();
    if ntfy {
        ntfy_topic = ask(interactive::t(&lang,"ntfyトピック（24文字以上）","ntfy topic (>=24 random characters)"), "")?;
        if !(24..=64).contains(&ntfy_topic.len()) || !ntfy_topic.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') { return Err("ntfy topic must be 24-64 letters/digits/_/-".into()); }
    }
    let bark = ask_yes(interactive::t(&lang,"Bark通知を有効にしますか？ (yes/no)","Enable Bark notifications? (yes/no)"))?;
    let bark_key = if bark { rpassword::prompt_password(interactive::t(&lang,"Barkデバイスキー（非表示）: ","Bark device key (hidden): ")).map_err(|e| e.to_string())? } else { String::new() };
    if bark && (bark_key.is_empty() || bark_key.contains(['\n','\r','\"'])) { return Err("invalid Bark device key".into()); }
    let config = format!("# Owned by root. Do not commit this file.\n[general]\nserver_name = {}\ntimeout_seconds = {}\nlanguage = {}\n\n[discord]\nenabled = true\nguild_id = {}\nchannel_id = {}\nallowed_role_ids = [{}]\n\n[ntfy]\nenabled = {}\nserver = \"https://ntfy.sh\"\ntopic = {}\n\n[bark]\nenabled = {}\nserver = \"https://api.day.app\"\ndevice_key = {}\n", toml_string(&server), timeout, toml_string(&lang), toml_string(&guild), toml_string(&channel), roles_toml, ntfy, toml_string(&ntfy_topic), bark, toml_string(&bark_key));
    let env_file = format!("DISCORD_TOKEN={}\nDISCORD_GUILD_ID={}\nDISCORD_CHANNEL_ID={}\nDISCORD_APPROVAL_ROLE_IDS={}\nDISCORD_MENTION_MODE={}\nDISCORD_LANGUAGE={}\n", token, guild, channel, roles_env, mention_mode, lang);
    let path = Path::new("/etc/saspam/config.toml");
    if path.exists() { fs::copy(path, "/etc/saspam/config.toml.bak").map_err(|e| e.to_string())?; }
    write_private(path, &config)?;
    write_private(Path::new("/etc/saspam/discord.env"), &env_file)?;
    let _ = fs::remove_file(LAST_TEST);
    let result = Command::new("systemctl").args(["enable", "--now", "saspamd.service", "saspam-discord.service"]).status().map_err(|e| e.to_string())?;
    if !result.success() { return Err("configuration saved but service startup failed; run journalctl -u saspamd -u saspam-discord".into()); }
    let restart = Command::new("systemctl").args(["restart", "saspamd.service", "saspam-discord.service"]).status().map_err(|e| e.to_string())?;
    if !restart.success() { return Err("services failed to restart with new configuration".into()); }
    println!("{}",interactive::t(&lang,"✓ 設定完了。次は sudo saspam doctor と sudo saspam test を実行してください","✓ Setup complete. Next run sudo saspam doctor && sudo saspam test"));
    Ok(())
}
fn probe() -> Result<(), String> {
    let mut stream = UnixStream::connect(PAM_SOCKET).map_err(|e| format!("daemon unreachable: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    let request = serde_json::to_string(&PamCommand::Probe {version: VERSION}).map_err(|e| e.to_string())?;
    stream.write_all(format!("{request}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).map_err(|e| e.to_string())?;
    let reply: Reply = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if reply.ok { Ok(()) } else { Err(format!("daemon returned: {:?}", reply.error)) }
}
fn sshd_check(args: &[&str]) -> Result<String, String> {
    let output = Command::new("/usr/sbin/sshd").args(args).output().map_err(|e| e.to_string())?;
    if !output.status.success() { return Err(format!("sshd {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr))); }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
fn sshd_setting(key: &str) -> Result<String, String> {
    let config = sshd_check(&["-T"])?;
    config.lines().find_map(|l| l.strip_prefix(&format!("{key} ")).map(str::to_owned)).ok_or_else(|| format!("sshd did not report {key}"))
}
fn configured() -> bool { Path::new("/etc/saspam/config.toml").exists() && Path::new("/etc/saspam/discord.env").exists() }
fn service_active(name: &str) -> bool {
    Command::new("systemctl").args(["is-active", "--quiet", name]).status().map(|s| s.success()).unwrap_or(false)
}
fn doctor() -> Result<(), String> {
    root()?;
    let lang = settings::current_language();
    println!("{}",interactive::t(&lang,"DisnanaSASPAM 診断","DisnanaSASPAM doctor"));
    sshd_check(&["-t"])?;
    println!("[OK] sshd syntax");
    if sshd_setting("usepam")?.trim() != "yes" { return Err("sshd UsePAM is not yes; account hook will not run".into()); }
    println!("[OK] UsePAM=yes (verify host-specific Match blocks separately)");
    if !configured() { return Err("run sudo saspam setup first".into()); }
    println!("[OK] config and Discord env exist");
    if !Path::new(PAM_MODULE).exists() { return Err(format!("PAM module missing: {PAM_MODULE}")); }
    println!("[OK] PAM module installed");
    probe()?;
    println!("[OK] daemon IPC");
    if !service_active("saspam-discord") { return Err("Discord Botが停止しています。systemctl status saspam-discord --no-pager -l / journalctl -u saspam-discord -n 80 で確認してください（SSH保護は有効化しないでください）".into()); }
    println!("[OK] Discord Bot service active (not a substitute for an approval test)");
    Ok(())
}
fn status() -> Result<(), String> {
    println!("SASPAM PAM enabled: {}", fs::read_to_string(PAM_FILE).unwrap_or_default().lines().any(|l|l == MARKER));
    println!("saspamd active: {}", service_active("saspamd"));
    println!("Discord Bot active: {}", service_active("saspam-discord"));
    println!("Config present: {}", configured());
    Ok(())
}
fn test_login() -> Result<(), String> {
    root()?;
    // Do not hang waiting for Discord when the Bot has not started.
    doctor()?;
    let user = env::var("SUDO_USER").unwrap_or_else(|_| "root".into());
    println!("Discord承認テストを開始します（PAM設定は変更しません）");
    println!("Discordの承認メッセージから「承認」または「拒否」を選んでください");
    let mut stream = UnixStream::connect(PAM_SOCKET).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(195))).map_err(|e| e.to_string())?;
    let cmd = PamCommand::Authorize {version:VERSION, user, service:"sshd".into(), remote_host:"SASPAM-CLI-TEST".into()};
    stream.write_all(format!("{}\n",serde_json::to_string(&cmd).map_err(|e|e.to_string())?).as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    BufReader::new(stream).read_line(&mut buf).map_err(|e|e.to_string())?;
    let reply: Reply = serde_json::from_str(&buf).map_err(|e|e.to_string())?;
    if reply.ok && reply.status == Some(saspam_protocol::Status::Approved) {
        write_private(Path::new(LAST_TEST), "passed\n")?;
        println!("[OK] Approval delivered to PAM gate (receipt valid 15 minutes)");
        Ok(())
    } else { Err(format!("test not approved: {:?} {:?}", reply.status, reply.error)) }
}
fn enable(args: &[String]) -> Result<(), String> {
    root()?;
    if !args.iter().any(|x|x=="--ack-recovery") || !args.iter().any(|x|x=="--ack-tested") {
        return Err("complete a test and confirm console/SSH recovery; then use --ack-recovery --ack-tested".into());
    }
    doctor()?;
    if !recent_approval_test() { return Err("no successful approval test within 15 minutes of current config; run sudo saspam test".into()); }
    let src = fs::read_to_string(PAM_FILE).map_err(|e|e.to_string())?;
    if src.lines().any(|l|l == MARKER) { println!("SASPAM already enabled"); return Ok(()); }
    if src.contains("pam_saspam.so") { return Err("unknown pre-existing SASPAM PAM entry; manual review required".into()); }
    let mut lines: Vec<String> = src.lines().map(str::to_owned).collect();
    let pos = lines.iter().position(|l| { let t=l.trim(); t.starts_with("account ") || t.starts_with("@include common-account") }).ok_or("cannot identify PAM account stack; aborting")?;
    lines.insert(pos, MARKER.into());
    let mut revised = lines.join("\n"); revised.push('\n');
    let backup = PathBuf::from("/var/lib/saspam/sshd.pam.backup");
    fs::create_dir_all(backup.parent().ok_or("invalid backup directory")?).map_err(|e|e.to_string())?;
    if !backup.exists() { write_private(&backup, &src)?; }
    write_pam(&revised)?;
    // sshd -t does not validate PAM semantics; manual SSH test is still required.
    if let Err(e) = sshd_check(&["-t"]) { write_pam(&src)?; return Err(e); }
    println!("SASPAM enabled for SSH ONLY. Keep current session; verify a NEW SSH login before disconnecting.");
    Ok(())
}
fn disable() -> Result<(), String> {
    root()?;
    let src = fs::read_to_string(PAM_FILE).map_err(|e|e.to_string())?;
    let mut n = 0;
    let mut result = String::new();
    for line in src.lines() {
        if line == MARKER { n += 1; continue; }
        result.push_str(line); result.push('\n');
    }
    if n == 0 { println!("SASPAM PAM hook already absent"); return Ok(()); }
    if n != 1 { return Err("multiple PAM hooks found; refusing to change automatically".into()); }
    write_pam(&result)?;
    println!("Removed SASPAM account hook; other PAM entries left untouched");
    Ok(())
}
fn reload_ssh() -> Result<(), String> {
    let status=Command::new("systemctl").args(["reload","ssh"]).status().map_err(|e| e.to_string())?;
    if !status.success() { return Err("failed to reload ssh; check service status".into()); }
    Ok(())
}
fn security(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str).unwrap_or("audit") {
        "audit" => {
            for name in ["usepam","passwordauthentication","pubkeyauthentication","permitrootlogin","maxauthtries","logingracetime","maxstartups"] {
                println!("{name} = {}", sshd_setting(name)?);
            }
            println!("Fail2ban active: {}", service_active("fail2ban"));
            println!("Optional safeguards are NEVER enabled automatically.");
            Ok(())
        }
        "apply" => {
            root()?;
            if !args.iter().any(|x| x=="--ack-recovery") { return Err("provide --ack-recovery after ensuring out-of-band console access".into()); }
            sshd_check(&["-t"])?;
            let dest = Path::new(HARDENING_FILE);
            if dest.exists() { return Err("SASPAM hardening file already exists; use rollback first".into()); }
            fs::create_dir_all(dest.parent().ok_or("invalid path")?).map_err(|e| e.to_string())?;
            write_hardening(dest, HARDENING)?;
            let validation = (|| -> Result<(), String> {
                sshd_check(&["-t"])?;
                for (k,v) in [("maxauthtries","4"),("logingracetime","45"),("maxstartups","10:30:60")] {
                    if sshd_setting(k)? != v { return Err(format!("{k} overridden by another sshd configuration; no changes kept")); }
                }
                Ok(())
            })();
            if let Err(e) = validation { let _ = fs::remove_file(dest); return Err(e); }
            if let Err(e) = reload_ssh() { let _ = fs::remove_file(dest); return Err(e); }
            println!("Applied SSH pre-auth limits. PasswordAuthentication and PermitRootLogin unchanged.");
            Ok(())
        }
        "rollback" => {
            root()?;
            let path=Path::new(HARDENING_FILE);
            if !path.exists() { println!("No SASPAM hardening file exists"); return Ok(()); }
            if fs::read_to_string(path).map_err(|e|e.to_string())? != HARDENING { return Err("hardening file was edited; refusing to overwrite manual changes".into()); }
            fs::remove_file(path).map_err(|e| e.to_string())?;
            if let Err(e) = sshd_check(&["-t"]).and_then(|_| reload_ssh()) {
                // Keep the original hardening policy on unexpected rollback failures.
                write_hardening(path, HARDENING)?;
                return Err(e);
            }
            println!("SSH hardening changes removed");
            Ok(())
        }
        _ => Err("usage: saspam security [audit|apply --ack-recovery|rollback]".into()),
    }
}
fn uninstall(args: &[String]) -> Result<(), String> {
    root()?;
    if !args.iter().any(|x| x=="--yes") { return Err("uninstall requires --yes".into()); }
    disable()?;
    if Path::new(HARDENING_FILE).exists() { security(&["rollback".into()])?; }
    for service in ["saspam-discord", "saspamd"] {
        let _ = Command::new("systemctl").args(["disable","--now",service]).status();
    }
    let script = "/usr/local/lib/saspam/uninstall.sh";
    if Path::new(script).exists() {
        let result = Command::new(script).status().map_err(|e| e.to_string())?;
        if !result.success() { return Err("uninstall helper failed; module is disabled but some files remain".into()); }
    } else {
        println!("SASPAM hook disabled and services stopped. No packaged uninstall helper found; remove files manually.");
    }
    Ok(())
}

#[cfg(test)]
mod role_tests {
    use super::*;
    #[test]
    fn comma_separated_roles_are_trimmed() {
        let ids = parse_role_ids("123456789012345678, 987654321098765432").unwrap();
        assert_eq!(ids, ["123456789012345678", "987654321098765432"]);
    }
    #[test]
    fn invalid_or_duplicate_roles_fail_closed() {
        assert!(parse_role_ids("").is_err());
        assert!(parse_role_ids("123456789012345678,").is_err());
        assert!(parse_role_ids("123456789012345678,123456789012345678").is_err());
        assert!(parse_role_ids("123456789012345678,hello").is_err());
    }
}
