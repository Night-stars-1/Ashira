//! `ashira --resume` with no id: a full-screen list of this folder's sessions to pick from.
//! Type to filter, ↑↓ to move, Enter to resume, Tab for every project, Esc to cancel.

use std::{
    io::{self, IsTerminal, Stdout},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    render::{DIM, sanitize, width},
    session::{self, Resumable},
};

const ACCENT: Style = Style::new().fg(Color::Yellow);
/// Rows each session takes: title, first message (when it adds anything), details, gap.
const ROWS: u16 = 4;

/// The session the user picked, or None when they cancelled or there is nothing to pick.
pub fn pick(cwd: &Path) -> Result<Option<PathBuf>, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("--resume 不带会话 ID 时需要在终端里选择；也可以用 --resume <ID> 或 --list".into());
    }
    let mut picker = Picker { cwd: cwd.to_path_buf(), all: false, query: String::new(), selected: 0, top: 0, sessions: session::resumable(cwd, false) };
    // Nothing here yet: open on every project so the picker is never just empty.
    if picker.sessions.is_empty() {
        picker.all = true;
        picker.sessions = session::resumable(cwd, true);
        if picker.sessions.is_empty() { return Err("还没有任何会话".into()); }
    }
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let mut out = io::stdout();
    if let Err(e) = execute!(out, EnterAlternateScreen) {
        let _ = terminal::disable_raw_mode();
        return Err(e.to_string());
    }
    let result = Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| e.to_string()).and_then(|mut term| picker.run(&mut term));
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    result
}

struct Picker {
    cwd: PathBuf,
    all: bool,
    query: String,
    selected: usize,
    /// First listed session on screen.
    top: usize,
    sessions: Vec<Resumable>,
}

impl Picker {
    fn run(&mut self, term: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<Option<PathBuf>, String> {
        loop {
            term.draw(|frame| self.draw(frame.area(), frame.buffer_mut())).map_err(|e| e.to_string())?;
            let Event::Key(key) = event::read().map_err(|e| e.to_string())? else { continue };
            if key.kind == KeyEventKind::Release { continue; }
            let shown = self.matches().len();
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
                KeyCode::Enter => return Ok(self.matches().get(self.selected).map(|item| item.path.clone())),
                KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                KeyCode::Down => self.selected = (self.selected + 1).min(shown.saturating_sub(1)),
                KeyCode::PageUp => self.selected = self.selected.saturating_sub(5),
                KeyCode::PageDown => self.selected = (self.selected + 5).min(shown.saturating_sub(1)),
                KeyCode::Home => self.selected = 0,
                KeyCode::End => self.selected = shown.saturating_sub(1),
                KeyCode::Tab | KeyCode::BackTab => {
                    self.all = !self.all;
                    self.sessions = session::resumable(&self.cwd, self.all);
                    self.selected = 0;
                    self.top = 0;
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.selected = 0;
                }
                KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    self.query.push(c);
                    self.selected = 0;
                }
                _ => {}
            }
        }
    }

    /// Sessions whose title, first message or folder holds every word typed (any case).
    fn matches(&self) -> Vec<&Resumable> {
        let words: Vec<String> = self.query.split_whitespace().map(str::to_lowercase).collect();
        self.sessions.iter().filter(|item| {
            let text = format!("{} {} {}", item.title, item.prompt, item.cwd).to_lowercase();
            words.iter().all(|word| text.contains(word))
        }).collect()
    }

    fn draw(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let title = if self.all { " 恢复会话（所有项目） " } else { " 恢复会话（当前目录） " };
        let block = Block::default().borders(Borders::ALL).border_style(DIM).title(Span::styled(title, ACCENT.add_modifier(Modifier::BOLD)));
        let inner = block.inner(area);
        ratatui::widgets::Widget::render(block, area, buf);
        if inner.height < 4 || inner.width < 10 { return; }
        let w = inner.width as usize;

        let query = Line::from(vec![Span::styled("> ", ACCENT), Span::raw(sanitize(&self.query)), Span::styled("█", DIM)]);
        ratatui::widgets::Widget::render(Paragraph::new(query), Rect { height: 1, ..inner }, buf);

        let hint = if self.all { "Tab 当前目录" } else { "Tab 所有项目" };
        let footer = Line::styled(format!("↑↓ 选择 · Enter 恢复 · {hint} · Esc 取消 · 输入文字筛选"), DIM);
        ratatui::widgets::Widget::render(Paragraph::new(footer), Rect { y: inner.bottom() - 1, height: 1, ..inner }, buf);

        let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(4), ..inner };
        let all = self.all;
        let now = SystemTime::now();
        let matches = self.matches();
        if matches.is_empty() {
            let empty = if self.query.is_empty() { "这个目录还没有会话，按 Tab 查看所有项目" } else { "没有匹配的会话" };
            ratatui::widgets::Widget::render(Paragraph::new(Line::styled(empty, DIM)), Rect { height: 1, ..list }, buf);
            return;
        }
        let selected = self.selected.min(matches.len() - 1);
        let fits = (list.height / ROWS).max(1) as usize;
        let mut top = self.top.min(selected);
        if selected >= top + fits { top = selected + 1 - fits; }
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, item) in matches.iter().enumerate().skip(top).take(fits) {
            let on = index == selected;
            let mark = if on { Span::styled("› ", ACCENT) } else { Span::raw("  ") };
            let title = if item.title.trim().is_empty() { "（空会话）".to_owned() } else { sanitize(item.title.lines().next().unwrap_or("")) };
            let title_style = if on { ACCENT.add_modifier(Modifier::BOLD) } else { Style::new().add_modifier(Modifier::BOLD) };
            lines.push(Line::from(vec![mark, Span::styled(fit(&title, w - 2), title_style)]));
            let prompt = sanitize(item.prompt.lines().next().unwrap_or(""));
            if !prompt.is_empty() && prompt != title {
                lines.push(Line::from(vec![Span::raw("  "), Span::styled(fit(&prompt, w - 2), DIM)]));
            }
            let mut details = format!("{} · {}", ago(now, item.modified), size(item.bytes));
            if all { details.push_str(&format!(" · {}", item.cwd)); }
            lines.push(Line::from(vec![Span::raw("  "), Span::styled(fit(&details, w - 2), DIM)]));
            lines.push(Line::raw(""));
        }
        ratatui::widgets::Widget::render(Paragraph::new(lines), list, buf);
        self.selected = selected;
        self.top = top;
    }
}

fn fit(text: &str, max: usize) -> String {
    if width(text) <= max { return text.to_owned(); }
    let mut out = String::new();
    for c in text.chars() {
        if width(&out) + width(&c.to_string()) + 1 > max { break; }
        out.push(c);
    }
    out.push('…');
    out
}

/// `刚刚`, `5 分钟前`, `3 小时前`, `2 天前`, then the date.
fn ago(now: SystemTime, then: SystemTime) -> String {
    let secs = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match secs {
        0..60 => "刚刚".into(),
        60..3_600 => format!("{} 分钟前", secs / 60),
        3_600..86_400 => format!("{} 小时前", secs / 3_600),
        86_400..604_800 => format!("{} 天前", secs / 86_400),
        _ => {
            let stamp = session::timestamp(then.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
            format!("{}-{}-{}", &stamp[0..4], &stamp[4..6], &stamp[6..8])
        }
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1_024 => format!("{bytes} B"),
        1_024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1_024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}
