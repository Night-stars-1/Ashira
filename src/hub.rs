//! `/models` hub: a full-screen, two-pane picker (providers on the left,
//! models on the right) in the alternate screen.
//! Pure state plus a render function; the TUI owns the terminal and keys.

use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    agent::tokens,
    config::{self, Entry, Level, ProviderInfo, ProviderStatus},
    render::{self, DIM},
};

const ACCENT: Color = Color::Magenta;
const SIDEBAR: u16 = 26;

pub enum Outcome {
    Stay,
    Close,
    /// Switch to this selector (`provider/id[:level]`).
    Pick(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Providers,
    Models,
    /// Second step after Enter on a model: pick its thinking level.
    Level,
}

/// Level choices in display order; `None` is the provider default.
const LEVELS: [Option<Level>; 8] = [None, Some(Level::Off), Some(Level::Minimal), Some(Level::Low), Some(Level::Medium), Some(Level::High), Some(Level::Xhigh), Some(Level::Max)];

pub struct Hub {
    providers: Vec<ProviderInfo>,
    models: Vec<Entry>,
    /// 0 = all providers, n = providers[n-1].
    provider: usize,
    focus: Focus,
    filter: String,
    /// Indices into `models` for the current provider + filter.
    shown: Vec<usize>,
    selected: usize,
    /// First visible row of the model list.
    scroll: usize,
    /// Rows the model list showed last frame, for PgUp/PgDn.
    page: usize,
    current: String,
    default: Option<String>,
    source: String,
    /// Index into `LEVELS`; starts at the current model's level.
    level: usize,
}

impl Hub {
    pub fn new(providers: Vec<ProviderInfo>, models: Vec<Entry>, current: &str, default: Option<&str>, source: String) -> Hub {
        let (base, level) = config::split_level(current);
        let mut h = Hub {
            providers,
            models,
            provider: 0,
            focus: Focus::Models,
            filter: String::new(),
            shown: Vec::new(),
            selected: 0,
            scroll: 0,
            page: 10,
            current: base.to_owned(),
            default: default.map(|d| config::split_level(d).0.to_owned()),
            source,
            level: LEVELS.iter().position(|&l| l == level).unwrap_or(0),
        };
        h.refilter();
        h.selected = h.shown.iter().position(|&i| h.models[i].selector() == h.current).unwrap_or(0);
        h
    }

    /// Discovery finished in the background: take the new lists, keep the cursor on the same model.
    pub fn update(&mut self, providers: Vec<ProviderInfo>, models: Vec<Entry>) {
        let keep = self.shown.get(self.selected).map(|&i| self.models[i].selector());
        self.providers = providers;
        self.models = models;
        self.provider = self.provider.min(self.providers.len());
        self.refilter();
        if let Some(sel) = keep {
            self.selected = self.shown.iter().position(|&i| self.models[i].selector() == sel).unwrap_or(self.selected);
        }
    }

    fn provider_id(&self) -> Option<&str> {
        self.provider.checked_sub(1).map(|i| self.providers[i].id.as_str())
    }

    /// Every whitespace-separated term must appear in `provider/id` or the name.
    fn refilter(&mut self) {
        let terms: Vec<String> = self.filter.split_whitespace().map(str::to_lowercase).collect();
        let pid = self.provider_id().map(str::to_owned);
        self.shown = (0..self.models.len())
            .filter(|&i| {
                let e = &self.models[i];
                if pid.as_deref().is_some_and(|p| p != e.provider) {
                    return false;
                }
                let hay = format!("{} {}", e.selector(), e.name.as_deref().unwrap_or("")).to_lowercase();
                terms.iter().all(|t| hay.contains(t.as_str()))
            })
            .collect();
        self.selected = self.selected.min(self.shown.len().saturating_sub(1));
    }

    fn set_provider(&mut self, p: usize) {
        self.provider = p;
        self.selected = 0;
        self.scroll = 0;
        self.refilter();
    }

    /// Indices into `LEVELS` the highlighted model accepts; 默认 is always first.
    /// Unknown support (no models.dev data) offers every level.
    fn levels(&self) -> Vec<usize> {
        let efforts = self.shown.get(self.selected).map(|&i| self.models[i].efforts.as_slice()).unwrap_or_default();
        (0..LEVELS.len()).filter(|&i| match LEVELS[i] {
            None => true,
            Some(l) => efforts.is_empty() || efforts.iter().any(|x| x == l.as_str() || (l == Level::Off && x == "none")),
        }).collect()
    }

    pub fn key(&mut self, code: KeyCode, ctrl: bool) -> Outcome {
        if self.focus == Focus::Level {
            let levels = self.levels();
            let n = levels.len();
            let at = levels.iter().position(|&i| i == self.level).unwrap_or(0);
            match code {
                KeyCode::Left | KeyCode::Up | KeyCode::BackTab => self.level = levels[(at + n - 1) % n],
                KeyCode::Right | KeyCode::Down | KeyCode::Tab => self.level = levels[(at + 1) % n],
                KeyCode::Home => self.level = levels[0],
                KeyCode::End => self.level = levels[n - 1],
                KeyCode::Enter => {
                    if let Some(&i) = self.shown.get(self.selected) {
                        let level = if levels.contains(&self.level) { LEVELS[self.level] } else { None };
                        return Outcome::Pick(config::with_level(&self.models[i].selector(), level));
                    }
                }
                KeyCode::Esc | KeyCode::Backspace => self.focus = Focus::Models,
                KeyCode::Char('c') if ctrl => return Outcome::Close,
                // Digits pick the shown levels in order: 0 = default, then 1, 2, …
                KeyCode::Char(c @ '0'..='9') => if let Some(&i) = levels.get(c as usize - '0' as usize) { self.level = i },
                _ => {}
            }
            return Outcome::Stay;
        }
        let n = self.shown.len();
        let np = self.providers.len() + 1;
        match (code, self.focus) {
            (KeyCode::Esc, _) if !self.filter.is_empty() => {
                self.filter.clear();
                self.refilter();
            }
            (KeyCode::Esc, _) => return Outcome::Close,
            (KeyCode::Char('c'), _) if ctrl => return Outcome::Close,
            (KeyCode::Left | KeyCode::BackTab, _) => self.focus = Focus::Providers,
            (KeyCode::Right | KeyCode::Tab, _) => self.focus = Focus::Models,
            (KeyCode::Up, Focus::Providers) => self.set_provider((self.provider + np - 1) % np),
            (KeyCode::Down, Focus::Providers) => self.set_provider((self.provider + 1) % np),
            (KeyCode::Enter, Focus::Providers) => self.focus = Focus::Models,
            (KeyCode::Up, Focus::Models) if n > 0 => self.selected = self.selected.checked_sub(1).unwrap_or(n - 1),
            (KeyCode::Down, Focus::Models) if n > 0 => self.selected = (self.selected + 1) % n,
            (KeyCode::PageUp, _) => self.selected = self.selected.saturating_sub(self.page),
            (KeyCode::PageDown, _) if n > 0 => self.selected = (self.selected + self.page).min(n - 1),
            (KeyCode::Home, _) => self.selected = 0,
            (KeyCode::End, _) => self.selected = n.saturating_sub(1),
            (KeyCode::Enter, Focus::Models) if n > 0 => self.focus = Focus::Level,
            (KeyCode::Backspace, _) => {
                self.filter.pop();
                self.refilter();
            }
            (KeyCode::Char(c), _) if !ctrl => {
                self.filter.push(c);
                self.focus = Focus::Models;
                self.refilter();
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height < 10 || area.width < 50 {
            Paragraph::new("窗口太小，无法显示模型列表 · Esc 返回").render(area, buf);
            return;
        }
        // Title, blank, body, blank, detail (2 rows), blank, help.
        let body = Rect { x: area.x, y: area.y + 2, width: area.width, height: area.height - 7 };
        let side = Rect { width: SIDEBAR.min(area.width / 3), ..body };
        let main = Rect { x: side.right() + 2, width: area.width.saturating_sub(side.width + 3), ..body };

        Line::from(vec![Span::styled(" 模型", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)), Span::styled(format!("   {}", self.source), DIM)])
            .render(Rect { height: 1, ..area }, buf);
        self.render_providers(side, buf);
        self.render_models(main, buf);
        self.render_detail(Rect { y: body.bottom() + 1, height: 2, ..area }, buf);
        let help = if self.focus == Focus::Level {
            " ←→ 选择思考级别 · 数字键按顺序直接选 · Enter 确认切换 · Esc 返回模型列表"
        } else {
            " ←→ 切换栏 · ↑↓ 移动 · PgUp/PgDn 翻页 · 输入文字搜索 · Enter 选择（再选思考级别） · Esc 清空搜索/关闭"
        };
        Line::styled(help, DIM).render(Rect { y: area.bottom() - 1, height: 1, ..area }, buf);
    }

    fn render_providers(&self, area: Rect, buf: &mut Buffer) {
        let focused = self.focus == Focus::Providers;
        let w = area.width as usize;
        let all = Span::styled("◆ ", Style::new().fg(ACCENT));
        let mut lines = vec![row(all, "全部模型", self.models.len(), w, self.provider == 0, focused, Style::new().add_modifier(Modifier::BOLD)), Line::raw("")];
        for (i, p) in self.providers.iter().enumerate() {
            let dot = match p.status {
                ProviderStatus::Ready if p.models > 0 => Span::styled("● ", Style::new().fg(Color::Green)),
                ProviderStatus::Ready | ProviderStatus::Unsupported(_) => Span::styled("○ ", DIM),
                ProviderStatus::Pending => Span::styled("◌ ", Style::new().fg(Color::Yellow)),
                ProviderStatus::Failed(_) => Span::styled("● ", Style::new().fg(Color::Red)),
            };
            let style = if matches!(p.status, ProviderStatus::Unsupported(_)) { DIM } else { Style::new() };
            lines.push(row(dot, &p.id, p.models, w, self.provider == i + 1, focused, style));
        }
        // Keep the selected provider visible (row index = provider + 1 for the blank).
        let skip = (self.provider + 3).saturating_sub(area.height as usize);
        Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()).render(area, buf);
    }

    fn render_models(&mut self, area: Rect, buf: &mut Buffer) {
        let focused = self.focus == Focus::Models;
        let w = area.width as usize;
        let scope = match self.provider.checked_sub(1).map(|i| &self.providers[i]) {
            Some(p) if p.api.is_empty() => format!("{} 的模型", p.id),
            Some(p) => format!("{} 的模型（{}）", p.id, p.api),
            None => "全部模型".to_owned(),
        };
        let mut head = vec![Span::styled(scope, Style::new().add_modifier(Modifier::BOLD)), Span::styled(format!("  {}/{}", self.shown.len(), self.models.len()), DIM)];
        if let Some(i) = self.provider.checked_sub(1) {
            match &self.providers[i].status {
                ProviderStatus::Failed(e) => head.push(Span::styled(format!("  获取模型列表失败：{}", fit(e, w.saturating_sub(36))), Style::new().fg(Color::Red))),
                ProviderStatus::Pending => head.push(Span::styled("  正在获取模型列表…", Style::new().fg(Color::Yellow))),
                ProviderStatus::Unsupported(e) => head.push(Span::styled(format!("  {e}"), DIM)),
                ProviderStatus::Ready => {}
            }
        }
        Line::from(head).render(Rect { height: 1, ..area }, buf);
        let search = if self.filter.is_empty() { Span::styled("输入以搜索", DIM) } else { Span::raw(self.filter.clone()) };
        Line::from(vec![Span::styled("⌕ ", Style::new().fg(ACCENT)), search]).render(Rect { y: area.y + 1, height: 1, ..area }, buf);

        let list = Rect { y: area.y + 3, height: area.height.saturating_sub(3), ..area };
        let h = (list.height as usize).max(1);
        self.page = h;
        if self.shown.is_empty() {
            let msg = if self.filter.is_empty() { "没有模型" } else { "没有匹配的模型" };
            Line::styled(msg, DIM).render(Rect { height: 1, ..list }, buf);
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + h {
            self.scroll = self.selected + 1 - h;
        }
        let (ctx_w, cost_w) = (8, 12);
        let id_w = w.saturating_sub(ctx_w + cost_w + 2);
        let end = (self.scroll + h).min(self.shown.len());
        let lines: Vec<Line> = self.shown[self.scroll..end]
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                let e = &self.models[i];
                let on = self.scroll + j == self.selected;
                let sel = e.selector();
                let tag = if sel == self.current {
                    "  当前"
                } else if self.default.as_deref() == Some(sel.as_str()) {
                    "  默认"
                } else {
                    ""
                };
                let fixed = 2 + render::width(&e.provider) + 1 + render::width(tag);
                let id = fit(&e.id, id_w.saturating_sub(fixed));
                let pad = id_w.saturating_sub(fixed + render::width(&id));
                let cost = e.cost.map(|(i, o)| format!("${}/{}", num(i), num(o))).unwrap_or_default();
                let base = sel_style(on, focused);
                Line::from(vec![
                    Span::styled(if on { "› " } else { "  " }, base),
                    Span::styled(format!("{}/", e.provider), if on { base } else { DIM }),
                    Span::styled(id, base.add_modifier(Modifier::BOLD)),
                    Span::styled(tag, Style::new().fg(Color::Green)),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(format!("{:>ctx_w$}", tokens(e.context_window)), DIM),
                    Span::styled(format!("{cost:>cost_w$}"), DIM),
                ])
            })
            .collect();
        Paragraph::new(lines).render(list, buf);
    }

    fn render_detail(&self, area: Rect, buf: &mut Buffer) {
        let Some(e) = self.shown.get(self.selected).map(|&i| &self.models[i]) else { return };
        let mut facts = vec![format!("上下文 {}", tokens(e.context_window))];
        if let Some(m) = e.max_tokens {
            facts.push(format!("最大输出 {}", tokens(u64::from(m))));
        }
        if let Some((i, o)) = e.cost {
            facts.push(format!("输入/输出 ${}/{} 每百万 token", num(i), num(o)));
        }
        let name = e.name.clone().unwrap_or_else(|| e.id.clone());
        Line::from(vec![
            Span::styled(format!(" {name}"), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", e.selector()), DIM),
            Span::styled(format!("  ·  {}", facts.join(" · ")), DIM),
        ])
        .render(Rect { height: 1, ..area }, buf);
        // Second row: the thinking levels this model accepts (all when unknown);
        // active (highlighted) after Enter.
        let picking = self.focus == Focus::Level;
        let levels = self.levels();
        // Moving to a model without the chosen level previews and picks 默认 instead.
        let chosen = if levels.contains(&self.level) { self.level } else { 0 };
        let mut row = vec![Span::styled(" 思考级别  ", if picking { Style::new().fg(ACCENT).add_modifier(Modifier::BOLD) } else { DIM })];
        for &i in &levels {
            let label = format!(" {} ", Level::label(LEVELS[i]));
            let style = match (i == chosen, picking) {
                (true, true) => Style::new().fg(ACCENT).add_modifier(Modifier::REVERSED | Modifier::BOLD),
                (true, false) => Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                (false, true) => Style::new(),
                (false, false) => DIM,
            };
            row.push(Span::styled(label, style));
            row.push(Span::raw(" "));
        }
        if !picking {
            row.push(Span::styled("  Enter 后选择", DIM));
        }
        Line::from(row).render(Rect { y: area.y + 1, height: 1, ..area }, buf);
    }
}

fn sel_style(on: bool, focused: bool) -> Style {
    match (on, focused) {
        (true, true) => Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        (true, false) => Style::new().add_modifier(Modifier::BOLD),
        _ => Style::new(),
    }
}

/// `› ● label ... count` filling `w` columns.
fn row(mark: Span<'static>, label: &str, count: usize, w: usize, on: bool, focused: bool, style: Style) -> Line<'static> {
    let count = count.to_string();
    let label = fit(label, w.saturating_sub(count.len() + 6));
    let gap = w.saturating_sub(render::width(&label) + count.len() + 5);
    Line::from(vec![
        Span::styled(if on { "› " } else { "  " }, sel_style(on, focused)),
        mark,
        Span::styled(label, if on { sel_style(true, focused) } else { style }),
        Span::raw(" ".repeat(gap)),
        Span::styled(count, DIM),
    ])
}

fn num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }
}

fn fit(s: &str, max: usize) -> String {
    if render::width(s) <= max {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}
