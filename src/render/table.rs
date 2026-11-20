use super::{highlight::CodeHighlighter, markdown, sanitize, width, wrap, DIM};
use ratatui::{style::{Color, Modifier, Style}, text::{Line, Span}};

#[derive(Clone, Copy)]
enum Align { Left, Center, Right }

type Cell = Vec<Span<'static>>;
type Row = Vec<Cell>;

struct Table {
    header: Row,
    align: Vec<Align>,
    rows: Vec<Row>,
    natural: Vec<usize>,
}

struct Fence { marker: char, length: usize, highlighter: CodeHighlighter }

#[derive(Default)]
pub struct Reply {
    pending: Vec<String>,
    fence: Option<Fence>,
    table: Option<Table>,
    revision: u64,
    cache_key: Option<(u64, String, u16, usize)>,
    cache: Vec<Line<'static>>,
}

impl Reply {
    fn flush_pending(&mut self, width: u16) -> Vec<Line<'static>> {
        if self.pending.is_empty() { return Vec::new(); }
        let text = self.pending.join("\n");
        self.pending.clear();
        markdown::render(&text, width)
    }
    fn flush_completed_blocks(&mut self, width: u16) -> Vec<Line<'static>> {
        let text = self.pending.join("\n");
        let Some(start) = markdown::last_block_start(&text).filter(|start| *start > 0) else { return Vec::new(); };
        let prefix = &text[..start];
        let mut out = markdown::render(prefix, width);
        if prefix.ends_with("\n\n") || (prefix.trim().is_empty() && prefix.contains('\n')) { out.push(Line::raw("")); }
        self.pending = text[start..].split('\n').map(str::to_owned).collect();
        out
    }
    pub fn push(&mut self, raw: &str, width: u16) -> Vec<Line<'static>> {
        self.revision = self.revision.wrapping_add(1);
        let width = width.max(1);
        let raw = sanitize(raw);
        if let Some(fence) = &mut self.fence {
            if is_close(&raw, fence.marker, fence.length) {
                self.fence = None;
                return wrap(&[Span::styled(raw, DIM)], width);
            }
            return wrap(&fence.highlighter.highlight_line(&raw), width);
        }
        let mut out = Vec::new();
        if let Some(table) = &mut self.table {
            if fence_open(&raw).is_none() && let Some(cells) = split(&raw) { table.add(cells); return out; }
            out.extend(self.table.take().unwrap().render(width));
        }
        if let Some(header) = self.pending.last().and_then(|line| split(line))
            && let Some(align) = delimiters(&raw, header.len())
        {
                let text = self.pending.join("\n");
                let start = text.len().saturating_sub(self.pending.last().unwrap().len());
                if markdown::top_level_paragraph_at(&text, start) {
                    self.pending.pop();
                    out.extend(self.flush_pending(width));
                    self.table = Some(Table::new(header, align));
                    return out;
                }
        }
        if let Some((marker, length, info)) = fence_open(&raw) {
            let mut text = self.pending.join("\n");
            if !self.pending.is_empty() { text.push('\n'); }
            let offset = text.len();
            text.push_str(&raw);
            if markdown::top_level_fence_at(&text, offset) {
                out.extend(self.flush_pending(width));
                self.fence = Some(Fence { marker, length, highlighter: CodeHighlighter::new(info) });
                out.extend(wrap(&[Span::styled(raw, DIM)], width));
                return out;
            }
        }
        self.pending.push(raw);
        out.extend(self.flush_completed_blocks(width));
        out
    }
    pub fn finish(&mut self, width: u16) -> Vec<Line<'static>> {
        self.revision = self.revision.wrapping_add(1);
        let width = width.max(1);
        let mut out = self.flush_pending(width);
        if let Some(table) = self.table.take() { out.extend(table.render(width)); }
        self.fence = None;
        out
    }
    pub fn preview(&mut self, partial: &str, width: u16, max_rows: usize) -> &[Line<'static>] {
        let width = width.max(1);
        if self.cache_key.as_ref().is_some_and(|key| key.0 == self.revision && key.1 == partial && key.2 == width && key.3 == max_rows) { return &self.cache; }
        let mut lines = Vec::new();
        if max_rows > 0 {
            let partial = sanitize(partial);
            if let Some(fence) = &self.fence {
                if !partial.is_empty() {
                    if is_close(&partial, fence.marker, fence.length) { lines.extend(wrap(&[Span::styled(partial.clone(), DIM)], width)); }
                    else { lines.extend(wrap(&fence.highlighter.preview_line(&partial), width)); }
                }
            } else if let Some(table) = &self.table {
                let extra = if partial.is_empty() || fence_open(&partial).is_some() { None } else { split(&partial) };
                let plain_tail = !partial.is_empty() && extra.is_none();
                lines = table.render_preview(width, max_rows, extra);
                if plain_tail { lines.extend(markdown::render_preview(&partial, width)); }
            } else {
                let mut text = self.pending.join("\n");
                if !partial.is_empty() { if !self.pending.is_empty() { text.push('\n'); } text.push_str(&partial); }
                if !text.is_empty() { lines = markdown::render_preview(&text, width); }
            }
            if lines.len() > max_rows { lines.drain(..lines.len() - max_rows); }
        }
        self.cache = lines;
        self.cache_key = Some((self.revision, partial.to_owned(), width, max_rows));
        &self.cache
    }
}

fn fence_open(raw: &str) -> Option<(char, usize, &str)> {
    let trimmed = raw.trim_start_matches(' ');
    if raw.len() - trimmed.len() > 3 { return None; }
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' { return None; }
    let length = trimmed.bytes().take_while(|b| *b == marker as u8).count();
    if length < 3 { return None; }
    let info = trimmed[length..].trim();
    if marker == '`' && info.contains('`') { return None; }
    Some((marker, length, info))
}
fn is_close(raw: &str, marker: char, length: usize) -> bool {
    let trimmed = raw.trim_start_matches(' ');
    if raw.len() - trimmed.len() > 3 { return false; }
    let count = trimmed.bytes().take_while(|b| *b == marker as u8).count();
    count >= length && trimmed[count..].trim().is_empty()
}

// A backtick run hides separators only when a matching run closes it.
fn split(raw: &str) -> Option<Vec<String>> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = raw.char_indices().peekable();
    let mut slashes = 0usize;
    let mut closing = None;
    let mut separators = 0;
    while let Some((i, ch)) = chars.next() {
        if ch == '`' && (closing.is_some() || slashes.is_multiple_of(2)) {
            let mut count = 1;
            while chars.peek().is_some_and(|(_, c)| *c == '`') { chars.next(); count += 1; }
            if closing == Some(i) { closing = None; }
            else if closing.is_none() {
                let rest = &raw[i + count..];
                let mut runs = rest.char_indices().peekable();
                while let Some((j, c)) = runs.next() {
                    if c != '`' { continue; }
                    let mut n = 1;
                    while runs.peek().is_some_and(|(_, c)| *c == '`') { runs.next(); n += 1; }
                    if n == count { closing = Some(i + count + j); break; }
                }
            }
            for _ in 0..count { cell.push('`'); }
            slashes = 0;
            continue;
        }
        if ch == '|' {
            if !slashes.is_multiple_of(2) { cell.pop(); cell.push('|'); }
            else if closing.is_none() { cells.push(std::mem::take(&mut cell)); separators += 1; }
            else { cell.push('|'); }
        } else { cell.push(ch); }
        if ch == '\\' { slashes += 1; } else { slashes = 0; }
    }
    if separators == 0 { return None; }
    cells.push(cell);
    if raw.trim_start().starts_with('|') { cells.remove(0); }
    if raw.trim_end().ends_with('|') && cells.last().is_some_and(|s| s.trim().is_empty()) { cells.pop(); }
    Some(cells.into_iter().map(|s| s.trim().to_owned()).collect())
}

fn delimiters(raw: &str, count: usize) -> Option<Vec<Align>> {
    let cells = split(raw)?;
    if cells.len() != count || count == 0 { return None; }
    cells.iter().map(|c| {
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        let core = c.trim_matches(':');
        if core.is_empty() || !core.bytes().all(|b| b == b'-') || c.len() != core.len() + left as usize + right as usize { return None; }
        Some(match (left, right) { (true, true) => Align::Center, (false, true) => Align::Right, _ => Align::Left })
    }).collect()
}

fn styled(text: &str, header: bool) -> Cell {
    let base = if header { Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::new() };
    markdown::inline(text, base)
}
fn cell_width(cell: &Cell) -> usize { cell.iter().map(|s| width(&s.content)).sum() }

impl Table {
    fn new(cells: Vec<String>, align: Vec<Align>) -> Self {
        let header: Row = cells.iter().map(|c| styled(c, true)).collect();
        let natural = header.iter().map(|c| cell_width(c).max(2)).collect();
        Self { header, align, rows: Vec::new(), natural }
    }
    fn make_row(&self, cells: Vec<String>) -> Row {
        (0..self.header.len()).map(|i| styled(cells.get(i).map_or("", String::as_str), false)).collect()
    }
    fn add(&mut self, cells: Vec<String>) {
        let row = self.make_row(cells);
        for (i, cell) in row.iter().enumerate() { self.natural[i] = self.natural[i].max(cell_width(cell)); }
        self.rows.push(row);
    }
    fn widths(&self, width: u16, extra: Option<&Row>) -> Option<Vec<usize>> {
        let n = self.header.len();
        let budget = (width as usize).checked_sub(3 * n + 1)?;
        if budget < 2 * n { return None; }
        let mut natural = self.natural.clone();
        if let Some(row) = extra {
            for (i, cell) in row.iter().enumerate() {
                natural[i] = natural[i].max(cell_width(cell));
            }
        }
        if natural.iter().sum::<usize>() <= budget {
            return Some(natural);
        }
        // Allocate at most the terminal's column budget, not one iteration per
        // character removed from an arbitrarily long cell.
        let mut widths = vec![2; n];
        let mut remaining = budget - 2 * n;
        while remaining > 0 {
            for (column, wanted) in widths.iter_mut().zip(&natural) {
                if *column < *wanted {
                    *column += 1;
                    remaining -= 1;
                    if remaining == 0 { break; }
                }
            }
        }
        Some(widths)
    }
    fn render(&self, width: u16) -> Vec<Line<'static>> {
        self.layout(width, 0, None, false)
    }
    fn render_preview(&self, width: u16, max_rows: usize, extra: Option<Vec<String>>) -> Vec<Line<'static>> {
        let start = self.rows.len().saturating_sub(max_rows);
        let extra = extra.map(|c| self.make_row(c));
        self.layout(width, start, extra.as_ref(), start > 0)
    }
    fn layout(&self, width: u16, start: usize, extra: Option<&Row>, omitted: bool) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        if let Some(widths) = self.widths(width, extra) {
            out.push(border(&widths, '┌', '┬', '┐'));
            out.extend(grid_row(&self.header, &widths, &self.align));
            out.push(border(&widths, '├', '┼', '┤'));
            if omitted { out.extend(wrap(&[Span::styled("… 仅预览末尾行", DIM)], width)); }
            let mut any = false;
            for row in self.rows[start..].iter().chain(extra) {
                if any { out.push(border(&widths, '├', '┼', '┤')); }
                any = true;
                out.extend(grid_row(row, &widths, &self.align));
            }
            out.push(border(&widths, '└', '┴', '┘'));
        } else {
            if omitted { out.extend(wrap(&[Span::styled("… 仅预览末尾行", DIM)], width)); }
            let mut any = false;
            for row in self.rows[start..].iter().chain(extra) {
                if any { out.extend(wrap(&[Span::styled("─", DIM)], width)); }
                any = true;
                for (header, cell) in self.header.iter().zip(row) {
                    let mut spans = header.clone();
                    spans.push(Span::styled(": ", DIM));
                    spans.extend(cell.iter().cloned());
                    out.extend(wrap(&spans, width));
                }
            }
            if !any {
                for header in &self.header { out.extend(wrap(header, width)); }
            }
        }
        out
    }
}

fn border(widths: &[usize], left: char, middle: char, right: char) -> Line<'static> {
    let mut text = String::new(); text.push(left);
    for (i, w) in widths.iter().enumerate() {
        text.push_str(&"─".repeat(w + 2));
        text.push(if i + 1 == widths.len() { right } else { middle });
    }
    Line::from(Span::raw(text))
}
fn grid_row(row: &Row, widths: &[usize], align: &[Align]) -> Vec<Line<'static>> {
    let wrapped: Vec<_> = row.iter().zip(widths).map(|(cell, w)| wrap(cell, (*w).min(u16::MAX as usize) as u16)).collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    (0..height).map(|y| {
        let mut spans = vec![Span::raw("│")];
        for (i, lines) in wrapped.iter().enumerate() {
            let line = lines.get(y);
            let used = line.map_or(0, |l| l.spans.iter().map(|s| width(&s.content)).sum());
            let space = widths[i].saturating_sub(used);
            let left = match align[i] { Align::Left => 0, Align::Right => space, Align::Center => space / 2 };
            spans.push(Span::raw(" ".repeat(1 + left)));
            if let Some(line) = line { spans.extend(line.spans.iter().cloned()); }
            spans.push(Span::raw(" ".repeat(1 + space - left)));
            spans.push(Span::raw("│"));
        }
        Line::from(spans)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> String {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn mixed_streaming_reply_table_flow() {
        let mut reply = Reply::default();
        let mut output = Vec::new();
        for line in include_str!("../../render-test.md").lines() {
            let preview = text(reply.preview(line, 80, 30));
            assert_eq!(preview, text(reply.preview(line, 80, 30)));
            let _ = reply.preview(line, 24, 12);
            output.extend(reply.push(line, 80));
        }
        output.extend(reply.finish(80));
        let rendered = text(&output);
        for expected in ["渲染测试", "这是粗体", "这是斜体", "这是删除线", "嵌套项目 A", "引用中的", "fn main()", "思考级别", "示例链接", "https://example.com", "渲染测试文本结束"] {
            assert!(rendered.contains(expected), "missing {expected}");
        }
        assert!(!rendered.contains("**这是粗体**"));
        assert_eq!(rendered.matches("fn main()").count(), 1);
        assert!(output.iter().all(|line| line.spans.iter().map(|s| width(&s.content)).sum::<usize>() <= 80));
        let spans: Vec<_> = output.iter().flat_map(|line| &line.spans).collect();
        assert!(spans.iter().any(|s| s.content.contains("这是粗体") && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(spans.iter().any(|s| s.content.contains("这是斜体") && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(spans.iter().any(|s| s.content.contains("这是删除线") && s.style.add_modifier.contains(Modifier::CROSSED_OUT)));
        assert!(spans.iter().any(|s| s.content.contains("示例链接") && s.style.add_modifier.contains(Modifier::UNDERLINED)));
        assert!(spans.iter().any(|s| s.content.contains("引用中的") && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(spans.iter().any(|s| s.content.contains("English") && !s.style.add_modifier.contains(Modifier::BOLD)));

        let mut rows = Vec::new();
        for line in ["**across", "lines** and \\*literal\\*", "", "hard first  ", "hard second", "", "- first", "", "  continuation", "", "  ```rust", "  let literal = \"**code**\";", "  ```", "", "| Name | Value | Empty |", "| :--- | ---: | --- |", r"| 中文 | a\|b |  |", "| next | `x|y` |  |"] {
            rows.extend(reply.push(line, 30));
        }
        let first = text(reply.preview("", 30, 30));
        assert_eq!(first, text(reply.preview("", 30, 30)));
        assert!(text(reply.preview("", 8, 40)).contains("a|b"));
        for line in ["", "<div>", "html literal", "</div>", "", "    indented **literal**"] { rows.extend(reply.push(line, 30)); }
        rows.extend(reply.finish(30));
        let joined = text(&rows);
        assert!(joined.contains("across lines") && joined.contains("*literal*"));
        assert_eq!(joined.matches("a|b").count(), 1);
        assert_eq!(joined.matches("x|y").count(), 1);
        assert!(rows.iter().flat_map(|l| &l.spans).any(|s| s.content.contains("across") && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(joined.contains("hard first\nhard second"));
        assert!(joined.contains("continuation") && joined.contains("**code**"));
        assert_eq!(joined.matches("**code**").count(), 1);
        assert!(!rows.iter().flat_map(|line| &line.spans).any(|span| span.content == "x"));
        assert!(rows.iter().flat_map(|line| &line.spans).all(|span| !span.content.contains(['\r', '\n'])));
        assert!(joined.contains("html literal") && joined.contains("indented **literal**"), "{joined}");

        reply.push("```rust", 30);
        for _ in 0..100 {
            let _ = reply.preview("let x", 20, 5);
            let committed = reply.push("let x = 1;", 30);
            assert_eq!(text(&committed).matches("let x = 1;").count(), 1);
        }
        reply.finish(30);
        let mut reset = reply.push("plain after fence", 30);
        reset.extend(reply.finish(30));
        assert_eq!(text(&reset).matches("plain after fence").count(), 1);
        assert!(reply.finish(30).is_empty());

        let mut quoted = reply.push("> ~~~text", 40);
        for _ in 0..40 { quoted.extend(reply.push("> **literal**", 40)); }
        let preview = reply.preview("> **last**", 40, 5).to_vec();
        assert!(text(&preview).contains("**last**"));
        assert!(preview.iter().flat_map(|line| &line.spans).any(|span| span.content.contains("**last**") && !span.style.add_modifier.contains(Modifier::BOLD)));
        assert_eq!(text(&preview), text(reply.preview("> **last**", 40, 5)));
        quoted.extend(reply.push("> hello~~~", 40));
        quoted.extend(reply.finish(40));
        let unclosed = text(&quoted);
        assert_eq!(unclosed.matches("**literal**").count(), 40);
        assert_eq!(unclosed.matches("hello~~~").count(), 1);
        assert_eq!(unclosed.matches("~~~").count(), 2);

        let mut closed = Vec::new();
        for line in ["> ~~~text", "> hello~~~", "> ~~~"] { closed.extend(reply.push(line, 40)); }
        closed.extend(reply.finish(40));
        assert_eq!(text(&closed).matches("~~~").count(), 3);
        let mut empty = Vec::new();
        for line in ["> ~~~text", "> ~~~"] { empty.extend(reply.push(line, 40)); }
        empty.extend(reply.finish(40));
        assert_eq!(text(&empty).matches("~~~").count(), 2);
    }
}
