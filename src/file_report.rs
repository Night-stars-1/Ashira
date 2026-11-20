//! Presentation of retained filesystem changes; all diff data comes from the tool.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    file_output::{DiffKind, FileDetail, FileOutput},
    render::{self, sanitize},
};

fn clean(text: &str) -> String { sanitize(text).replace('\n', " ") }

fn clip(text: &str, width: usize) -> String {
    if render::width(text) <= width { return text.to_owned(); }
    if width == 0 { return String::new(); }
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > width { break; }
        result.push(ch);
        used += w;
    }
    result.push('…');
    result
}

pub fn path(file: Option<&FileOutput>) -> String {
    file.map(|f| clean(&crate::tui::short_path(std::path::Path::new(&f.path))))
        .filter(|s| !s.is_empty()).unwrap_or_else(|| "(unknown path)".into())
}

pub fn read_label(file: Option<&FileOutput>) -> String {
    let mut label = path(file);
    if let Some(FileOutput { detail: FileDetail::Read { start, end, directory }, .. }) = file {
        if *directory { label.push_str(" / (directory)"); }
        else if let (Some(start), Some(end)) = (start, end) { label.push_str(&format!(":{start}-{end}")); }
    }
    label
}

const CARD_BG: Color = Color::Rgb(15, 18, 22);
const ADDED: Color = Color::Rgb(0, 255, 136);
const REMOVED: Color = Color::Rgb(255, 71, 87);
const CONTEXT: Color = Color::Rgb(156, 163, 176);
const BORDER: Color = Color::Rgb(31, 37, 45);
const TITLE: Color = Color::Rgb(0, 180, 255);

fn clipped_spans(spans: &[Span<'static>], width: usize) -> (Vec<Span<'static>>, bool) {
    let total: usize = spans.iter().map(|s| render::width(&s.content)).sum();
    if total <= width { return (spans.to_vec(), false); }
    if width == 0 { return (Vec::new(), true); }
    let mut remaining = width - 1;
    let mut result = Vec::new();
    let mut last_style = Style::new().fg(CONTEXT);
    for span in spans {
        let mut part = String::new();
        for ch in span.content.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w > remaining { break; }
            part.push(ch);
            remaining -= w;
        }
        if !part.is_empty() {
            last_style = span.style;
            let complete = part.len() == span.content.len();
            result.push(Span::styled(part, span.style));
            if !complete { break; }
        } else if !span.content.is_empty() { break; }
    }
    result.push(Span::styled("…", last_style));
    (result, true)
}

fn diff_text(row: &crate::file_output::DiffRow, color: Color) -> Vec<Span<'static>> {
    let normal = Style::new().fg(color);
    let highlighted = Style::new().fg(CARD_BG).bg(color);
    let mut spans = Vec::new();
    let mut pos = 0;
    for range in &row.highlights {
        if range.start < pos || range.end <= range.start { continue; }
        let (Some(before), Some(marked)) = (row.text.get(pos..range.start), row.text.get(range.start..range.end)) else { continue; };
        if !before.is_empty() { spans.push(Span::styled(clean(before), normal)); }
        spans.push(Span::styled(clean(marked), highlighted));
        pos = range.end;
    }
    if let Some(rest) = row.text.get(pos..) { spans.push(Span::styled(clean(rest), normal)); }
    spans
}

fn diff_card_line(
    text: &[Span<'static>], gutter: Option<&Span<'static>>, first: bool,
    inner: usize, bg: Style, border: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled("│", border)];
    let mut used = 0;
    if let Some(gutter) = gutter {
        used = render::width(&gutter.content);
        if first {
            spans.push(Span::styled(gutter.content.clone(), bg.patch(gutter.style)));
        } else {
            spans.push(Span::styled(" ".repeat(used.saturating_sub(1)), bg.patch(gutter.style)));
            spans.push(Span::styled("│", bg.patch(gutter.style)));
        }
    }
    for span in text {
        used += render::width(&span.content);
        spans.push(Span::styled(span.content.clone(), bg.patch(span.style)));
    }
    spans.push(Span::styled(" ".repeat(inner.saturating_sub(used)), bg));
    spans.push(Span::styled("│", border));
    Line::from(spans)
}

// Stop as soon as a thirteenth physical body line proves the preview overflows.
fn append_row(
    result: &mut Vec<Line<'static>>, body_start: usize, expanded: bool,
    gutter: Option<Span<'static>>, mut text: Vec<Span<'static>>,
    inner: usize, framed: bool,
) -> bool {
    const BG: Style = Style::new().bg(CARD_BG);
    const EDGE: Style = BG.fg(BORDER);
    let gutter = if gutter.as_ref().map_or(0, |s| render::width(&s.content)) + 2 <= inner {
        gutter
    } else {
        if let Some(gutter) = gutter { text.insert(0, gutter); }
        None
    };
    let available = inner - gutter.as_ref().map_or(0, |s| render::width(&s.content));
    let mut parts = Vec::new();
    let mut used = 0;
    let mut first = true;
    for span in text {
        let mut part = String::new();
        for ch in span.content.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            // A double-width glyph cannot fit in a one-cell terminal.
            let (ch, w) = if w > available { ('…', 1) } else { (ch, w) };
            if used + w > available {
                if !part.is_empty() { parts.push(Span::styled(std::mem::take(&mut part), span.style)); }
                if !expanded && result.len() - body_start == 12 { return false; }
                if framed { result.push(diff_card_line(&parts, gutter.as_ref(), first, inner, BG, EDGE)); }
                else { result.push(Line::from(std::mem::take(&mut parts))); }
                first = false;
                parts.clear();
                used = 0;
            }
            part.push(ch);
            used += w;
        }
        if !part.is_empty() { parts.push(Span::styled(part, span.style)); }
    }
    if !expanded && result.len() - body_start == 12 { return false; }
    if framed { result.push(diff_card_line(&parts, gutter.as_ref(), first, inner, BG, EDGE)); }
    else { result.push(Line::from(parts)); }
    true
}

/// Render the retained snapshot in place, without a separate report.
pub fn change(name: &str, file: Option<&FileOutput>, output: &str, is_error: bool, width: u16, expanded: bool) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 { return Vec::new(); }
    let change = if !is_error {
        match file.map(|f| &f.detail) { Some(FileDetail::Change(change)) => Some(change), _ => None }
    } else { None };
    let counts = change.and_then(|c| c.added.zip(c.removed));
    let context = Style::new().fg(CONTEXT);
    let title_style = Style::new().fg(TITLE).add_modifier(Modifier::BOLD);
    let heading = vec![Span::styled(format!("✎ {name}: "), title_style), Span::styled(path(file), title_style)];
    let stats = counts.map(|(added, removed)| vec![
        Span::styled(" ⟨", context), Span::styled(format!("+{added}"), Style::new().fg(ADDED)),
        Span::styled("/", context), Span::styled(format!("-{removed}"), Style::new().fg(REMOVED)),
        Span::styled("⟩", context),
    ]).unwrap_or_default();
    let framed = width >= 5;
    let inner = if framed { width - 2 } else { width };
    let bg = Style::new().bg(CARD_BG);
    let border = bg.fg(BORDER);
    let mut result = Vec::new();
    let mut title_truncated = false;
    if framed {
        let title_space = inner.saturating_sub(4);
        let stats_width: usize = stats.iter().map(|s| render::width(&s.content)).sum();
        let show_stats = stats_width <= title_space;
        let (mut title, truncated) = clipped_spans(&heading, title_space.saturating_sub(if show_stats { stats_width } else { 0 }));
        title_truncated = truncated || (!show_stats && !stats.is_empty());
        if show_stats { title.extend(stats.iter().cloned()); }
        let title_width: usize = title.iter().map(|s| render::width(&s.content)).sum();
        let mut top = vec![Span::styled("╭", border), Span::styled("─".repeat(inner.min(3)), border)];
        if inner > 3 { top.push(Span::styled(" ", border)); }
        top.extend(title.into_iter().map(|s| Span::styled(s.content, bg.patch(s.style))));
        top.push(Span::styled("─".repeat(title_space.saturating_sub(title_width)), border));
        top.push(Span::styled("╮", border));
        result.push(Line::from(top));
    } else if !expanded {
        let (title, _) = clipped_spans(&heading, width);
        result.push(Line::from(title));
    }
    let body_start = result.len();
    let mut overflow = false;
    'body: {
        if expanded && (!framed || title_truncated) {
            let mut full_title = heading;
            full_title.extend(stats);
            if !append_row(&mut result, body_start, expanded, None, full_title, inner, framed) { overflow = true; break 'body; }
        }
        if is_error {
            for (index, line) in output.split('\n').enumerate() {
                let line = sanitize(line);
                let text = if index == 0 { format!("Error: {line}") } else { line };
                if !append_row(&mut result, body_start, expanded, None, vec![Span::styled(text, Style::new().fg(REMOVED))], inner, framed) { overflow = true; break 'body; }
            }
        } else if let Some(change) = change {
            let digits = change.rows.iter().filter_map(|r| r.old.into_iter().chain(r.new).max()).max()
                .map(|n| n.to_string().len()).unwrap_or(1).max(3);
            let mut seen_hunk = false;
            for row in &change.rows {
                if matches!(&row.kind, DiffKind::Hunk) {
                    if expanded || seen_hunk {
                        let text = if expanded { clean(&row.text) } else { "⋯".to_owned() };
                        if !append_row(&mut result, body_start, expanded, None, vec![Span::styled(text, context)], inner, framed) { overflow = true; break 'body; }
                    }
                    seen_hunk = true;
                    continue;
                }
                let (mark, color, number) = match &row.kind {
                    DiffKind::Added => ('+', ADDED, row.new),
                    DiffKind::Removed => ('-', REMOVED, row.old),
                    DiffKind::Context => (' ', CONTEXT, row.old.or(row.new)),
                    DiffKind::NoNewline => (' ', CONTEXT, None),
                    DiffKind::Hunk => unreachable!(),
                };
                let number = number.map(|n| format!("{mark}{n}")).unwrap_or_default();
                let gutter = Span::styled(format!("{number:>width$}│", width = digits + 1), Style::new().fg(color));
                if !append_row(&mut result, body_start, expanded, Some(gutter), diff_text(row, color), inner, framed) { overflow = true; break 'body; }
            }
            let mut notices = Vec::new();
            if change.rows.is_empty() && change.notice.is_none() {
                notices.push(if counts.is_some() { "No textual changes".to_owned() } else { "Diff unavailable".to_owned() });
            }
            if change.omitted_rows > 0 { notices.push(format!("{} diff rows omitted from retained output", change.omitted_rows)); }
            if let Some(notice) = &change.notice { notices.push(notice.clone()); }
            if let Some(path) = &change.full_diff_path { notices.push(format!("Full diff: {}", clean(&path.display().to_string()))); }
            for notice in notices {
                for line in notice.split('\n') {
                    if !append_row(&mut result, body_start, expanded, None, vec![Span::styled(sanitize(line), context)], inner, framed) { overflow = true; break 'body; }
                }
            }
        } else {
            let message = if matches!(file.map(|f| &f.detail), Some(FileDetail::Summary)) { "历史记录未保存差异快照" } else { "Diff unavailable" };
            if !append_row(&mut result, body_start, expanded, None, vec![Span::styled(message, context)], inner, framed) { overflow = true; break 'body; }
            if !output.is_empty() {
                for line in output.split('\n') {
                    if !append_row(&mut result, body_start, expanded, None, vec![Span::styled(sanitize(line), context)], inner, framed) { overflow = true; break 'body; }
                }
            }
        }
    }
    if overflow {
        let hint = clip("… Ctrl+O: Expand", inner);
        if framed { result.push(diff_card_line(&[Span::styled(hint, context)], None, true, inner, bg, border)); }
        else { result.push(Line::from(Span::styled(hint, context))); }
    }
    if framed { result.push(Line::from(vec![Span::styled(format!("╰{}╯", "─".repeat(inner)), border)])); }
    result
}
