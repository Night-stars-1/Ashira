use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{style::{Color, Modifier, Style}, text::{Line, Span}};
use super::{highlight::CodeHighlighter, width as text_width, wrap, DIM};

fn options() -> Options { Options::ENABLE_STRIKETHROUGH }

// The last top-level block may still change when more input arrives.
pub(super) fn last_block_start(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut last = None;
    let line_start = |offset: usize| text[..offset].rfind('\n').map_or(0, |at| at + 1);
    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(_) => { if depth == 0 { last = Some(line_start(range.start)); } depth += 1; }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Rule if depth == 0 => last = Some(line_start(range.start)),
            _ => {}
        }
    }
    last
}

pub(super) fn top_level_fence_at(text: &str, offset: usize) -> bool {
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 && range.start >= offset && matches!(tag, Tag::CodeBlock(CodeBlockKind::Fenced(_))) { return true; }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

pub(super) fn top_level_paragraph_at(text: &str, offset: usize) -> bool {
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 && range.start == offset { return matches!(tag, Tag::Paragraph); }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

// Inline cells use exactly the same event styling as paragraphs.
pub(super) fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    if text.trim().is_empty() { return Vec::new(); }
    let mut renderer = Renderer::new(u16::MAX, base, true, false);
    renderer.events(&format!("x {text}"));
    if let Some(first) = renderer.spans.first_mut()
        && let Some(rest) = first.content.strip_prefix('x')
    {
        first.content = rest.strip_prefix(' ').unwrap_or(rest).to_owned().into();
    }
    renderer.spans
}

pub(super) fn render(text: &str, width: u16) -> Vec<Line<'static>> {
    let mut renderer = Renderer::new(width, Style::new(), false, true);
    renderer.events(text);
    renderer.flush();
    renderer.rows
}

pub(super) fn render_preview(text: &str, width: u16) -> Vec<Line<'static>> {
    let mut renderer = Renderer::new(width, Style::new(), false, false);
    renderer.events(text);
    renderer.flush();
    renderer.rows
}

struct CodeBlock {
    highlighter: Option<CodeHighlighter>,
    content: String,
    closing: Option<(String, usize)>,
}

struct Renderer {
    width: u16,
    rows: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    style: Style,
    styles: Vec<Style>,
    links: Vec<(String, String)>,
    lists: Vec<Option<u64>>,
    item_prefix: Option<String>,
    quote: usize,
    quote_gap: bool,
    quote_rows: Vec<usize>,
    inline: bool,
    code: Option<CodeBlock>,
    highlight_code: bool,
}
impl Renderer {
    fn new(width: u16, style: Style, inline: bool, highlight_code: bool) -> Self {
        Self { width: width.max(1), rows: Vec::new(), spans: Vec::new(), style, styles: Vec::new(), links: Vec::new(), lists: Vec::new(), item_prefix: None, quote: 0, quote_gap: false, quote_rows: Vec::new(), inline, code: None, highlight_code }
    }
    fn push_style(&mut self, style: Style) { self.styles.push(self.style); self.style = style; }
    fn pop_style(&mut self) { if let Some(style) = self.styles.pop() { self.style = style; } }
    fn flush(&mut self) {
        if self.spans.is_empty() && self.item_prefix.is_none() { return; }
        let prefix = self.item_prefix.take();
        let indent = self.quote * 2 + self.lists.len().saturating_sub(1) * 2 + if prefix.is_none() && !self.lists.is_empty() { 2 } else { 0 };
        let reserve = if self.width > 1 { 2 } else { 1 };
        let indent = indent.min(self.width.saturating_sub(reserve) as usize);
        let marker = prefix.as_ref().map(|p| p.chars().take((self.width as usize).saturating_sub(indent + reserve as usize)).collect::<String>());
        let marker_width = marker.as_ref().map_or(0, |p| text_width(p));
        let continuation = (indent + marker_width).min(self.width.saturating_sub(reserve) as usize);
        let available = (self.width as usize).saturating_sub(continuation).max(1).min(u16::MAX as usize) as u16;
        let wrapped = wrap(&self.spans, available);
        for (i, line) in wrapped.into_iter().enumerate() {
            let mut spans = Vec::new();
            let pad = if i == 0 { indent } else { continuation };
            if pad > 0 { spans.push(Span::raw(" ".repeat(pad))); }
            if i == 0 && let Some(marker) = &marker { spans.push(Span::styled(marker.clone(), Style::new().fg(Color::Cyan))); }
            spans.extend(line.spans);
            self.rows.push(Line::from(spans));
        }
        self.spans.clear();
    }
    fn events(&mut self, text: &str) {
        for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => { if self.quote > 0 && self.quote_gap { self.rows.push(Line::raw("")); self.quote_gap = false; } },
                    Tag::Heading { level, .. } => {
                        self.flush();
                        let style = if level == HeadingLevel::H1 { Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD) };
                        self.push_style(style);
                    }
                    Tag::CodeBlock(kind) => {
                        self.flush();
                        let (info, closing) = match kind {
                            CodeBlockKind::Fenced(info) => {
                                let source = &text[range.clone()];
                                let mut lines = source.lines();
                                let first = lines.next().unwrap_or("");
                                let opening = first.find(['`', '~']).map_or(first.trim_start(), |at| &first[at..]);
                                self.spans.push(Span::styled(opening.to_owned(), DIM));
                                self.flush();
                                let marker = opening.chars().next().unwrap_or('`');
                                let count = opening.chars().take_while(|c| *c == marker).count();
                                let last_line = lines.last().unwrap_or("");
                                let marker_at = last_line.find(marker);
                                let last = marker_at.map_or("", |at| &last_line[at..]);
                                let close_count = last.chars().take_while(|c| *c == marker).count();
                                let closing = (count >= 3 && close_count >= count && last[close_count..].trim().is_empty()).then(|| {
                                    let line_start = source.rfind(last_line).unwrap_or(0);
                                    (last.to_owned(), range.start + line_start + marker_at.unwrap_or(0))
                                });
                                (info.to_string(), closing)
                            }
                            CodeBlockKind::Indented => (String::new(), None),
                        };
                        self.code = Some(CodeBlock { highlighter: self.highlight_code.then(|| CodeHighlighter::new(&info)), content: String::new(), closing });
                    }
                    Tag::BlockQuote(_) => { self.flush(); self.quote_rows.push(self.rows.len()); self.quote += 1; self.push_style(self.style.fg(Color::DarkGray).add_modifier(Modifier::ITALIC)); }
                    Tag::List(start) => { self.flush(); self.lists.push(start); }
                    Tag::Item => {
                        self.flush();
                        let marker = match self.lists.last_mut() { Some(Some(n)) => { let marker = format!("{}. ", *n); *n += 1; marker }, _ => "- ".to_owned() };
                        self.item_prefix = Some(marker);
                    }
                    Tag::Emphasis => self.push_style(self.style.add_modifier(Modifier::ITALIC)),
                    Tag::Strong => self.push_style(self.style.fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    Tag::Strikethrough => self.push_style(self.style.add_modifier(Modifier::CROSSED_OUT)),
                    Tag::Link { dest_url, .. } => { self.links.push((dest_url.to_string(), String::new())); self.push_style(self.style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)); }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph => { if !self.inline { self.flush(); if self.quote > 0 { self.quote_gap = true; } } },
                    TagEnd::Heading(_) => { self.flush(); self.pop_style(); },
                    TagEnd::CodeBlock => {
                        if let Some(mut code) = self.code.take() {
                            for line in code.content.split_terminator('\n') {
                                if let Some(highlighter) = &mut code.highlighter { self.spans.extend(highlighter.highlight_line(line)); }
                                else { self.spans.push(Span::raw(line.to_owned())); }
                                if self.spans.is_empty() { self.spans.push(Span::raw("")); }
                                self.flush();
                            }
                            if let Some((closing, _)) = code.closing { self.spans.push(Span::styled(closing, DIM)); self.flush(); }
                        }
                    }
                    TagEnd::BlockQuote(_) => { self.flush(); if self.quote_rows.pop() == Some(self.rows.len()) { self.rows.push(Line::raw("")); } self.quote = self.quote.saturating_sub(1); self.quote_gap = false; self.pop_style(); },
                    TagEnd::List(_) => { self.flush(); self.lists.pop(); },
                    TagEnd::Item => self.flush(),
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
                    TagEnd::Link => {
                        let link_style = self.style;
                        self.pop_style();
                        if let Some((url, label)) = self.links.pop()
                            && label != url
                        {
                            self.spans.push(Span::styled(format!(" ({url})"), link_style));
                        }
                    }
                    _ => {}
                },
                Event::Text(s) if self.code.is_some() => {
                    let code = self.code.as_mut().unwrap();
                    if code.closing.as_ref().is_some_and(|(_, start)| range.start <= *start && range.end > *start) { code.closing = None; }
                    code.content.push_str(&s);
                }
                Event::Text(s) | Event::Html(s) | Event::InlineHtml(s) => {
                    if let Some((_, label)) = self.links.last_mut() { label.extend(s.chars().map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c })); }
                    for part in s.split_inclusive('\n') {
                        let has_newline = part.ends_with('\n');
                        let content = part.trim_end_matches(['\r', '\n']);
                        if !content.is_empty() { self.spans.push(Span::styled(content.to_owned(), self.style)); }
                        if has_newline {
                            if self.spans.is_empty() && self.item_prefix.is_none() { self.rows.push(Line::raw("")); }
                            else { self.flush(); }
                        }
                    }
                }
                Event::Code(s) => {
                    if let Some((_, label)) = self.links.last_mut() { label.push_str(&s); }
                    self.spans.push(Span::styled(s.to_string(), self.style.fg(Color::Yellow)));
                }
                Event::SoftBreak => { if let Some((_, label)) = self.links.last_mut() { label.push(' '); } self.spans.push(Span::styled(" ", self.style)); },
                Event::HardBreak => { if let Some((_, label)) = self.links.last_mut() { label.push(' '); } self.flush(); },
                Event::Rule => { self.flush(); self.rows.push(Line::from(Span::styled("─".repeat(self.width as usize), Style::new().fg(Color::DarkGray)))); },
                Event::TaskListMarker(checked) => self.spans.push(Span::styled(if checked { "[x] " } else { "[ ] " }, self.style)),
                _ => {}
            }
        }
    }
}
