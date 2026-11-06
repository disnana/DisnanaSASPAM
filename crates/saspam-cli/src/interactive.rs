//! Minimal TestDisk-style terminal menus with no heavyweight TUI framework.
//! Raw mode is only used while selecting a menu item, never inside PAM.
use std::{io::{self, IsTerminal, Write}, mem::MaybeUninit};

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
        // Block on keyboard input rather than repainting ten times per second.
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
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

// Direct fd reads only. Mixing std::io::Stdin's internal buffer with poll(0)
// previously meant that ESC [ B was consumed by Stdin while poll(0) saw no
// data, so Down was incorrectly treated as the Esc/exit key.
fn read_tty_byte(timeout_ms: i32) -> io::Result<Option<u8>> {
    loop {
        let mut fd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
        let rc = unsafe { libc::poll(&mut fd, 1, timeout_ms) };
        if rc == 0 { return Ok(None); }
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted { continue; }
            return Err(error);
        }
        if fd.revents & libc::POLLIN == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "terminal input disconnected"));
        }
        let mut byte = 0u8;
        let n = unsafe { libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) };
        if n == 1 { return Ok(Some(byte)); }
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "terminal input ended"));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MenuKey { Up, Down, Confirm, Back, Backspace, Digit(u8), Ignore }

/// ANSI CSI and SS3 cursor-key sequences are supported, including CSI
/// modifiers (ESC [ 1 ; 5 A). Incomplete CSI must never act as a bare Esc.
fn read_menu_key() -> Result<MenuKey, String> {
    let Some(first) = read_tty_byte(-1).map_err(|e| e.to_string())? else {
        return Ok(MenuKey::Ignore);
    };
    Ok(match first {
        b'\n' | b'\r' | b' ' => MenuKey::Confirm,
        b'q' | b'Q' | 3 => MenuKey::Back,
        8 | 127 => MenuKey::Backspace,
        b'j' => MenuKey::Down,
        b'k' => MenuKey::Up,
        b'0'..=b'9' => MenuKey::Digit(first - b'0'),
        27 => {
            match read_tty_byte(350).map_err(|e| e.to_string())? {
                // A standalone Esc is Back. Never interpret a partially
                // received escape sequence as a request to close the menu.
                None => MenuKey::Back,
                Some(b'[') => {
                    let mut result = MenuKey::Ignore;
                    for _ in 0..24 {
                        match read_tty_byte(350).map_err(|e| e.to_string())? {
                            Some(b'A') => { result = MenuKey::Up; break; }
                            Some(b'B') => { result = MenuKey::Down; break; }
                            Some(b'0'..=b'9' | b';' | b':' | b'?') => (),
                            // Stop at the ANSI final byte: do not accidentally
                            // parse sequence fragments as menu number input.
                            Some(0x40..=0x7e) | None => break,
                            Some(_) => break,
                        }
                    }
                    result
                }
                Some(b'O') => match read_tty_byte(350).map_err(|e| e.to_string())? {
                    Some(b'A') => MenuKey::Up,
                    Some(b'B') => MenuKey::Down,
                    _ => MenuKey::Ignore,
                },
                // Alt+key and malformed sequences must not exit the menu.
                Some(_) => MenuKey::Ignore,
            }
        }
        _ => MenuKey::Ignore,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum MenuResult { Continue, Selected(usize), Back }

struct MenuState {
    index: usize,
    digits: String,
    invalid: bool,
}
impl MenuState {
    fn new(selected: usize, count: usize) -> Self {
        Self { index: selected.min(count.saturating_sub(1)), digits: String::new(), invalid: false }
    }

    fn number(&self, count: usize) -> Option<usize> {
        if self.digits == "0" { return Some(0); }
        if self.digits.starts_with('0') { return None; }
        let n = self.digits.parse::<usize>().ok()?;
        if (1..=count).contains(&n) { Some(n) } else { None }
    }

    fn highlighted(&self, count: usize) -> usize {
        self.number(count).filter(|n| *n > 0).map_or(self.index, |n| n - 1)
    }

    fn apply(&mut self, key: MenuKey, count: usize) -> MenuResult {
        match key {
            MenuKey::Back => return MenuResult::Back,
            MenuKey::Up => {
                self.index = (self.highlighted(count) + count - 1) % count;
                self.digits.clear();
                self.invalid = false;
            }
            MenuKey::Down => {
                self.index = (self.highlighted(count) + 1) % count;
                self.digits.clear();
                self.invalid = false;
            }
            MenuKey::Digit(n) => {
                // Retain the entire number until Enter/Space; do not turn
                // 111 into "11" followed by an accidental action on "1".
                if self.digits.len() < 6 {
                    self.digits.push(char::from(b'0' + n));
                    self.invalid = false;
                } else {
                    self.invalid = true;
                }
            }
            MenuKey::Backspace => {
                self.digits.pop();
                self.invalid = false;
            }
            MenuKey::Confirm => {
                if self.digits.is_empty() { return MenuResult::Selected(self.index); }
                match self.number(count) {
                    Some(0) => return MenuResult::Back,
                    Some(n) => return MenuResult::Selected(n - 1),
                    None => self.invalid = true,
                }
            }
            MenuKey::Ignore => (),
        }
        MenuResult::Continue
    }
}

/// Use Up/Down, number + Enter/Space, Esc/Q. Return None if user backs out.
pub fn choose(title: &str, entries: &[String], selected: usize, language: &str) -> Result<Option<usize>, String> {
    if entries.is_empty() { return Ok(None); }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        loop {
            println!("\n{title}");
            for (idx, entry) in entries.iter().enumerate() { println!(" {}. {}", idx + 1, entry); }
            print!("{} ", t(language, "番号（0で戻る）:", "Number (0 to go back):"));
            io::stdout().flush().map_err(|e| e.to_string())?;
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer).map_err(|e| e.to_string())? == 0 { return Ok(None); }
            let trimmed = answer.trim();
            if trimmed == "0" { return Ok(None); }
            if !trimmed.is_empty() && !trimmed.starts_with('0')
                && trimmed.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(n) = trimmed.parse::<usize>() {
                    if (1..=entries.len()).contains(&n) { return Ok(Some(n - 1)); }
                }
            }
            eprintln!("{}", t(language,
                "範囲外の番号です。メニューは終了しません",
                "Invalid number. The menu remains open"));
        }
    }
    let mut state = MenuState::new(selected, entries.len());
    let _raw = RawTty::enter().map_err(|e| e.to_string())?;
    loop {
        print!("\x1b[2J\x1b[H\x1b[1;36m╔══════════════════════════════════════════════════╗\n");
        println!("  DisnanaSASPAM  •  {}\x1b[0m", title);
        print!("\x1b[36m╚══════════════════════════════════════════════════╝\x1b[0m\n\n");
        for (idx, entry) in entries.iter().enumerate() {
            if idx == state.highlighted(entries.len()) {
                print!("\x1b[30;46m ❯ {:<2}  {} \x1b[0m\n", idx + 1, entry);
            } else {
                println!("   {:<2}  {}", idx + 1, entry);
            }
        }
        println!("\n\x1b[2m{}\x1b[0m", t(language,
            "↑↓ 移動 / 数字+Enter 選択 / Space 選択 / Backspace 修正 / 0+Enter・Esc・Q 戻る",
            "↑↓ Move / Number+Enter Select / Space Select / Backspace Edit / 0+Enter, Esc, Q Back"));
        if !state.digits.is_empty() {
            print!("{} \x1b[1m{}\x1b[0m", t(language, "番号入力:", "Number:"), state.digits);
            if state.number(entries.len()).is_none() {
                print!(" \x1b[33m{}\x1b[0m", t(language, "（範囲外）", "(out of range)"));
            }
            println!();
        }
        if state.invalid {
            println!("\x1b[31m{}\x1b[0m", t(language,
                "番号が範囲外です。Backspaceで修正するか↑↓で選択してください",
                "Number out of range. Use Backspace to edit or ↑↓ to navigate"));
        }
        io::stdout().flush().map_err(|e| e.to_string())?;
        match state.apply(read_menu_key()?, entries.len()) {
            MenuResult::Continue => (),
            MenuResult::Selected(index) => {
                print!("\x1b[2J\x1b[H");
                return Ok(Some(index));
            }
            MenuResult::Back => {
                print!("\x1b[2J\x1b[H");
                return Ok(None);
            }
        }
    }
}

pub fn initial_language(existing: &str) -> Result<String, String> {
    let list = vec!["日本語 (Japanese)".to_string(), "English (英語)".to_string()];
    let choice = choose("Language / 言語を選択", &list, if existing == "en" { 1 } else { 0 }, existing)?
        .ok_or_else(|| "language selection cancelled".to_string())?;
    Ok(if choice == 0 { "ja" } else { "en" }.into())
}

/// Keep command output visible until the operator acknowledges it.
pub fn pause_result(language: &str) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() { return Ok(()); }
    print!("\n\x1b[2m{}\x1b[0m",
        t(language, "Enterでメニューに戻る...", "Press Enter to return to the menu..."));
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line=String::new();
    io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn main_menu() -> Result<(), String> {
    let mut selected = 0;
    loop {
        let language = crate::settings::current_language();
        let entries = if language == "en" {
            vec!["Partial settings / language", "Status", "Test Discord approval", "Test ntfy or Bark", "Diagnostics", "Enable SSH protection", "Disable SSH protection", "Check SSH hardening", "Update / downgrade", "Exit"]
        } else {
            vec!["部分設定・言語切替", "状態確認", "Discord承認テスト", "ntfy / Bark通知テスト", "診断", "SSH保護を有効化", "SSH保護を解除", "SSH攻撃対策を診断", "更新・ダウングレード", "終了"]
        }.into_iter().map(str::to_string).collect::<Vec<_>>();
        let Some(choice)=choose(t(&language,"メインメニュー","Main menu"),&entries,selected,&language)? else { return Ok(()); };
        selected=choice;
        if choice==9 { return Ok(()); }
        // Never instantly redraw over diagnostics, status or an error message.
        let result:Result<(),String>=match choice {
            0 => crate::settings::config_menu(),
            1 => crate::status(),
            2 => crate::test_login(),
            3 => crate::settings::notify_menu(),
            4 => crate::doctor(),
            5 => {
                if crate::ask_yes(t(&language,
                    "既存SSH接続・コンソール復旧・承認テスト成功を確認しましたか？ (yes/no)",
                    "Confirm open SSH session, console recovery and successful approval test? (yes/no)"))? {
                    crate::enable(&["enable".into(), "--ack-recovery".into(), "--ack-tested".into()])
                } else { Ok(()) }
            }
            6 => {
                if crate::ask_yes(t(&language,
                    "SSHの追加承認を解除しますか？ (yes/no)",
                    "Disable SASPAM SSH approval? (yes/no)"))? { crate::disable() } else { Ok(()) }
            }
            7 => crate::security(&["audit".into()]),
            8 => crate::update_version(&[]),
            _ => Ok(()),
        };
        if let Err(err)=result {
            eprintln!("\n\x1b[31m[ERROR]\x1b[0m {err}");
        }
        if choice!=0 { pause_result(&language)?; }
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
    #[test]
    fn numbers_are_committed_only_on_confirm_and_can_be_two_digits() {
        let mut s = MenuState::new(0, 11);
        assert_eq!(s.apply(MenuKey::Digit(1), 11), MenuResult::Continue);
        assert_eq!(s.apply(MenuKey::Digit(1), 11), MenuResult::Continue);
        assert_eq!(s.highlighted(11), 10);
        assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Selected(10));
    }

    #[test]
    fn twelve_and_one_hundred_eleven_do_not_execute_or_exit_an_eleven_item_menu() {
        for digits in ["12", "111"] {
            let mut s = MenuState::new(0, 11);
            for b in digits.bytes() {
                assert_eq!(s.apply(MenuKey::Digit(b - b'0'), 11), MenuResult::Continue);
            }
            assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Continue);
            assert!(s.invalid);
            assert_eq!(s.index, 0);
            assert_eq!(s.digits, digits);
        }
    }

    #[test]
    fn backspace_corrects_invalid_numbers_without_activating_an_earlier_digit() {
        let mut s = MenuState::new(0, 11);
        for n in [1, 1, 1] { s.apply(MenuKey::Digit(n), 11); }
        assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Continue);
        assert_eq!(s.apply(MenuKey::Backspace, 11), MenuResult::Continue);
        assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Selected(10));
    }

    #[test]
    fn back_requires_only_zero_and_zero_prefixes_are_not_accepted() {
        let mut s = MenuState::new(0, 11);
        s.apply(MenuKey::Digit(0), 11);
        assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Back);
        s.apply(MenuKey::Digit(0), 11);
        assert_eq!(s.apply(MenuKey::Confirm, 11), MenuResult::Continue);
        assert!(s.invalid);
    }

    #[test]
    fn arrow_keys_discard_number_entry_and_wrap() {
        let mut s = MenuState::new(0, 11);
        s.apply(MenuKey::Digit(1), 11);
        s.apply(MenuKey::Digit(1), 11);
        s.apply(MenuKey::Down, 11);
        assert_eq!(s.index, 0);
        assert!(s.digits.is_empty());
        s.apply(MenuKey::Up, 11);
        assert_eq!(s.index, 10);
    }

}
