//! Reports and the interactive server list for MCP commands.

use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Widget},
};

use crate::{mcp::{Mcp, ServerView, Status}, render::{DIM, sanitize}, report::Report};

const ACCENT: Style = Style::new().fg(Color::Cyan);
const SUCCESS: Style = Style::new().fg(Color::Green);
const WARNING: Style = Style::new().fg(Color::Yellow);

pub enum View {
    Help,
    Servers,
    Tools(String),
    Resources(String),
    Prompts(String),
}

pub enum PickerAction {
    Stay,
    Close,
    Toggle { name: String, enabled: bool },
    Reconnect(String),
}

struct ServerPicker {
    rows: Vec<ServerView>,
    warnings: Vec<String>,
    selected: usize,
    top: usize,
    viewport: usize,
    detail: bool,
    notice: Option<(String, Style)>,
}

pub struct McpReport {
    view: View,
    pub report: Report,
    picker: Option<ServerPicker>,
}

impl McpReport {
    pub fn new(view: View, mcp: &Mcp) -> Self {
        let picker = matches!(view, View::Servers).then(|| {
            let (mut rows, warnings) = mcp.servers();
            rows.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            ServerPicker { rows, warnings, selected: 0, top: 0, viewport: 1, detail: false, notice: None }
        });
        let title = match view {
            View::Help => "MCP Server Management",
            View::Servers => "MCP 服务器详情",
            View::Tools(_) => "MCP Tools",
            View::Resources(_) => "MCP Resources",
            View::Prompts(_) => "MCP Prompts",
        };
        let report = Report::new(title, body(&view, mcp));
        Self { view, report, picker }
    }

    pub fn refresh(&mut self, mcp: &Mcp) {
        if let Some(picker) = &mut self.picker {
            let (mut rows, warnings) = mcp.servers();
            rows.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            let selection = picker.rows.get(picker.selected).and_then(|old| rows.iter().position(|row| row.name == old.name));
            if selection.is_none() { picker.detail = false; }
            picker.selected = selection.unwrap_or_else(|| picker.selected.min(rows.len().saturating_sub(1)));
            picker.rows = rows;
            picker.warnings = warnings;
            if picker.detail && let Some(row) = picker.rows.get(picker.selected) {
                self.report.set_body(server_detail(row, mcp));
            }
        } else if !matches!(self.view, View::Help) {
            self.report.set_body(body(&self.view, mcp));
        }
    }

    pub fn is_picker(&self) -> bool { self.picker.is_some() }

    pub fn picker_notice(&mut self, result: Result<String, String>) {
        if let Some(picker) = &mut self.picker {
            picker.notice = Some(match result {
                Ok(text) => (single_line(&text), SUCCESS),
                Err(error) => (single_line(&error), WARNING),
            });
        }
    }

    pub fn picker_key(&mut self, key: KeyCode, mcp: &Mcp) -> PickerAction {
        let Some(picker) = &mut self.picker else { return PickerAction::Stay };
        if picker.detail {
            match key {
                KeyCode::Esc | KeyCode::Left => picker.detail = false,
                KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k') => self.report.page(true),
                KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j') => self.report.page(false),
                KeyCode::Home => self.report.home(),
                KeyCode::End => self.report.end(),
                _ => {}
            }
            return PickerAction::Stay;
        }
        if matches!(key, KeyCode::Esc | KeyCode::Left) { return PickerAction::Close; }
        let Some(row) = picker.rows.get(picker.selected) else { return PickerAction::Stay };
        let last = picker.rows.len() - 1;
        match key {
            KeyCode::Up | KeyCode::Char('k') => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => picker.selected = (picker.selected + 1).min(last),
            KeyCode::PageUp => picker.selected = picker.selected.saturating_sub(picker.viewport),
            KeyCode::PageDown => picker.selected = picker.selected.saturating_add(picker.viewport).min(last),
            KeyCode::Home => picker.selected = 0,
            KeyCode::End => picker.selected = last,
            KeyCode::Enter | KeyCode::Right => {
                self.report.set_body(server_detail(row, mcp));
                self.report.home();
                picker.detail = true;
            }
            KeyCode::Char(' ') => return PickerAction::Toggle { name: row.name.clone(), enabled: row.status.is_none() },
            KeyCode::Char('r') if row.status.is_some() => return PickerAction::Reconnect(row.name.clone()),
            KeyCode::Char('r') => picker.notice = Some(("服务器已停用，先按空格启用。".into(), WARNING)),
            _ => {}
        }
        PickerAction::Stay
    }

    pub fn picker_height(&mut self, width: u16) -> usize {
        let Some(picker) = &self.picker else { return 0 };
        if picker.detail {
            return self.report.height(width.min(88)).min(16);
        }
        let chrome = 3 + usize::from(!picker.rows.is_empty())
            + usize::from(picker.notice.is_some() || !picker.warnings.is_empty());
        picker.rows.len().max(1).min(12 - chrome) + chrome
    }

    pub fn picker_render(&mut self, area: Rect, buf: &mut Buffer) {
        let Some(picker) = &mut self.picker else { return };
        if picker.detail { self.report.render(Rect { width: area.width.min(88), ..area }, buf); }
        else { picker.render(Rect { width: area.width.min(72), ..area }, buf); }
    }
}

fn body(view: &View, mcp: &Mcp) -> Vec<Line<'static>> {
    match view {
        View::Help => help(),
        View::Servers => Vec::new(),
        View::Tools(server) => {
            let tools = mcp.tools(server);
            if tools.is_empty() {
                return vec![Line::styled("没有可用的 MCP 工具；用 /mcp list 查看服务器状态。", DIM)];
            }
            tools.into_iter().flat_map(|(name, description)| [
                Line::styled(name, ACCENT),
                Line::styled(format!("  {description}"), DIM),
            ]).collect()
        }
        View::Resources(server) | View::Prompts(server) => {
            let prompts = matches!(view, View::Prompts(_));
            let rows = mcp.listing(server, prompts);
            if rows.is_empty() {
                return vec![Line::styled(if prompts { "已连接的服务器没有提供提示词。" } else { "已连接的服务器没有提供资源。" }, DIM)];
            }
            let mut lines = Vec::new();
            let mut previous = String::new();
            for (server, key, description) in rows {
                if previous != server {
                    if !lines.is_empty() {
                        lines.push(Line::raw(""));
                    }
                    lines.push(Line::styled(format!("{server}:"), ACCENT));
                    previous = server;
                }
                lines.push(Line::from(vec![Span::raw("  "), Span::styled(key, SUCCESS), Span::styled(format!("  {description}"), DIM)]));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(if prompts {
                "用法：/<服务器>:<提示词> 参数（key=value 或按顺序），结果作为下一条消息发送。"
            } else {
                "模型可用 read mcp://<uri> 读取资源。"
            }, DIM));
            lines
        }
    }
}

fn help() -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw("管理 Model Context Protocol (MCP) 服务器和外部工具。"),
        Line::raw(""),
        Line::styled("命令：", ACCENT),
    ];
    for (command, description) in [
        ("/mcp add <名称> -- <命令> [参数…]", "添加 stdio 服务器"),
        ("/mcp add <名称> --url <地址>", "添加 HTTP 服务器；--type sse 使用旧版 SSE"),
        ("  --scope project|user", "配置作用域（默认 user）；--env K=V / --header K=V"),
        ("/mcp list", "打开服务器列表：选择、查看工具、启停和重连"),
        ("/mcp tools [服务器]", "查看工具名称和描述"),
        ("/mcp resources [服务器]", "查看服务器提供的资源"),
        ("/mcp prompts [服务器]", "查看提示词及调用参数"),
        ("/mcp test <名称>", "测试连接，显示工具数量和耗时"),
        ("/mcp reconnect <名称>", "重新连接指定服务器"),
        ("/mcp reload", "重新读取配置并连接服务器"),
        ("/mcp enable <名称>", "启用服务器"),
        ("/mcp disable <名称>", "停用服务器"),
        ("/mcp remove <名称>", "删除 Ashira 自己配置的服务器"),
        ("/mcp help", "显示此帮助"),
    ] {
        // Column width is in terminal cells, not Unicode scalar values.
        let gap = 39usize.saturating_sub(crate::render::width(command)).max(2);
        lines.push(Line::from(vec![Span::raw(format!("  {command}{}", " ".repeat(gap))), Span::raw(description)]));
    }
    lines
}

impl ServerPicker {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        let frame = Block::new().border_type(BorderType::Rounded).borders(Borders::ALL)
            .border_style(DIM).title(Line::styled(" MCP 服务器 ", ACCENT));
        let inner = frame.inner(area);
        frame.render(area, buf);
        if inner.is_empty() { return; }
        let row_at = |y| Rect { y, height: 1, ..inner };
        let row = |offset: u16| row_at(inner.y + offset);
        Line::styled(if inner.width < 48 { "↑↓ · Enter · Space · r · Esc" } else { "↑↓ 选择 · Enter 详情 · Space 启停 · r 重连 · Esc 返回" }, DIM)
            .render(row_at(inner.bottom() - 1), buf);
        if inner.height < 2 { return; }
        let selected = self.rows.get(self.selected);
        let mut end = inner.height - 1;
        if end > 1 && let Some((text, style)) = &self.notice {
            end -= 1;
            Line::styled(text.as_str(), *style).render(row(end), buf);
        } else if end > 1 && let Some(warning) = self.warnings.first() {
            end -= 1;
            Line::styled(single_line(warning), WARNING).render(row(end), buf);
        }
        if end > 1 && let Some(server) = selected {
            end -= 1;
            Line::styled(format!("来源  {}", single_line(&short_path(&server.source))), DIM).render(row(end), buf);
        }
        if self.rows.is_empty() {
            Line::styled("没有配置 MCP 服务器。用 /mcp add 添加。", DIM).render(row(0), buf);
            return;
        }
        self.viewport = usize::from(end);
        self.top = self.top.clamp((self.selected + 1).saturating_sub(self.viewport), self.selected)
            .min(self.rows.len().saturating_sub(self.viewport));
        for (offset, server) in self.rows.iter().skip(self.top).take(self.viewport).enumerate() {
            let rect = row(offset as u16);
            let selected = self.top + offset == self.selected;
            if selected { buf.set_style(rect, Style::new().bg(Color::Rgb(30, 44, 53))); }
            let (label, style) = status(server);
            let mut line = vec![Span::styled(if selected { " › " } else { "   " }, ACCENT), Span::styled(label, style), Span::raw("  "), Span::raw(single_line(&server.name))];
            if !server.kind.is_empty() { line.push(Span::styled(format!("  [{}]", server.kind), DIM)); }
            Line::from(line).render(rect, buf);
        }
    }
}

fn status(server: &ServerView) -> (&'static str, Style) {
    match &server.status {
        Some(Status::Connected) => ("● 已连接", SUCCESS),
        Some(Status::Connecting) => ("◌ 连接中", DIM),
        Some(Status::Failed(_)) => ("○ 连接失败", WARNING),
        None => ("◌ 已停用", WARNING),
    }
}

fn single_line(text: &str) -> String { sanitize(text).replace('\n', " ") }

fn server_detail(server: &ServerView, mcp: &Mcp) -> Vec<Line<'static>> {
    let (label, style) = status(server);
    let mut lines = vec![
        Line::from(vec![Span::styled(single_line(&server.name), ACCENT), Span::styled(format!("   {label}"), style)]),
        Line::styled(format!("来源：{}", single_line(&server.source)), DIM),
    ];
    if !server.kind.is_empty() { lines.push(Line::styled(format!("传输：{}", server.kind), DIM)); }
    if let Some(Status::Failed(error)) = &server.status { lines.push(Line::styled(single_line(error), WARNING)); }
    lines.push(Line::raw(""));
    let tools = mcp.tools(&server.name);
    lines.push(Line::styled(format!("工具（{}）", tools.len()), ACCENT));
    if tools.is_empty() { lines.push(Line::styled("没有可用工具。", DIM)); }
    for (name, description) in tools {
        lines.push(Line::styled(single_line(&name), ACCENT));
        if !description.is_empty() { lines.push(Line::styled(format!("  {}", single_line(&description)), DIM)); }
    }
    lines
}

fn short_path(path: &str) -> String {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_owned(),
    }
}
