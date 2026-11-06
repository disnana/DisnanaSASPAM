//! Minimal TestDisk-style terminal menus with no heavyweight TUI framework.
//! Raw mode is only used while selecting a menu item, never inside PAM.
use std::{io::{self, IsTerminal, Read, Write}, mem::MaybeUninit};

pub fn t<'a>(language: &str, ja: &'a str, en: &'a str) -> &'a str {
    if language == "en" { en } else { ja }
}

struct RawTty(libc::termios);
impl RawTty {
    fn enter() -> io::Result<Self> {
        let mut original = MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(0, original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        // Ctrl+C is handled as Back, ensuring the terminal is restored.
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 1;
        if unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        print!("\x1b[?25l");
        let _ = io::stdout().flush();
        Ok(Self(original))
    }
}
impl Drop for RawTty {
    fn drop(&mut self) {
        let _ = unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &self.0) };
        print!("\x1b[?25h\x1b[0m");
        let _ = io::stdout().flush();
    }
}

/// Use Up/Down, Enter/Space, Esc/Q. Return None if user backs out.
pub fn choose(title: &str, entries: &[String], selected: usize, language: &str) -> Result<Option<usize>, String> {
    if entries.is_empty() { return Ok(None); }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        println!("\n{title}");
        for (idx, entry) in entries.iter().enumerate() { println!(" {}. {}", idx+1, entry); }
        print!("{} ", t(language, "番号（0で戻る）:", "Number (0 to go back):"));
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
        let n: usize = answer.trim().parse().map_err(|_| "invalid menu selection")?;
        return Ok(if n == 0 { None } else if n <= entries.len() { Some(n-1) } else {
            return Err("menu selection out of range".into());
        });
    }
    let mut index = selected.min(entries.len()-1);
    let _raw = RawTty::enter().map_err(|e| e.to_string())?;
    let mut stdin = io::stdin();
    loop {
        print!("\x1b[2J\x1b[H\x1b[1;36m╔══════════════════════════════════════════════════╗\n");
        println!("  DisnanaSASPAM  •  {}\x1b[0m", title);
        print!("\x1b[36m╚══════════════════════════════════════════════════╝\x1b[0m\n\n");
        for (idx, entry) in entries.iter().enumerate() {
            if idx == index { print!("\x1b[30;46m ❯ {:<2}  {} \x1b[0m\n", idx+1, entry); }
            else { println!("   {:<2}  {}", idx+1, entry); }
        }
        println!("\n\x1b[2m{}\x1b[0m", t(language, "↑↓ 移動 / Enter・Space 選択 / Esc・Q 戻る / Ctrl+C 戻る", "↑↓ Move / Enter・Space Select / Esc・Q Back / Ctrl+C Back"));
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut buf = [0u8; 1];
        if stdin.read(&mut buf).map_err(|e| e.to_string())? == 0 { continue; }
        match buf[0] {
            b'\n' | b'\r' | b' ' => { print!("\x1b[2J\x1b[H"); return Ok(Some(index)); }
            3 | b'q' | b'Q' => { print!("\x1b[2J\x1b[H"); return Ok(None); }
            b'j' => index = (index + 1) % entries.len(),
            b'k' => index = (index + entries.len() - 1) % entries.len(),
            b'1'..=b'9' => {
                let v = (buf[0] - b'1') as usize;
                if v < entries.len() { index = v; }
            }
            27 => {
                let mut second = [0u8;1];
                if stdin.read(&mut second).map_err(|e| e.to_string())? == 0 {
                    print!("\x1b[2J\x1b[H"); return Ok(None);
                }
                if second[0] == b'[' {
                    let mut third = [0u8;1];
                    if stdin.read(&mut third).map_err(|e| e.to_string())? > 0 {
                        match third[0] {
                            b'A' => index = (index + entries.len() - 1) % entries.len(),
                            b'B' => index = (index + 1) % entries.len(),
                            _ => (),
                        }
                    }
                } else { print!("\x1b[2J\x1b[H"); return Ok(None); }
            }
            _ => (),
        }
    }
}

pub fn initial_language(existing: &str) -> Result<String, String> {
    let list = vec!["日本語 (Japanese)".to_string(), "English (英語)".to_string()];
    let choice = choose("Language / 言語を選択", &list, if existing == "en" { 1 } else { 0 }, existing)?
        .ok_or_else(|| "language selection cancelled".to_string())?;
    Ok(if choice == 0 { "ja" } else { "en" }.into())
}

pub fn main_menu() -> Result<(), String> {
    let mut selected = 0;
    loop {
        let language = crate::settings::current_language();
        let entries = if language == "en" {
            vec!["Partial settings / language", "Status", "Test Discord approval", "Test ntfy or Bark", "Diagnostics", "Enable SSH protection", "Disable SSH protection", "Check SSH hardening", "Exit"]
        } else {
            vec!["部分設定・言語切替", "状態確認", "Discord承認テスト", "ntfy / Bark通知テスト", "診断", "SSH保護を有効化", "SSH保護を解除", "SSH攻撃対策を診断", "終了"]
        }.into_iter().map(str::to_string).collect::<Vec<_>>();
        let Some(choice) = choose(t(&language,"メインメニュー","Main menu"),&entries,selected,&language)? else { return Ok(()); };
        selected = choice;
        match choice {
            0 => crate::settings::config_menu()?,
            1 => crate::status()?,
            2 => crate::test_login()?,
            3 => crate::settings::notify_menu()?,
            4 => crate::doctor()?,
            5 => return Err(t(&language,"SSH保護有効化は安全確認後に sudo saspam enable --ack-recovery --ack-tested で実行してください","To enable SSH protection, run sudo saspam enable --ack-recovery --ack-tested after checking your recovery options").into()),
            6 => crate::disable()?,
            7 => crate::security(&["audit".into()])?,
            _ => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_fallback_is_japanese() {
        assert_eq!(t("ja", "確認", "Check"), "確認");
        assert_eq!(t("en", "確認", "Check"), "Check");
        assert_eq!(t("invalid", "確認", "Check"), "確認");
    }
}
