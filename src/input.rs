use ratatui::crossterm::event::Event;
use std::io;
use tokio::sync::mpsc::{self, UnboundedReceiver};

/// Start the input reader after raw mode has been enabled.
pub fn events() -> io::Result<UnboundedReceiver<Event>> {
    platform::events()
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use ratatui::crossterm::event;

    pub fn events() -> io::Result<UnboundedReceiver<Event>> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new().name("terminal-input".into()).spawn(move || {
            while let Ok(event) = event::read() {
                if tx.send(event).is_err() {
                    break;
                }
            }
        })?;
        Ok(rx)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        sync::mpsc as std_mpsc,
        time::{Duration, Instant},
    };
    enum Chunk {
        Unit(u16),
        Many(Vec<u16>),
    }
    use ratatui::crossterm::terminal;
    use windows_sys::Win32::System::Console::{ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetStdHandle, ReadConsoleW, STD_INPUT_HANDLE, SetConsoleMode};

    pub fn events() -> io::Result<UnboundedReceiver<Event>> {
        // The console handle is borrowed; raw mode restoration belongs to the TUI.
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let mut mode = 0;
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 || unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (chunks_tx, chunks_rx) = std_mpsc::channel();
        // HANDLE is borrowed and is not Send; reacquire the process standard handle in the reader.
        std::thread::Builder::new().name("console-reader".into()).spawn(move || {
            let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            let mut buf = [0u16; 1024];
            loop {
                let mut count = 0;
                let ok = unsafe { ReadConsoleW(handle, buf.as_mut_ptr().cast(), buf.len() as u32, &mut count, std::ptr::null()) };
                if ok == 0 || count == 0 {
                    break;
                }
                let chunk = if count == 1 { Chunk::Unit(buf[0]) } else { Chunk::Many(buf[..count as usize].to_vec()) };
                if chunks_tx.send(chunk).is_err() {
                    break;
                }
            }
        })?;
        std::thread::Builder::new().name("terminal-input".into()).spawn(move || {
            let mut parser = super::Parser::default();
            let mut size = terminal::size().ok();
            let interval = Duration::from_millis(75);
            let mut next_resize = Instant::now() + interval;
            let mut escape_since = None;
            loop {
                let now = Instant::now();
                let deadline = escape_since.map_or(next_resize, |t: Instant| next_resize.min(t + Duration::from_millis(35)));
                match chunks_rx.recv_timeout(deadline.saturating_duration_since(now)) {
                    Ok(chunk) => {
                        match chunk {
                            Chunk::Unit(unit) => parser.feed(std::slice::from_ref(&unit), &mut |event| {
                                let _ = tx.send(event);
                            }),
                            Chunk::Many(units) => parser.feed(&units, &mut |event| {
                                let _ = tx.send(event);
                            }),
                        }
                        if parser.pending_escape() {
                            escape_since.get_or_insert_with(Instant::now);
                        } else {
                            escape_since = None;
                        }
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
                let now = Instant::now();
                if escape_since.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(35)) {
                    parser.flush_escape(&mut |event| {
                        let _ = tx.send(event);
                    });
                    escape_since = None;
                }
                if now >= next_resize {
                    if let Ok(current) = terminal::size()
                        && size != Some(current)
                    {
                        size = Some(current);
                        if tx.send(Event::Resize(current.0, current.1)).is_err() {
                            break;
                        }
                    }
                    next_resize = now + interval;
                }
                if tx.is_closed() {
                    break;
                }
            }
        })?;
        Ok(rx)
    }
}

#[cfg(any(windows, test))]
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[cfg(any(windows, test))]
#[derive(Default)]
struct Parser {
    high_surrogate: Option<u16>,
    escape: String,
    paste: Option<String>,
    paste_end: usize,
}

#[cfg(any(windows, test))]
impl Parser {
    fn pending_escape(&self) -> bool {
        self.paste.is_none() && self.escape == "\x1b"
    }

    fn flush_escape(&mut self, emit: &mut impl FnMut(Event)) {
        if self.pending_escape() {
            self.escape.clear();
            emit(key(KeyCode::Esc, KeyModifiers::NONE));
        }
    }

    fn feed(&mut self, units: &[u16], emit: &mut impl FnMut(Event)) {
        // A whole, already-read unframed chunk is safe to recognize only if it
        // contains ordinary text on both sides of a newline and no other controls.
        if units.len() > 2
            && units.iter().any(|&u| u == 10 || u == 13)
            && !units.contains(&27)
            && self.paste.is_none()
            && self.escape.is_empty()
            && self.high_surrogate.is_none()
            && let Ok(text) = String::from_utf16(units)
        {
            let clean = text.chars().all(|c| !c.is_control() || c == '\r' || c == '\n' || c == '\t');
            let first = text.char_indices().find(|(_, c)| !c.is_control()).map(|(i, _)| i);
            let last = text.char_indices().rfind(|(_, c)| !c.is_control()).map(|(i, _)| i);
            let interior = first.zip(last).is_some_and(|(first, last)| text[first..last].contains(['\r', '\n']));
            if clean && interior {
                emit(Event::Paste(text));
                return;
            }
        }
        for &unit in units {
            if let Some(high) = self.high_surrogate.take() {
                if (0xdc00..=0xdfff).contains(&unit) {
                    let value = 0x10000 + (((high as u32 - 0xd800) << 10) | (unit as u32 - 0xdc00));
                    self.character(char::from_u32(value).unwrap(), emit);
                    continue;
                }
                self.character('\u{fffd}', emit);
            }
            if (0xd800..=0xdbff).contains(&unit) {
                self.high_surrogate = Some(unit);
            } else {
                self.character(char::from_u32(unit as u32).unwrap_or('\u{fffd}'), emit);
            }
        }
    }

    fn character(&mut self, c: char, emit: &mut impl FnMut(Event)) {
        const END: &[u8] = b"\x1b[201~";
        if let Some(paste) = &mut self.paste {
            if c as u32 == END[self.paste_end] as u32 {
                self.paste_end += 1;
                if self.paste_end == END.len() {
                    self.paste_end = 0;
                    emit(Event::Paste(self.paste.take().unwrap()));
                }
            } else {
                if self.paste_end != 0 {
                    paste.push_str(std::str::from_utf8(&END[..self.paste_end]).unwrap());
                    self.paste_end = 0;
                }
                if c == '\x1b' {
                    self.paste_end = 1;
                } else {
                    paste.push(c);
                }
            }
            return;
        }
        if !self.escape.is_empty() {
            self.escape.push(c);
            if self.escape == "\x1b[200~" {
                self.escape.clear();
                self.paste = Some(String::new());
            } else if self.escape.len() == 2 && c != '[' && c != 'O' {
                self.escape.clear();
                if let Some((code, modifiers)) = plain_key(c) {
                    emit(key(code, modifiers | KeyModifiers::ALT));
                }
            } else if self.escape.len() > 2 && (('@'..='~').contains(&c)) {
                if let Some(event) = sequence(&self.escape) {
                    emit(event);
                }
                self.escape.clear();
            } else if self.escape.len() > 128 {
                self.escape.clear();
            }
            return;
        }
        if c == '\x1b' {
            self.escape.push(c);
        } else if let Some((code, modifiers)) = plain_key(c) {
            emit(key(code, modifiers));
        }
    }
}

#[cfg(any(windows, test))]
fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

#[cfg(any(windows, test))]
fn plain_key(c: char) -> Option<(KeyCode, KeyModifiers)> {
    let none = KeyModifiers::NONE;
    Some(match c {
        '\r' => (KeyCode::Enter, none),
        '\n' => (KeyCode::Char('j'), KeyModifiers::CONTROL),
        '\t' => (KeyCode::Tab, none),
        '\x08' | '\x7f' => (KeyCode::Backspace, none),
        '\x01'..='\x1a' => (KeyCode::Char(((c as u8 - 1) + b'a') as char), KeyModifiers::CONTROL),
        c if !c.is_control() => (KeyCode::Char(c), none),
        _ => return None,
    })
}

#[cfg(any(windows, test))]
fn modifiers(value: u32) -> KeyModifiers {
    let bits = value.saturating_sub(1);
    let mut result = KeyModifiers::NONE;
    if bits & 1 != 0 {
        result |= KeyModifiers::SHIFT;
    }
    if bits & 2 != 0 {
        result |= KeyModifiers::ALT;
    }
    if bits & 4 != 0 {
        result |= KeyModifiers::CONTROL;
    }
    if bits & 8 != 0 {
        result |= KeyModifiers::SUPER;
    }
    result
}

#[cfg(any(windows, test))]
fn sequence(seq: &str) -> Option<Event> {
    let final_char = seq.chars().last()?;
    let body = &seq[2..seq.len() - 1];
    if final_char == 'I' && body.is_empty() {
        return Some(Event::FocusGained);
    }
    if final_char == 'O' && body.is_empty() {
        return Some(Event::FocusLost);
    }
    let parts: Vec<&str> = body.split(';').collect();
    let number = |i: usize, default: u32| parts.get(i).and_then(|s| s.split(':').next()).and_then(|s| s.parse().ok()).unwrap_or(default);
    if final_char == '_' {
        // Windows Terminal's Win32-input-mode encoding: Vk;Sc;Uc;Kd;Cs;Rc_.
        if number(3, 0) == 0 {
            return None;
        }
        let state = number(4, 0);
        let mut mods = KeyModifiers::NONE;
        if state & 0x10 != 0 {
            mods |= KeyModifiers::SHIFT;
        }
        if state & 0x03 != 0 {
            mods |= KeyModifiers::ALT;
        }
        if state & 0x0c != 0 {
            mods |= KeyModifiers::CONTROL;
        }
        let code = match number(0, 0) {
            0x0d => KeyCode::Enter,
            0x08 => KeyCode::Backspace,
            0x09 if mods.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            0x09 => KeyCode::Tab,
            0x1b => KeyCode::Esc,
            0x21 => KeyCode::PageUp,
            0x22 => KeyCode::PageDown,
            0x23 => KeyCode::End,
            0x24 => KeyCode::Home,
            0x25 => KeyCode::Left,
            0x26 => KeyCode::Up,
            0x27 => KeyCode::Right,
            0x28 => KeyCode::Down,
            0x2e => KeyCode::Delete,
            _ => {
                let c = char::from_u32(number(2, 0))?;
                let (code, m) = plain_key(c)?;
                mods |= m;
                code
            }
        };
        return Some(key(code, mods));
    }
    let mods = modifiers(number(1, 1));
    let code = match final_char {
        'A' => KeyCode::Up,
        'B' => KeyCode::Down,
        'C' => KeyCode::Right,
        'D' => KeyCode::Left,
        'H' => KeyCode::Home,
        'F' => KeyCode::End,
        'Z' => KeyCode::BackTab,
        '~' => match number(0, 0) {
            1 | 7 => KeyCode::Home,
            3 => KeyCode::Delete,
            4 | 8 => KeyCode::End,
            5 => KeyCode::PageUp,
            6 => KeyCode::PageDown,
            27 if number(2, 0) == 13 => return Some(key(KeyCode::Enter, mods)),
            _ => return None,
        },
        'u' => {
            // Kitty CSI-u event type 3 is release; never dispatch it as a key.
            if parts.get(1).and_then(|s| s.split(':').nth(1)).and_then(|s| s.parse::<u32>().ok()) == Some(3) {
                return None;
            }
            let c = char::from_u32(number(0, 0))?;
            if c == '\r' || c == '\n' {
                KeyCode::Enter
            } else {
                let (code, extra) = plain_key(c)?;
                return Some(key(code, mods | extra));
            }
        }
        _ => return None,
    };
    Some(key(code, mods))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_input_flow() {
        let mut parser = Parser::default();
        let mut events = Vec::new();
        let chunks: Vec<Vec<u16>> =
            vec!["\x1b[20".encode_utf16().collect(), "0~中文\r\n\t".encode_utf16().collect(), vec![0xd840], vec![0xdc00, 10, 3, 27, 91, 50], "01~".encode_utf16().collect()];
        for chunk in &chunks {
            parser.feed(chunk, &mut |e| events.push(e));
        }
        assert_eq!(events, vec![Event::Paste("中文\r\n\t𠀀\n\u{3}".into())]);
        for chunk in ["\r\x1b[D\x1b[3~\x1b[Z", "\x1b[200~one\x1b[201~\x1b[200~two\x1b[201~", "\x1b[13;3u\x1b[13;1:3u"] {
            parser.feed(&chunk.encode_utf16().collect::<Vec<_>>(), &mut |e| events.push(e));
        }
        parser.feed(&[27], &mut |e| events.push(e));
        assert!(parser.pending_escape());
        parser.flush_escape(&mut |e| events.push(e));
        assert_eq!(
            &events[1..],
            &[
                key(KeyCode::Enter, KeyModifiers::NONE),
                key(KeyCode::Left, KeyModifiers::NONE),
                key(KeyCode::Delete, KeyModifiers::NONE),
                key(KeyCode::BackTab, KeyModifiers::NONE),
                Event::Paste("one".into()),
                Event::Paste("two".into()),
                key(KeyCode::Enter, KeyModifiers::ALT),
                key(KeyCode::Esc, KeyModifiers::NONE),
            ]
        );
    }
}
