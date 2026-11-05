//! Administrative CLI: only the sshd PAM stack is modified.
use saspam_protocol::{PamCommand, Reply, PAM_SOCKET, VERSION};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::{fs::{MetadataExt, PermissionsExt}, net::UnixStream},
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
    match args.first().map(String::as_str).unwrap_or("help") {
        "help" | "--help" | "-h" => { help(); Ok(()) }
        "setup" => setup(),
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
    println!("DisnanaSASPAM v{} | SSH post-authentication approval\n\n  sudo saspam setup                 Configure Discord and optional push alerts\n  sudo saspam doctor                Check SSH, daemon, PAM and Bot\n  sudo saspam test                  Real approval round-trip without changing PAM\n  sudo saspam enable --ack-recovery --ack-tested\n  sudo saspam disable               Remove only SASPAM's PAM line\n  sudo saspam status\n  sudo saspam security audit\n  sudo saspam security apply --ack-recovery\n  sudo saspam security rollback\n  sudo saspam uninstall --yes\n\nNo console, sudo or su PAM stacks are modified.", env!("CARGO_PKG_VERSION"));
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
    println!("SASPAM setup: Discord approval is required in v0.1; ntfy and Bark are alert-only");
    let server = ask("Server name", "ubuntu-server")?;
    if server.len() > 128 || server.trim().is_empty() || server.contains(['\n','\r']) { return Err("invalid server name".into()); }
    let timeout = ask("Approval timeout (10-180 seconds)", "60")?.parse::<u64>().map_err(|_| "invalid timeout")?;
    if !(10..=180).contains(&timeout) { return Err("timeout must be 10-180".into()); }
    let allowed = ask("Allowed Discord user ID (numeric)", "")?;
    if !snowflake(&allowed) { return Err("Discord user ID must be a numeric snowflake".into()); }
    let channel = ask("Discord channel ID (numeric)", "")?;
    if !snowflake(&channel) { return Err("Discord channel ID must be a numeric snowflake".into()); }
    let token = rpassword::prompt_password("Discord Bot Token (hidden): ").map_err(|e| e.to_string())?;
    if token.len() < 30 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_') { return Err("invalid Discord Bot token format".into()); }
    let ntfy = ask_yes("Enable ntfy notification alerts? (yes/no)")?;
    let mut ntfy_topic = String::new();
    if ntfy {
        ntfy_topic = ask("ntfy topic (use >=24 random characters)", "")?;
        if !(24..=64).contains(&ntfy_topic.len()) || !ntfy_topic.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') { return Err("ntfy topic must be 24-64 letters/digits/_/-".into()); }
    }
    let bark = ask_yes("Enable Bark notification alerts? (yes/no)")?;
    let bark_key = if bark { rpassword::prompt_password("Bark device key (hidden): ").map_err(|e| e.to_string())? } else { String::new() };
    if bark && (bark_key.is_empty() || bark_key.contains(['\n','\r','\"'])) { return Err("invalid Bark device key".into()); }
    let config = format!("# Owned by root. Do not commit this file.\n[general]\nserver_name = {}\ntimeout_seconds = {}\n\n[discord]\nenabled = true\nallowed_user_ids = [{}]\n\n[ntfy]\nenabled = {}\nserver = \"https://ntfy.sh\"\ntopic = {}\n\n[bark]\nenabled = {}\nserver = \"https://api.day.app\"\ndevice_key = {}\n", toml_string(&server), timeout, toml_string(&allowed), ntfy, toml_string(&ntfy_topic), bark, toml_string(&bark_key));
    let env_file = format!("DISCORD_TOKEN={}\nDISCORD_CHANNEL_ID={}\nDISCORD_ALLOWED_USER_IDS={}\n", token, channel, allowed);
    let path = Path::new("/etc/saspam/config.toml");
    if path.exists() { fs::copy(path, "/etc/saspam/config.toml.bak").map_err(|e| e.to_string())?; }
    write_private(path, &config)?;
    write_private(Path::new("/etc/saspam/discord.env"), &env_file)?;
    let _ = fs::remove_file(LAST_TEST);
    let result = Command::new("systemctl").args(["enable", "--now", "saspamd.service", "saspam-discord.service"]).status().map_err(|e| e.to_string())?;
    if !result.success() { return Err("configuration saved but service startup failed; run journalctl -u saspamd -u saspam-discord".into()); }
    let restart = Command::new("systemctl").args(["restart", "saspamd.service", "saspam-discord.service"]).status().map_err(|e| e.to_string())?;
    if !restart.success() { return Err("services failed to restart with new configuration".into()); }
    println!("Services started. Next: sudo saspam doctor && sudo saspam test (no PAM change yet)");
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
    println!("DisnanaSASPAM doctor");
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
    if !service_active("saspam-discord") { return Err("saspam-discord systemd service not active".into()); }
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
    let user = env::var("SUDO_USER").unwrap_or_else(|_| "root".into());
    println!("Testing real Discord approval without editing PAM");
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
            write_private(dest, HARDENING)?;
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
            fs::remove_file(path).map_err(|e|e.to_string())?;
            sshd_check(&["-t"])?;
            reload_ssh()?;
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
