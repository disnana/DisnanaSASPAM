//! Partial configuration commands and keyboard-driven configuration menus.
//! All mutations are root-only and fail closed, never editing the sshd PAM stack.
use std::{
    collections::BTreeMap, fs, io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream, path::Path, process::Command, time::Duration,
};
use saspam_protocol::{BotCommand, PamCommand, Reply, BOT_SOCKET, PAM_SOCKET, VERSION};
use super::{interactive::{choose, initial_language, t}, ask, root, service_active, snowflake, parse_role_ids, write_private, PAM_FILE, MARKER, LAST_TEST};

const CFG: &str = "/etc/saspam/config.toml";
const BOT_ENV: &str = "/etc/saspam/discord.env";

pub fn current_language() -> String {
    let text = fs::read_to_string(CFG).unwrap_or_default();
    let parsed: Result<toml::Value, _> = toml::from_str(&text);
    parsed.ok().and_then(|v| v.get("general")?.get("language")?.as_str().map(str::to_owned))
        .filter(|v| v == "ja" || v == "en").unwrap_or_else(|| "ja".into())
}

fn config_file() -> Result<toml::Value, String> {
    let raw = fs::read_to_string(CFG).map_err(|e| format!("cannot read configuration: {e}"))?;
    toml::from_str::<toml::Value>(&raw).map_err(|e| format!("invalid TOML: {e}"))
}
fn bot_env() -> Result<BTreeMap<String,String>,String> {
    let data = fs::read_to_string(BOT_ENV).map_err(|e| e.to_string())?;
    let mut map = BTreeMap::new();
    for line in data.lines() {
        if line.is_empty() || line.starts_with('#') { continue; }
        let (key, val) = line.split_once('=').ok_or("invalid discord.env line")?;
        if key.is_empty() || key.chars().any(|c| !c.is_ascii_uppercase() && c != '_') {
            return Err("invalid discord.env key".into());
        }
        map.insert(key.to_string(),val.to_string());
    }
    Ok(map)
}
fn serialize_env(values: &BTreeMap<String,String>) -> Result<String,String> {
    let mut out = String::new();
    for (key,val) in values {
        if val.chars().any(|c| c == '\r' || c == '\n' || c == '\0') { return Err("multiline value not allowed in Bot env".into()); }
        out.push_str(&format!("{key}={val}\n"));
    }
    Ok(out)
}
fn get_str<'a>(cfg: &'a toml::Value, section: &str, key: &str) -> &'a str {
    cfg.get(section).and_then(|s| s.get(key)).and_then(|v| v.as_str()).unwrap_or("")
}
fn get_bool(cfg:&toml::Value, section:&str, key:&str) -> bool {
    cfg.get(section).and_then(|s| s.get(key)).and_then(|v| v.as_bool()).unwrap_or(false)
}
fn set_value(cfg:&mut toml::Value, section:&str, field:&str, value:toml::Value) -> Result<(),String> {
    let table=cfg.get_mut(section).and_then(toml::Value::as_table_mut).ok_or_else(||format!("missing [{section}] section"))?;
    table.insert(field.to_string(),value);
    Ok(())
}
fn snowflake_list(ids:&str) -> Result<Vec<String>,String> { parse_role_ids(ids) }
fn valid_topic(topic: &str) -> bool {
    (24..=64).contains(&topic.len()) && topic.bytes().all(|c| c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
}
fn random_topic() -> Result<String,String> {
    let mut bytes = [0u8;24];
    fs::File::open("/dev/urandom").and_then(|mut file|file.read_exact(&mut bytes)).map_err(|e|e.to_string())?;
    Ok(bytes.iter().map(|b|format!("{b:02x}")).collect())
}
fn service_enabled(service: &str) -> bool {
    Command::new("systemctl").args(["is-enabled", "--quiet", service])
        .status().map(|s|s.success()).unwrap_or(false)
}
fn restart_if_needed(service:&str) -> Result<(),String> {
    if !service_active(service) && !service_enabled(service) { return Ok(()); }
    restart_force(service)
}
fn restart_force(service: &str) -> Result<(),String> {
    let status=Command::new("systemctl").args(["restart",service]).status().map_err(|e|e.to_string())?;
    if status.success() && service_active(service) { Ok(()) }
    else { Err(format!("{service} could not restart; config will be rolled back")) }
}
fn has_pending_requests() -> Result<bool,String> {
    let mut socket = UnixStream::connect(BOT_SOCKET).map_err(|e| format!("approval IPC unavailable: {e}"))?;
    socket.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e|e.to_string())?;
    let body=serde_json::to_string(&BotCommand::List{version:VERSION}).map_err(|e|e.to_string())?;
    socket.write_all(format!("{body}\n").as_bytes()).map_err(|e|e.to_string())?;
    let mut line=String::new();
    BufReader::new(socket).read_line(&mut line).map_err(|e|e.to_string())?;
    let response:Reply=serde_json::from_str(&line).map_err(|e|e.to_string())?;
    if !response.ok { return Err("unable to inspect pending approval requests".into()); }
    Ok(response.requests.unwrap_or_default().iter().any(|r| r.status==saspam_protocol::Status::Pending))
}
fn commit_configuration(cfg:&toml::Value, env:&BTreeMap<String,String>, ack:bool) -> Result<(),String> {
    root()?;
    let gated = fs::read_to_string(PAM_FILE).unwrap_or_default().lines().any(|l|l==MARKER);
    if gated {
        if !ack { return Err("SSH PAM protection is active. Supply --ack-interruption to confirm temporary fail-closed restart; keep a recovery console".into()); }
        if has_pending_requests()? { return Err("an SSH login is awaiting approval; retry after it completes".into()); }
    }
    let old_cfg=fs::read_to_string(CFG).map_err(|e|e.to_string())?;
    let old_env=fs::read_to_string(BOT_ENV).map_err(|e|e.to_string())?;
    let new_cfg=toml::to_string_pretty(cfg).map_err(|e|e.to_string())?;
    let new_env=serialize_env(env)?;
    if old_cfg==new_cfg && old_env==new_env {
        println!("No settings changed");
        return Ok(());
    }
    // The recovery backup is persistent, not an ephemeral temp file.
    fs::create_dir_all("/var/lib/saspam").map_err(|e|e.to_string())?;
    write_private(Path::new("/var/lib/saspam/config.previous.toml"),&old_cfg)?;
    write_private(Path::new("/var/lib/saspam/discord.previous.env"),&old_env)?;
    write_private(Path::new(CFG),&new_cfg)?;
    if let Err(e)=write_private(Path::new(BOT_ENV),&new_env) {
        let _=write_private(Path::new(CFG),&old_cfg);
        return Err(e);
    }
    let old_daemon_active=service_active("saspamd");
    let old_bot_active=service_active("saspam-discord");
    let apply_result=(||->Result<(),String>{
        let check = Command::new("/usr/local/bin/saspamd").args(["--check-config","--config",CFG])
            .status().map_err(|e|e.to_string())?;
        if !check.success() { return Err("daemon rejected new configuration".into()); }
        restart_if_needed("saspamd")?;
        restart_if_needed("saspam-discord")?;
        Ok(())
    })();
    if let Err(e)=apply_result {
        // Restore both secrets/config; restore old processes whenever possible.
        let _=write_private(Path::new(CFG),&old_cfg);
        let _=write_private(Path::new(BOT_ENV),&old_env);
        if old_daemon_active { let _=restart_force("saspamd"); }
        if old_bot_active { let _=restart_force("saspam-discord"); }
        return Err(format!("{e}; previous configuration restored (check journal)"));
    }
    let _=fs::remove_file(LAST_TEST);
    let language=get_str(cfg,"general","language");
    println!("{}",t(language, "✓ 設定を保存し、必要なサービスへ反映しました", "✓ Settings saved and services reloaded"));
    if gated {
        println!("{}",t(language,"注意: 再起動中のSSH承認はFail Closedで拒否される場合があります","Note: SSH approvals during restart may fail closed"));
    }
    Ok(())
}
fn config_change(name:&str, values:&[String], ack:bool) -> Result<(),String> {
    root()?;
    let mut cfg=config_file()?;
    let mut env=bot_env()?;
    match name {
        "language" => {
            let val=values.first().ok_or("usage: saspam config set language ja|en")?;
            if val!="ja" && val!="en" { return Err("language must be ja or en".into()); }
            set_value(&mut cfg,"general","language",toml::Value::String(val.clone()))?;
            env.insert("DISCORD_LANGUAGE".into(),val.clone());
        }
        "mentions" => {
            let val=values.first().ok_or("usage: saspam config set mentions none|roles|everyone")?;
            if !["none","roles","everyone"].contains(&val.as_str()) { return Err("invalid mention mode".into()); }
            if val=="everyone" && !ack { return Err("@everyone requires --ack-interruption to confirm the broad notification audience".into()); }
            env.insert("DISCORD_MENTION_MODE".into(),val.clone());
        }
        "ntfy" | "bark" => {
            let op=values.first().ok_or("usage: saspam config set ntfy|bark on|off|topic|key")?.as_str();
            match op {
                "on" => set_value(&mut cfg,name,"enabled",toml::Value::Boolean(true))?,
                "off" => set_value(&mut cfg,name,"enabled",toml::Value::Boolean(false))?,
                "topic" if name=="ntfy" => {
                    let topic=values.get(1).map(String::as_str).unwrap_or("");
                    let topic=if topic=="--generate" || topic.is_empty() { random_topic()? } else { topic.into() };
                    if !valid_topic(&topic) { return Err("ntfy topic must be 24-64 characters [a-zA-Z0-9_-]".into()); }
                    set_value(&mut cfg,"ntfy","topic",toml::Value::String(topic))?;
                }
                "key" if name=="bark" => {
                    // Never accept a secret as CLI argv (shell history/process list).
                    if values.len()>1 { return Err("Bark key must be entered interactively, not in CLI arguments".into()); }
                    let key=rpassword::prompt_password("Bark device key (hidden): ").map_err(|e|e.to_string())?;
                    if key.len()<8 || key.contains(['\r','\n','\"']) { return Err("invalid Bark device key".into()); }
                    set_value(&mut cfg,"bark","device_key",toml::Value::String(key))?;
                }
                _ => return Err("invalid provider setting".into()),
            }
        }
        "timeout" => {
            let val=values.first().ok_or("usage: saspam config set timeout SECONDS")?;
            let secs=val.parse::<i64>().map_err(|_|"timeout must be numeric")?;
            if !(10..=180).contains(&secs) { return Err("timeout must be 10..180 seconds".into()); }
            set_value(&mut cfg,"general","timeout_seconds",toml::Value::Integer(secs))?;
        }
        "server" => {
            let val=values.first().ok_or("usage: saspam config set server NAME")?;
            if val.is_empty() || val.len()>128 || val.chars().any(|c|c.is_control()) { return Err("invalid server name".into()); }
            set_value(&mut cfg,"general","server_name",toml::Value::String(val.clone()))?;
        }
        "roles" => {
            let val=values.first().ok_or("usage: saspam config set roles ROLE1,ROLE2,...")?;
            let ids=snowflake_list(val)?;
            let guild=get_str(&cfg,"discord","guild_id");
            if ids.iter().any(|r|r==guild) { return Err("@everyone cannot be an approval role".into()); }
            let array=ids.iter().cloned().map(toml::Value::String).collect();
            set_value(&mut cfg,"discord","allowed_role_ids",toml::Value::Array(array))?;
            env.insert("DISCORD_APPROVAL_ROLE_IDS".into(),ids.join(","));
        }
        _ => return Err("unknown setting. Try language, mentions, ntfy, bark, timeout, server, roles".into()),
    }
    commit_configuration(&cfg,&env,ack)
}
pub fn config_command(args:&[String]) -> Result<(),String> {
    if args.is_empty() { return config_menu(); }
    match args[0].as_str() {
        "show" => {
            root()?;
            let cfg=config_file()?;
            let env=bot_env()?;
            println!("Language: {}",get_str(&cfg,"general","language"));
            println!("Server: {}",get_str(&cfg,"general","server_name"));
            println!("Approval timeout: {} seconds",cfg.get("general").and_then(|v|v.get("timeout_seconds")).and_then(|v|v.as_integer()).unwrap_or(60));
            println!("Discord mention: {}",env.get("DISCORD_MENTION_MODE").map(String::as_str).unwrap_or("none"));
            println!("Allowed roles: {}",env.get("DISCORD_APPROVAL_ROLE_IDS").map(String::as_str).unwrap_or("(not configured)"));
            println!("ntfy enabled: {} (topic: {})",get_bool(&cfg,"ntfy","enabled"),get_str(&cfg,"ntfy","topic"));
            println!("Bark enabled: {} (key: {})",get_bool(&cfg,"bark","enabled"),if get_str(&cfg,"bark","device_key").is_empty() { "not configured" } else { "******" });
            Ok(())
        }
        "set" if args.len()>=3 => {
            let ack=args.iter().any(|a|a=="--ack-interruption");
            let parts=args[3..].iter().filter(|s|s.as_str()!="--ack-interruption").cloned().collect::<Vec<_>>();
            config_change(&args[1],&[vec![args[2].clone()],parts].concat(),ack)
        }
        _ => Err("usage: saspam config [show|set SETTING VALUE [--ack-interruption]]".into()),
    }
}
pub fn config_menu() -> Result<(),String> {
    root()?;
    let mut selected=0;
    loop {
        let cfg=config_file()?;
        let lang=current_language();
        let mention=bot_env()?.get("DISCORD_MENTION_MODE").cloned().unwrap_or_else(||"none".into());
        let ntfy=get_bool(&cfg,"ntfy","enabled");
        let bark=get_bool(&cfg,"bark","enabled");
        let timeout=cfg.get("general").and_then(|g|g.get("timeout_seconds")).and_then(|v|v.as_integer()).unwrap_or(60);
        let entries=vec![
            format!("{}: {}",t(&lang,"言語","Language"),if lang=="ja"{"日本語"}else{"English"}),
            format!("Discord {}: {}",t(&lang,"メンション","Mentions"),mention),
            format!("{}: {}",t(&lang,"ntfy通知","ntfy alerts"),if ntfy{"[x] ON"}else{"[ ] OFF"}),
            format!("{}: {}",t(&lang,"Bark通知","Bark alerts"),if bark{"[x] ON"}else{"[ ] OFF"}),
            t(&lang,"ntfyトピックの設定","Configure ntfy topic").to_owned(),
            t(&lang,"Barkデバイスキーの設定","Configure Bark device key").to_owned(),
            format!("{}: {}s",t(&lang,"承認期限","Approval timeout"),timeout),
            t(&lang,"承認ロールIDの変更","Change approval role IDs").to_owned(),
            t(&lang,"サーバー名の変更","Change server name").to_owned(),
            t(&lang,"設定の詳細表示","Show current settings").to_owned(),
            t(&lang,"戻る","Back").to_owned(),
        ];
        let Some(choice)=choose(t(&lang,"設定メニュー（項目ごとに保存）","Settings (save each change)"),&entries,selected,&lang)? else{return Ok(());};
        selected=choice;
        let mut args:Vec<String>=vec![];
        match choice {
            0 => { args.push("language".into()); args.push(initial_language(&lang)?); }
            1 => {
                let v=vec!["roles".to_string(),"everyone".into(),"none".into()];
                let labels=if lang=="en" {
                    vec!["Mention all approval roles".into(),"Mention @everyone (confirmation required)".into(),"No mentions".into()]
                } else {
                    vec!["承認ロール全員へ通知".into(),"@everyoneへ通知（追加確認）".into(),"メンションなし".into()]
                };
                let Some(i)=choose(t(&lang,"メンション対象","Mention audience"),&labels,0,&lang)? else{continue;};
                if i==1 && !super::ask_yes(t(&lang,"@everyoneへの通知を許可しますか? (yes/no)","Allow @everyone notifications? (yes/no)"))? {continue;}
                args.extend(["mentions".into(),v[i].clone()]);
            }
            2 => {
                args.extend(["ntfy".into(),if ntfy{"off"}else{"on"}.into()]);
                if !ntfy && get_str(&cfg,"ntfy","topic").is_empty() {
                    args = vec!["ntfy".into(), "topic".into(), "--generate".into()];
                    println!("{}",t(&lang,"先に安全なトピックを生成します。次に通知をオンにしてください","A random topic will be generated first. Enable notifications afterwards."));
                }
            }
            3 => {
                if !bark && get_str(&cfg,"bark","device_key").is_empty() {
                    args.extend(["bark".into(),"key".into()]);
                    println!("{}",t(&lang,"先にデバイスキーを設定します。次にBark通知をオンにしてください","Configure the Bark device key first, then enable notifications."));
                } else {
                    args.extend(["bark".into(),if bark{"off"}else{"on"}.into()]);
                }
            }
            4 => {
                let topic=ask(t(&lang,"ntfy topic（空欄で安全なランダム生成）","ntfy topic (empty generates random)"),"")?;
                args.extend(["ntfy".into(),"topic".into(),if topic.is_empty(){"--generate".into()}else{topic}]);
            }
            5 => args.extend(["bark".into(),"key".into()]),
            6 => {
                args.push("timeout".into());
                args.push(ask(t(&lang,"承認時間（秒）","Approval timeout (seconds)"),&timeout.to_string())?);
            }
            7 => {
                args.push("roles".into());
                args.push(ask(t(&lang,"承認ロールIDをカンマ区切りで入力","Approval role IDs, comma separated"),"")?);
            }
            8 => {
                args.push("server".into());
                args.push(ask(t(&lang,"サーバー名","Server name"),get_str(&cfg,"general","server_name"))?);
            }
            9 => { config_command(&["show".into()])?; continue; }
            _ => return Ok(()),
        }
        // Changing settings while SSH PAM is active can reject pending sessions
        // temporarily. Explicitly ask rather than quietly restarting the daemon.
        let gated=fs::read_to_string(PAM_FILE).unwrap_or_default().lines().any(|l|l==MARKER);
        let ack=!gated || super::ask_yes(t(&lang,"SSH保護中です。変更時に接続要求が拒否される場合があります。続けますか? (yes/no)","SSH protection is active. Pending requests may be denied during restart. Continue? (yes/no)"))?;
        if !ack { continue; }
        config_change(&args[0],&args[1..],gated || (args[0]=="mentions" && args[1]=="everyone"))?;
    }
}
pub fn notify_menu() -> Result<(),String> {
    let lang=current_language();
    let entries=vec![t(&lang,"ntfyをテスト","Test ntfy").into(),t(&lang,"Barkをテスト","Test Bark").into(),t(&lang,"有効な通知先をすべてテスト","Test all enabled providers").into(),t(&lang,"戻る","Back").into()];
    let Some(i)=choose(t(&lang,"通知の動作テスト","Push notification test"),&entries,0,&lang)? else{return Ok(())};
    if i<3 { notify_command(&["test".into(),["ntfy","bark","all"][i].into()]) } else { Ok(()) }
}
pub fn notify_command(args:&[String]) -> Result<(),String> {
    root()?;
    if args.first().map(String::as_str)!=Some("test") { return Err("usage: saspam notify test ntfy|bark|all".into()); }
    let provider=args.get(1).ok_or("select ntfy, bark or all")?;
    if !["ntfy","bark","all"].contains(&provider.as_str()) { return Err("unknown provider".into()); }
    let mut socket=UnixStream::connect(PAM_SOCKET).map_err(|e|e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_secs(20))).map_err(|e|e.to_string())?;
    socket.set_write_timeout(Some(Duration::from_secs(3))).map_err(|e|e.to_string())?;
    let cmd=PamCommand::NotificationTest{version:VERSION,provider:provider.clone()};
    let wire=serde_json::to_string(&cmd).map_err(|e|e.to_string())?;
    socket.write_all(format!("{wire}\n").as_bytes()).map_err(|e|e.to_string())?;
    let mut line=String::new();
    BufReader::new(socket).read_line(&mut line).map_err(|e|e.to_string())?;
    let r:Reply=serde_json::from_str(&line).map_err(|e|e.to_string())?;
    if r.ok {
        let lang=current_language();
        println!("{}",t(&lang,"✓ テスト通知を送信しました。受信端末で確認してください","✓ Test notification submitted. Check your device."));
        Ok(())
    } else { Err(r.error.unwrap_or_else(||"notification delivery failed".into())) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn topic_validation() {
        assert!(valid_topic("secure_ssh_abcdefghijklmnopqrst"));
        assert!(!valid_topic("ssh"));
        assert!(!valid_topic("hello world with spaces and bangs!!!!"));
    }
    #[test]
    fn sanitized_env_cannot_contain_newlines() {
        let mut fields=BTreeMap::new();
        fields.insert("DISCORD_LANGUAGE".into(),"ja".into());
        assert!(serialize_env(&fields).is_ok());
        fields.insert("DISCORD_TOKEN".into(),"secret\nATTACKER_CONTROLLED=x".into());
        assert!(serialize_env(&fields).is_err());
    }
}
