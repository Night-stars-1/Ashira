use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Widget},
};

use crate::{
    render::{DIM, sanitize, wrap},
    report::Report,
    subagents::{Snapshot, Status},
};

const SPLIT_WIDTH: u16 = 84;
const ACCENT: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);

pub struct AgentHub {
    pub selected: Option<String>,
    pub transcript: Option<(String, u64, Option<Report>)>,
    roster_top: usize,
    narrow_details: bool,
    split: bool,
    detail_top: usize,
    detail_height: usize,
}

impl AgentHub {
    pub fn new(jobs: &[Snapshot]) -> Self {
        Self {
            selected: jobs.first().map(|job| job.identity.id.clone()),
            transcript: None,
            roster_top: 0,
            narrow_details: false,
            split: false,
            detail_top: 0,
            detail_height: 0,
        }
    }

    fn select(&mut self, job: Option<&Snapshot>) {
        let id = job.map(|job| job.identity.id.as_str());
        if self.selected.as_deref() != id {
            self.selected = id.map(str::to_owned);
            self.detail_top = 0;
        }
    }

    fn selected_index(&self, jobs: &[Snapshot]) -> Option<usize> {
        let id = self.selected.as_deref()?;
        jobs.iter().position(|job| job.identity.id == id)
    }

    pub fn sync_selection(&mut self, jobs: &[Snapshot]) {
        if self.selected_index(jobs).is_none() { self.select(jobs.first()); }
        if jobs.is_empty() { self.narrow_details = false; }
    }

    pub fn move_selection(&mut self, jobs: &[Snapshot], down: bool) {
        if jobs.is_empty() { return; }
        let index = self.selected_index(jobs).unwrap_or(0);
        let next = if down { (index + 1) % jobs.len() } else { (index + jobs.len() - 1) % jobs.len() };
        self.select(jobs.get(next));
    }

    pub fn edge_selection(&mut self, jobs: &[Snapshot], first: bool) {
        self.select(if first { jobs.first() } else { jobs.last() });
    }

    pub fn toggle_details(&mut self, jobs: &[Snapshot]) {
        if !self.split && !jobs.is_empty() { self.narrow_details = !self.narrow_details; }
    }

    pub fn back(&mut self) -> bool {
        !self.split && std::mem::take(&mut self.narrow_details)
    }

    pub fn page_details(&mut self, up: bool) {
        if self.split || self.narrow_details {
            let step = self.detail_height.saturating_sub(1).max(1);
            self.detail_top = if up { self.detail_top.saturating_sub(step) } else { self.detail_top.saturating_add(step) };
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, jobs: &[Snapshot]) {
        let area = area.intersection(buf.area);
        if area.is_empty() { return; }
        self.sync_selection(jobs);
        if let Some((id, _, report)) = &mut self.transcript {
            if let Some(job) = jobs.iter().find(|job| job.identity.id == *id) {
                let (glyph, style) = status_style(job.status);
                Line::from(vec![Span::styled(glyph, style), Span::styled(one_line(&job.identity.name), ACCENT), Span::styled(format!(" · {}", status_label(job.status)), DIM)])
                    .render(Rect { height: 1, ..area }, buf);
            }
            let body = Rect { y: area.y + 1, height: area.height - 1, ..area };
            if let Some(report) = report {
                report.render(body, buf);
            } else {
                Line::styled("正在读取日志… · Esc 返回", DIM).render(Rect { height: body.height.min(1), ..body }, buf);
            }
            return;
        }

        let frame = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded)
            .border_style(DIM).title(Line::styled(" Agent Hub · 子代理 ", ACCENT));
        let inner = frame.inner(area);
        frame.render(area, buf);
        self.split = inner.width >= SPLIT_WIDTH && inner.height >= 4;
        if inner.is_empty() { return; }

        let body = Rect { width: inner.width.saturating_sub(2), x: inner.x + 1, height: inner.height - 1, ..inner };
        let selected = self.selected_index(jobs);
        if body.is_empty() {
            // Leave the last available row for the close hint.
        } else if self.split {
            let roster = Rect { width: (u32::from(body.width) * 11 / 20) as u16, ..body };
            let divider = roster.right() + 1;
            for y in body.y..body.bottom() {
                buf[(divider, y)].set_symbol("│").set_style(DIM);
            }
            let detail = Rect { x: divider + 2, width: body.right() - divider - 2, ..body };
            self.roster(roster, buf, jobs, selected);
            self.details(detail, buf, selected.map(|index| &jobs[index]));
        } else if self.narrow_details {
            self.details(body, buf, selected.map(|index| &jobs[index]));
        } else {
            self.roster(body, buf, jobs, selected);
        }
        let hint = if inner.width < 44 {
            "Esc 返回 · Tab 详情 · Enter 日志"
        } else if self.split {
            "↑↓ / j k 选择 · Enter 日志 · PgUp/PgDn 详情 · x 取消 · Esc 返回"
        } else {
            "↑↓ / j k 选择 · Tab 切换详情 · Enter 日志 · x 取消 · Esc 返回"
        };
        Line::styled(hint, DIM).render(Rect { y: inner.bottom() - 1, height: 1, ..body }, buf);
    }

    fn roster(&mut self, area: Rect, buf: &mut Buffer, jobs: &[Snapshot], selected: Option<usize>) {
        let (mut active, mut completed, mut failed) = (0, 0, 0);
        for job in jobs {
            match job.status {
                Status::Completed => completed += 1,
                Status::Failed => failed += 1,
                status if status.active() => active += 1,
                _ => {}
            }
        }
        Line::from(vec![Span::styled("代理列表", ACCENT), Span::styled(format!("  {} 个 · {} 活动 · {} 完成 · {} 失败", jobs.len(), active, completed, failed), DIM)])
            .render(Rect { height: 1, ..area }, buf);
        if area.height < 2 { return; }
        let Some(selected) = selected else {
            Line::styled("本次运行没有子代理", DIM).render(Rect { y: area.y + 1, height: 1, ..area }, buf);
            return;
        };
        let rows = (area.height - 1) as usize;
        self.roster_top = self.roster_top.clamp((selected + 1).saturating_sub(rows), selected)
            .min(jobs.len().saturating_sub(rows));
        let show_status = area.width >= 30;
        let status_width = if show_status { 10 } else { 0 };
        for (offset, job) in jobs.iter().skip(self.roster_top).take(rows).enumerate() {
            let row = Rect { y: area.y + 1 + offset as u16, height: 1, ..area };
            let selected = self.roster_top + offset == selected;
            let (glyph, status) = status_style(job.status);
            if selected { buf.set_style(row, Style::new().bg(Color::Rgb(30, 44, 53))); }
            Line::from(vec![
                Span::styled(if selected { "› " } else { "  " }, ACCENT),
                Span::styled(glyph, status),
                Span::styled(one_line(&job.identity.name), if selected { ACCENT } else { Style::default() }),
            ]).render(Rect { width: row.width - status_width, ..row }, buf);
            if show_status {
                Line::styled(status_label(job.status), status).render(Rect { x: row.right() - status_width + 1, width: status_width - 1, ..row }, buf);
            }
        }
    }

    fn details(&mut self, area: Rect, buf: &mut Buffer, job: Option<&Snapshot>) {
        Line::styled("代理详情", ACCENT).render(Rect { height: 1, ..area }, buf);
        self.detail_height = area.height.saturating_sub(1) as usize;
        let Some(job) = job else {
            if self.detail_height > 0 { Line::styled("请选择子代理", DIM).render(Rect { y: area.y + 1, height: 1, ..area }, buf); }
            return;
        };
        if self.detail_height == 0 { return; }
        let (glyph, status) = status_style(job.status);
        let mut lines = vec![Line::from(vec![Span::styled(glyph, status), Span::styled(one_line(&job.identity.name), ACCENT)])];
        let pid = job.worker_pid.map_or_else(|| "—".into(), |pid| pid.to_string());
        for (label, text) in [
            ("状态", format!("{} · {:.1}s", status_label(job.status), job.elapsed_ms as f64 / 1000.0)),
            ("模型", format!("{} · {}", job.model, job.identity.role.as_str())),
            ("ID", job.identity.id.clone()),
            ("进程", format!("代次 {} · PID {pid} · 收件箱 {}{}", job.generation, job.pending_messages, if job.closed { " · 已关闭" } else { "" })),
        ] {
            add_detail(&mut lines, label, &text, area.width);
        }
        if let Some(error) = &job.error { add_detail(&mut lines, "错误", error, area.width); }
        if !job.activity.is_empty() { add_detail(&mut lines, "活动", &job.activity, area.width); }
        add_detail(&mut lines, "任务", &job.task, area.width);
        self.detail_top = self.detail_top.min(lines.len().saturating_sub(self.detail_height));
        for (offset, line) in lines.into_iter().skip(self.detail_top).take(self.detail_height).enumerate() {
            line.render(Rect { y: area.y + 1 + offset as u16, height: 1, ..area }, buf);
        }
    }
}

fn one_line(text: &str) -> String { sanitize(text).replace('\n', " ") }

fn add_detail(lines: &mut Vec<Line<'static>>, label: &'static str, value: &str, width: u16) {
    let text = sanitize(value);
    let mut rows = text.split('\n');
    let prefix = format!("{label}  ");
    lines.extend(wrap(&[Span::styled(prefix, DIM), Span::raw(rows.next().unwrap_or_default())], width));
    for row in rows { lines.extend(wrap(&[Span::raw(row)], width)); }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Queued => "排队中",
        Status::Running => "运行中",
        Status::WaitingApproval => "待确认",
        Status::WaitingMessage => "等消息",
        Status::Cancelling => "取消中",
        Status::Completed => "已完成",
        Status::Failed => "失败",
        Status::Cancelled => "已取消",
    }
}

fn status_style(status: Status) -> (&'static str, Style) {
    match status {
        Status::Running => ("● ", Style::new().fg(Color::Cyan)),
        Status::Completed => ("✓ ", Style::new().fg(Color::Green)),
        Status::Failed => ("× ", Style::new().fg(Color::Red)),
        Status::WaitingApproval | Status::WaitingMessage | Status::Cancelling => ("◉ ", Style::new().fg(Color::Yellow)),
        Status::Queued | Status::Cancelled => ("○ ", DIM),
    }
}
