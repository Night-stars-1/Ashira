//! Streaming reply rendering and terminal layout helpers.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

mod highlight;
mod markdown;
mod table;
pub use table::Reply;

pub const DIM: Style = Style::new().fg(Color::DarkGray);

/// Drops ANSI escape sequences and control characters; expands tabs.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                } else {
                    chars.next();
                }
            }
            '\t' => out.push_str("    "),
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub fn width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Foreground color shared by a pasted block and its card.
pub fn paste_style(id: u64) -> Style {
    const COLORS: [(u8, u8, u8); 6] = [
        (255, 179, 102),
        (125, 207, 255),
        (189, 147, 249),
        (105, 220, 158),
        (255, 141, 188),
        (240, 223, 120),
    ];
    let (r, g, b) = COLORS[((id % 6 + 2) % 6) as usize];
    Style::new().fg(Color::Rgb(r, g, b))
}

// Truncate by terminal display width, reserving one column for the ellipsis.
fn paste_clip(text: &str, max_width: usize) -> String {
    if width(text) <= max_width {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);
        if used + char_width > max_width - 1 {
            break;
        }
        result.push(ch);
        used += char_width;
    }
    result.push('…');
    result
}

fn paste_border(caption: &str, left: char, right: char, style: Style) -> Vec<Span<'static>> {
    let caption = paste_clip(caption, 10);
    let remaining = 10 - width(&caption);
    let before = remaining / 2;
    let after = remaining - before;
    vec![
        Span::styled(format!("{left}{} ", "─".repeat(before)), style),
        Span::styled(caption, style.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}{right}", "─".repeat(after)), style),
    ]
}

/// Render complete, horizontally arranged cards for pasted blocks that fit.
pub fn paste_cards<'a>(
    blocks: impl Iterator<Item = &'a crate::composer::Atom>,
    width: u16,
) -> Vec<Line<'static>> {
    if width < 14 {
        return Vec::new();
    }
    let count = (usize::from(width) + 2) / 16;
    let mut blocks = blocks.take(count).peekable();
    if blocks.peek().is_none() {
        return Vec::new();
    }
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new(); 6];
    for (drawn, block) in blocks.enumerate() {
        if drawn > 0 {
            for row in &mut rows {
                row.push(Span::raw("  "));
            }
        }
        let (id, label) = match block {
            crate::composer::Atom::Block(block) => (block.id, &block.label),
            crate::composer::Atom::Image(image) => (image.id, &image.label),
            _ => continue,
        };
        let style = paste_style(id);
        rows[0].extend(paste_border(label, '╭', '╮', style));
        for (index, row) in rows[1..5].iter_mut().enumerate() {
            row.push(Span::styled("│", style));
            match block {
                crate::composer::Atom::Block(block) => {
                    let text = paste_clip(block.preview.get(index).map(String::as_str).unwrap_or(""), 12);
                    let padding = 12 - crate::render::width(&text);
                    row.push(Span::styled(format!("{text}{}", " ".repeat(padding)), DIM));
                }
                crate::composer::Atom::Image(_) => {
                    if index == 1 {
                        row.push(Span::raw("     "));
                        row.push(Span::styled(crate::composer::IMAGE_ICON, Style::new().fg(Color::Gray)));
                        row.push(Span::raw("      "));
                    } else {
                        row.push(Span::raw("            "));
                    }
                }
                _ => unreachable!(),
            }
            row.push(Span::styled("│", style));
        }
        let caption = match block {
            crate::composer::Atom::Block(block) if block.lines == 1 => format!("{} chars", block.characters),
            crate::composer::Atom::Block(block) => format!("+{} lines", block.lines),
            crate::composer::Atom::Image(image) => format!("{}×{}", image.data.width, image.data.height),
            _ => unreachable!(),
        };
        rows[5].extend(paste_border(&caption, '╰', '╯', style));
    }
    rows.into_iter().map(Line::from).collect()
}

/// Greedy word wrap by display width; hard-breaks words longer than a row
/// (and CJK text, which has no spaces). Always returns at least one row.
pub fn wrap(spans: &[Span<'_>], width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut rows = Vec::new();
    let mut cur: Vec<(char, Style)> = Vec::new();
    let mut cur_w = 0;
    let mut last_space: Option<usize> = None;
    for span in spans {
        for ch in span.content.chars() {
            let w = ch.width().unwrap_or(0);
            if cur_w + w > width && !cur.is_empty() {
                match last_space.filter(|&i| i > 0) {
                    Some(sp) => {
                        let rest = cur.split_off(sp + 1);
                        while cur.last().is_some_and(|(c, _)| *c == ' ') {
                            cur.pop();
                        }
                        rows.push(to_line(&cur));
                        cur = rest;
                    }
                    None => {
                        rows.push(to_line(&cur));
                        cur.clear();
                    }
                }
                cur_w = cur.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
                last_space = None;
            }
            if ch == ' ' {
                last_space = Some(cur.len());
            }
            cur.push((ch, span.style));
            cur_w += w;
        }
    }
    rows.push(to_line(&cur));
    rows
}

fn to_line(cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut style = cells.first().map(|c| c.1).unwrap_or_default();
    for &(c, s) in cells {
        if s != style {
            spans.push(Span::styled(std::mem::take(&mut text), style));
            style = s;
        }
        text.push(c);
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

