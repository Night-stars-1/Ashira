//! Terminal image protocols (kitty, iTerm2, sixel) for pictures printed into scrollback.
//! Chosen once at startup by asking the terminal; without a reply the TUI stays text-only.
use std::{
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{DynamicImage, RgbaImage, imageops::FilterType};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Protocol {
    Kitty,
    Iterm,
    Sixel,
}

pub struct Graphics {
    protocol: Protocol,
    /// One cell in pixels (width, height).
    cell: (u32, u32),
}

/// An encoded picture: `rows` terminal rows tall, drawn by writing `data` at its top-left cell.
pub struct Picture {
    pub data: String,
    pub rows: u16,
}

/// Used when the terminal does not report its cell size. Smaller than most real cells,
/// so a picture sized with it never runs past the rows reserved for it.
const FALLBACK_CELL: (u32, u32) = (8, 16);
const REPLY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Ask the terminal which image protocol it speaks. Must run in raw mode, before the input reader starts.
pub fn detect() -> Option<Graphics> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }
    // Multiplexers pass graphics through only when configured to; stay with text there.
    if std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() {
        return None;
    }
    // Kitty graphics query, cell size in pixels, then primary device attributes. Every
    // terminal answers the last one, and answers in order, so its reply ends the wait.
    let mut out = io::stdout();
    out.write_all(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[16t\x1b[c").ok()?;
    out.flush().ok()?;
    let reply = read_reply(REPLY_TIMEOUT);
    let iterm = matches!(std::env::var("TERM_PROGRAM").as_deref(), Ok("iTerm.app" | "WezTerm"))
        || std::env::var("LC_TERMINAL").is_ok_and(|v| v == "iTerm2");
    choose(&reply, iterm)
}

fn choose(reply: &str, iterm_env: bool) -> Option<Graphics> {
    let cell = cell_size(reply);
    // WezTerm also answers the kitty query, but renders iTerm2 images more reliably.
    let protocol = if iterm_env {
        Protocol::Iterm
    } else if reply.contains("\x1b_Gi=31;OK") {
        Protocol::Kitty
    } else if device_attributes(reply).is_some_and(|params| params.split(';').any(|p| p == "4")) {
        Protocol::Sixel
    } else {
        return None;
    };
    Some(Graphics { protocol, cell: cell.unwrap_or(FALLBACK_CELL) })
}

/// `CSI 6 ; height ; width t`
fn cell_size(reply: &str) -> Option<(u32, u32)> {
    let rest = &reply[reply.find("\x1b[6;")? + 4..];
    let (h, rest) = rest.split_once(';')?;
    let (w, _) = rest.split_once('t')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Parameters of `CSI ? params c`.
fn device_attributes(reply: &str) -> Option<&str> {
    let rest = &reply[reply.find("\x1b[?")? + 3..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != ';')?;
    (rest.as_bytes()[end] == b'c').then(|| &rest[..end])
}

#[cfg(unix)]
fn read_reply(timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut fd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut fd, 1, left.as_millis().max(1) as i32) } <= 0 {
            break;
        }
        let mut chunk = [0u8; 256];
        let n = unsafe { libc::read(libc::STDIN_FILENO, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
        if device_attributes(&String::from_utf8_lossy(&buf)).is_some() {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(windows)]
fn read_reply(timeout: Duration) -> String {
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::{
            Console::{ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetStdHandle, INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, STD_INPUT_HANDLE, SetConsoleMode},
            Threading::WaitForSingleObject,
        },
    };
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut mode = 0;
    // Replies arrive as key input only in VT input mode; the input reader sets the same mode.
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 || unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) } == 0 {
        return String::new();
    }
    let deadline = Instant::now() + timeout;
    let mut units: Vec<u16> = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || unsafe { WaitForSingleObject(handle, left.as_millis().max(1) as u32) } != WAIT_OBJECT_0 {
            break;
        }
        // Records, not ReadConsoleW: a focus or resize record alone would block a character read.
        let mut records = [INPUT_RECORD::default(); 64];
        let mut count = 0;
        if unsafe { ReadConsoleInputW(handle, records.as_mut_ptr(), records.len() as u32, &mut count) } == 0 {
            break;
        }
        for record in &records[..count as usize] {
            if u32::from(record.EventType) != KEY_EVENT {
                continue;
            }
            let key = unsafe { record.Event.KeyEvent };
            let unit = unsafe { key.uChar.UnicodeChar };
            if key.bKeyDown != 0 && unit != 0 {
                units.push(unit);
            }
        }
        if device_attributes(&String::from_utf16_lossy(&units)).is_some() {
            break;
        }
    }
    String::from_utf16_lossy(&units)
}

impl Graphics {
    /// Encode a `data:image/...;base64,` URL to fit `max_cols` × `max_rows` cells, never enlarged.
    pub fn picture(&self, data_url: &str, max_cols: u16, max_rows: u16) -> Option<Picture> {
        if max_cols == 0 || max_rows == 0 {
            return None;
        }
        let bytes = STANDARD.decode(data_url.split_once(";base64,")?.1).ok()?;
        let image = image::load_from_memory(&bytes).ok()?;
        let (cw, ch) = self.cell;
        let (w, h) = (image.width().max(1), image.height().max(1));
        let scale = (f64::from(max_cols) * f64::from(cw) / f64::from(w))
            .min(f64::from(max_rows) * f64::from(ch) / f64::from(h))
            .min(1.0);
        let (pw, ph) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
        let image = if (pw, ph) == (w, h) { image } else { image.resize_exact(pw, ph, FilterType::Triangle) };
        let mut rows = ph.div_ceil(ch);
        let data = match self.protocol {
            Protocol::Sixel => {
                // Sixel bands are 6 pixels tall; keep the last band inside the reserved rows.
                if ph.div_ceil(6) * 6 > rows * ch {
                    rows += 1;
                }
                sixel(&image.to_rgba8())
            }
            Protocol::Kitty => kitty(&png(&image)?, pw.div_ceil(cw), rows),
            Protocol::Iterm => iterm(&png(&image)?, pw.div_ceil(cw), rows),
        };
        Some(Picture { data, rows: u16::try_from(rows).ok()? })
    }
}

fn png(image: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Transmit and place at the cursor in one go (`a=T`) over `cols` × `rows` cells,
/// cursor left in place (`C=1`).
fn kitty(png: &[u8], cols: u32, rows: u32) -> String {
    let encoded = STANDARD.encode(png);
    let chunks = encoded.as_bytes().chunks(4096);
    let count = chunks.len();
    let mut out = String::with_capacity(encoded.len() + count * 16 + 48);
    for (index, chunk) in chunks.enumerate() {
        out.push_str("\x1b_G");
        if index == 0 {
            let _ = write!(out, "a=T,f=100,q=2,C=1,c={cols},r={rows},");
        }
        let _ = write!(out, "m={};", u8::from(index + 1 < count));
        // Base64 is ASCII, so any byte boundary is a char boundary.
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\x1b\\");
    }
    out
}

/// Sized in cells with the aspect kept, so the picture never exceeds the reserved box.
fn iterm(png: &[u8], cols: u32, rows: u32) -> String {
    format!(
        "\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=1;doNotMoveCursor=1:{}\x07",
        png.len(),
        STANDARD.encode(png)
    )
}

/// 256-colour sixel; fully transparent pixels stay unset.
fn sixel(image: &RgbaImage) -> String {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let pixels = image.as_raw();
    let quant = color_quant::NeuQuant::new(10, 256, pixels);
    let index: Vec<Option<u8>> = pixels.chunks_exact(4).map(|p| (p[3] >= 128).then(|| quant.index_of(p) as u8)).collect();
    let mut out = String::with_capacity(w * h / 2);
    // P2=1: unset pixels keep the background. Raster: 1:1 aspect, w × h pixels.
    let _ = write!(out, "\x1bP0;1;0q\"1;1;{w};{h}");
    for (i, rgb) in quant.color_map_rgb().chunks_exact(3).enumerate() {
        let pct = |v: u8| (u32::from(v) * 100 + 127) / 255;
        let _ = write!(out, "#{i};2;{};{};{}", pct(rgb[0]), pct(rgb[1]), pct(rgb[2]));
    }
    // Per band: each used colour's six-pixel column bits, then one run-length row per colour.
    let mut bits = vec![0u8; 256 * w];
    let mut used = [false; 256];
    for top in (0..h).step_by(6) {
        used.fill(false);
        for dy in 0..6.min(h - top) {
            for x in 0..w {
                if let Some(c) = index[(top + dy) * w + x] {
                    let c = usize::from(c);
                    if !used[c] {
                        used[c] = true;
                        bits[c * w..(c + 1) * w].fill(0);
                    }
                    bits[c * w + x] |= 1 << dy;
                }
            }
        }
        let mut first = true;
        for c in (0..256).filter(|&c| used[c]) {
            if !first {
                out.push('$');
            }
            first = false;
            let _ = write!(out, "#{c}");
            let row = &bits[c * w..(c + 1) * w];
            // Trailing empty columns need no characters.
            let end = row.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
            let mut x = 0;
            while x < end {
                let run = row[x..end].iter().take_while(|&&b| b == row[x]).count();
                let ch = char::from(63 + row[x]);
                if run > 3 {
                    let _ = write!(out, "!{run}{ch}");
                } else {
                    (0..run).for_each(|_| out.push(ch));
                }
                x += run;
            }
        }
        if top + 6 < h {
            out.push('-');
        }
    }
    out.push_str("\x1b\\");
    out
}
