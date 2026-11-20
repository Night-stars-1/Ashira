//! A width-cached, scrollable single-column report.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::render::{sanitize, wrap};

// Top border, top padding, bottom padding, footer, bottom border.
const CHROME: usize = 5;

pub struct Report {
    title: String,
    body: Vec<Line<'static>>,
    wrapped: Vec<Line<'static>>,
    cached_width: Option<u16>,
    scroll: usize,
    viewport: usize,
    raw_output: Option<(String, Style)>,
}

impl Report {
    pub fn new(title: &'static str, body: Vec<Line<'static>>) -> Self {
        Self {
            title: sanitize(title).replace('\n', " "),
            body,
            wrapped: Vec::new(),
            cached_width: None,
            scroll: 0,
            raw_output: None,
            viewport: 0,
        }
    }



    pub fn raw(title: &'static str, text: String, is_error: bool) -> Self {
        let text = if text.chars().any(|c| c == '\t' || (c.is_control() && c != '\n')) { sanitize(&text) } else { text };
        let style = if is_error { Style::new().fg(Color::Red) } else { crate::render::DIM };
        let mut report = Self::new(title, Vec::new());
        report.raw_output = Some((text, style));
        report
    }

    pub fn set_body(&mut self, body: Vec<Line<'static>>) {
        self.body = body;
        self.raw_output = None;
        self.cached_width = None;
    }

    fn reflow(&mut self, width: u16) {
        if self.cached_width == Some(width) {
            return;
        }
        self.wrapped.clear();
        let content_width = width.saturating_sub(4).max(1);
        if let Some((text, style)) = &self.raw_output {
            for logical in text.split('\n') {
                let mut start = 0;
                let mut used = 0;
                for (index, ch) in logical.char_indices() {
                    let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if used + width > content_width as usize && index > start {
                        self.wrapped.push(Line::styled(logical[start..index].to_owned(), *style));
                        start = index;
                        used = 0;
                    }
                    used += width;
                }
                self.wrapped.push(Line::styled(logical[start..].to_owned(), *style));
            }
            self.cached_width = Some(width);
            return;
        }
        for line in &self.body {
            let mut spans = Vec::new();
            for span in &line.spans {
                let text = sanitize(&span.content);
                let mut parts = text.split('\n').peekable();
                while let Some(part) = parts.next() {
                    spans.push(Span::styled(part.to_owned(), span.style));
                    if parts.peek().is_some() {
                        for mut row in wrap(&spans, content_width) {
                            row.style = line.style;
                            row.alignment = line.alignment;
                            self.wrapped.push(row);
                        }
                        spans.clear();
                    }
                }
            }
            for mut row in wrap(&spans, content_width) {
                row.style = line.style;
                row.alignment = line.alignment;
                self.wrapped.push(row);
            }
        }
        self.cached_width = Some(width);
    }

    pub fn height(&mut self, width: u16) -> usize {
        self.reflow(width);
        self.wrapped.len().saturating_add(CHROME)
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        self.reflow(area.width);
        self.viewport = (area.height as usize).saturating_sub(CHROME);
        self.scroll = self.scroll.min(self.wrapped.len().saturating_sub(self.viewport));

        let title = Line::from(vec![
            Span::styled("─", Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" {} ", self.title), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title)
            .render(area, buf);

        if area.width <= 4 || area.height < CHROME as u16 {
            return;
        }
        let x = area.x.saturating_add(2);
        let content_width = area.width - 4;
        for (offset, line) in self.wrapped.iter().skip(self.scroll).take(self.viewport).enumerate() {
            let row = Rect::new(x, area.y + 2 + offset as u16, content_width, 1);
            line.render(row, buf);
        }
        let footer = if self.raw_output.is_some() {
            "PgUp/PgDn scroll · Home/End · Ctrl+O/Esc close"
        } else if self.wrapped.len() > self.viewport {
            "PgUp/PgDn 滚动 · Esc 关闭"
        } else {
            "Esc 关闭"
        };
        Paragraph::new(footer)
            .style(Style::default().fg(Color::DarkGray))
            .render(Rect::new(x, area.bottom() - 2, content_width, 1), buf);
    }

    pub fn page(&mut self, up: bool) {
        let step = self.viewport.max(1);
        if up {
            self.scroll = self.scroll.saturating_sub(step);
        } else {
            self.scroll = self.scroll.saturating_add(step)
                .min(self.wrapped.len().saturating_sub(self.viewport));
        }
    }

    pub fn home(&mut self) {
        self.scroll = 0;
    }

    pub fn end(&mut self) {
        self.scroll = self.wrapped.len().saturating_sub(self.viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_subagent_transcript_viewer_flow() {
        fn draw(report: &mut Report, width: u16) -> String {
            let area = Rect::new(0, 0, width, 12);
            let mut buffer = Buffer::empty(area);
            report.render(area, &mut buffer);
            (0..area.height).map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < area.width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
                }
                row
            }).collect::<Vec<_>>().join("\n")
        }

        let text = format!("FIRST\n\n{}\nLAST 界界界界界界界界", (1..=20).map(|n| format!("row {n:02}")).collect::<Vec<_>>().join("\n"));
        let mut report = Report::raw("Subagent transcript", text, false);
        let first = draw(&mut report, 40);
        assert!(first.contains("FIRST"));
        assert!(first.contains("row 01"));
        assert!(!first.contains("LAST"));
        report.page(false);
        let next = draw(&mut report, 40);
        assert!(!next.contains("FIRST"));
        assert!(next.contains("row 10"));
        report.end();
        assert!(draw(&mut report, 40).contains("LAST 界界界界界界界界"));
        draw(&mut report, 20);
        report.end();
        let resized = draw(&mut report, 20);
        assert!(resized.contains("LAST 界界界界界"));
        assert!(resized.contains("界界界"));
        report.home();
        let first_again = draw(&mut report, 20);
        assert!(first_again.contains("FIRST"));
        assert!(first_again.contains("row 01"));
        report.set_body(vec![Line::raw("replacement")]);
        let replaced = draw(&mut report, 40);
        assert!(replaced.contains("replacement"));
        assert!(!replaced.contains("FIRST"));
    }
}
