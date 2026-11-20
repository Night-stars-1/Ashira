//! Mutable tail of the inline conversation. Archived rows belong to the terminal.
use std::collections::VecDeque;
use std::sync::Arc;
use ratatui::{style::{Color, Style}, text::{Line, Span}};
use unicode_width::UnicodeWidthChar;
use crate::{file_output::FileOutput, file_report, render::DIM};

enum Content {
    Static(Vec<Line<'static>>),
    Tool { collapsed: Vec<Line<'static>>, expanded: Vec<Line<'static>> },
    Output { text: String, error: bool },
    File { name: &'static str, file: Option<FileOutput>, output: String, error: bool },
    Todo { snapshot: Arc<crate::todo::Snapshot>, turn: u64 },
}

struct Block {
    content: Content,
    layout: Vec<Line<'static>>,
    key: Option<(u16, bool)>,
    consumed: usize,
    archived: String,
    reflowed: bool,
}

#[derive(Default)]
pub struct Transcript {
    blocks: VecDeque<Block>,
    expanded: bool,
    todo_turn: u64,
}

fn fingerprint(row: &Line<'_>) -> String {
    row.spans.iter().flat_map(|s| s.content.chars()).filter(|c| c.is_alphanumeric()).collect()
}

// Hard wrapping preserves styling and spaces; old markdown is never reparsed.
pub(crate) fn hard_wrap(rows: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if width == 0 { return out; }
    let width = usize::from(width);
    for row in rows {
        let mut spans = Vec::new();
        let mut used = 0;
        for span in &row.spans {
            let mut part = String::new();
            for ch in span.content.chars() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                let (ch, w) = if w > width { ('…', 1) } else { (ch, w) };
                if ch == '\n' || (used > 0 && used + w > width) {
                    if !part.is_empty() { spans.push(Span::styled(std::mem::take(&mut part), row.style.patch(span.style))); }
                    out.push(Line::from(std::mem::take(&mut spans)).style(row.style));
                    used = 0;
                    if ch == '\n' { continue; }
                }
                part.push(ch);
                used += w;
            }
            if !part.is_empty() { spans.push(Span::styled(part, row.style.patch(span.style))); }
        }
        out.push(Line::from(spans).style(row.style));
    }
    out
}

fn output_rows(text: &str, error: bool, width: u16, expanded: bool) -> Vec<Line<'static>> {
    let style = if error { Style::new().fg(Color::Red) } else { DIM };
    let mut rows = Vec::new();
    let mut clipped = false;
    let mut count: usize = 0;
    for (index, line) in text.lines().enumerate() {
        count += 1;
        if !expanded && index >= 4 { continue; }
        let prefix = if index == 0 { "  └ " } else { "    " };
        let body = if expanded { line.to_owned() } else {
            let available = usize::from(width).saturating_sub(4);
            let mut used = 0;
            let mut end = 0;
            for (pos, ch) in line.char_indices() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                if used + w > available { clipped = true; break; }
                used += w;
                end = pos + ch.len_utf8();
            }
            line[..end].to_owned()
        };
        rows.push(Line::from(vec![Span::styled(prefix, DIM), Span::styled(body, style)]));
    }
    if rows.is_empty() { rows.push(Line::styled("  └ （无输出）", DIM)); }
    if expanded { rows.push(Line::styled("    Ctrl+O: Collapse", DIM)); }
    else if count > 4 || clipped { rows.push(Line::styled(format!("    … ({} more lines) (Ctrl+O: Expand)", count.saturating_sub(4)), DIM)); }
    hard_wrap(&rows, width)
}

impl Block {
    fn new(content: Content) -> Self { Self { content, layout: Vec::new(), key: None, consumed: 0, archived: String::new(), reflowed: false } }
    fn layout(&mut self, width: u16, expanded: bool) {
        let expanded = expanded && !matches!(self.content, Content::Static(_));
        if self.key == Some((width, expanded)) { return; }
        let layout = match &self.content {
            Content::Static(rows) => hard_wrap(rows, width),
            Content::Tool { collapsed, expanded: full } => hard_wrap(if expanded { full } else { collapsed }, width),
            Content::Output { text, error } => output_rows(text, *error, width, expanded),
            Content::File { name, file, output, error } => file_report::change(name, file.as_ref(), output, *error, width, expanded),
            Content::Todo { snapshot, .. } => crate::todo_render::rows(snapshot, width, expanded),
        };
        // Equal physical rows are proof even when the width changes. In
        // particular, an archived leading blank must not restart a whole Read.
        let identical = self.layout.iter().take(self.consumed).zip(&layout)
            .take_while(|(old, new)| old == new).count();
        let mut matched: usize = self.layout[..identical].iter().map(|row| fingerprint(row).len()).sum();
        self.consumed = identical;
        for row in &layout[identical..] {
            let part = fingerprint(row);
            // Unknown empty or punctuation-only rows cannot prove a seam.
            if part.is_empty() || !self.archived.get(matched..).is_some_and(|rest| rest.starts_with(&part)) { break; }
            matched += part.len();
            self.consumed += 1;
        }
        self.reflowed = true;
        self.layout = layout;
        // Native history is immutable. If a reflow cannot prove a complete-row
        // seam, keep the uncertain row rather than discarding unseen content.
        self.key = Some((width, expanded));
    }
}

impl Transcript {
    pub fn push_static(&mut self, rows: Vec<Line<'static>>) { if !rows.is_empty() { self.blocks.push_back(Block::new(Content::Static(rows))); } }
    pub fn push_tool(&mut self, collapsed: Vec<Line<'static>>, expanded: Vec<Line<'static>>) { self.blocks.push_back(Block::new(Content::Tool { collapsed, expanded })); }
    pub fn push_output(&mut self, text: String, error: bool) { self.blocks.push_back(Block::new(Content::Output { text, error })); }
    pub fn push_file(&mut self, name: &'static str, file: Option<FileOutput>, output: String, error: bool) { self.blocks.push_back(Block::new(Content::File { name, file, output, error })); }
    pub fn push_todo(&mut self, snapshot: Arc<crate::todo::Snapshot>) {
        if let Some(index) = self.blocks.iter().rposition(|block| matches!(&block.content, Content::Todo { turn, .. } if *turn == self.todo_turn))
            && self.blocks[index].consumed == 0 && self.blocks[index].archived.is_empty()
        { self.blocks.remove(index); }
        self.blocks.push_back(Block::new(Content::Todo { snapshot, turn: self.todo_turn }));
    }
    pub fn seal_todo(&mut self) { self.todo_turn = self.todo_turn.wrapping_add(1); }
    pub fn toggle(&mut self) { self.expanded = !self.expanded; }
    pub fn clear(&mut self) { self.blocks.clear(); self.expanded = false; self.todo_turn = 0; }
    pub fn visible(&mut self, width: u16, budget: usize, pending: &mut Vec<Line<'static>>) -> Vec<Line<'static>> {
        if width == 0 { return Vec::new(); }
        let mut total = 0;
        for block in &mut self.blocks { block.layout(width, self.expanded); total += block.layout.len().saturating_sub(block.consumed); }
        while self.blocks.front().is_some_and(|block| block.consumed == block.layout.len()) {
            self.blocks.pop_front();
        }
        let mut excess = total.saturating_sub(budget);
        while excess > 0 {
            let Some(block) = self.blocks.front_mut() else { break };
            let remaining = block.layout.len().saturating_sub(block.consumed);
            if remaining <= excess {
                let block = self.blocks.pop_front().expect("front exists");
                pending.extend(block.layout.into_iter().skip(block.consumed));
                excess -= remaining;
                continue;
            }
            let end = block.consumed + excess;
            if block.reflowed {
                block.archived = block.layout[..block.consumed].iter().map(fingerprint).collect();
                block.reflowed = false;
            }
            for row in &block.layout[block.consumed..end] { block.archived.push_str(&fingerprint(row)); }
            pending.extend(block.layout[block.consumed..end].iter().cloned());
            block.consumed = end;
            excess = 0;
        }
        self.blocks.iter().flat_map(|block| block.layout[block.consumed..].iter().cloned()).collect()
    }
    pub fn flush(&mut self, width: u16, pending: &mut Vec<Line<'static>>) { let _ = self.visible(width, 0, pending); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(rows: &[Line<'_>]) -> String { rows.iter().map(|r| r.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("|") }
    #[test]
    fn transcript_lifecycle_preserves_order_and_archived_history() {
        let mut t = Transcript::default();
        let mut history = Vec::new();
        t.push_static(vec![Line::raw("before")]);
        t.push_tool(vec![Line::raw("tool"), Line::raw("short")], vec![Line::raw("tool"), Line::raw("long output")]);
        t.push_static(vec![Line::raw("after")]);
        assert_eq!(text(&t.visible(40, 20, &mut history)), "before|tool|short|after");
        t.toggle();
        assert_eq!(text(&t.visible(40, 20, &mut history)), "before|tool|long output|after");
        t.toggle();
        assert_eq!(text(&t.visible(40, 20, &mut history)), "before|tool|short|after");
        let _ = t.visible(40, 2, &mut history);
        let archived = text(&history);
        t.toggle();
        let live = t.visible(6, 20, &mut history);
        assert_eq!(text(&history), archived);
        assert!(text(&live).replace('|', "").contains("after"));
        t.push_static(vec![Line::raw("continued")]);
        t.flush(6, &mut history);
        assert!(text(&history).replace('|', "").contains("continued"));
        t.clear();
        t.push_output("alpha\n{}[];\n\nomega".into(), false);
        t.toggle();
        let _ = t.visible(40, 4, &mut history);
        let archived = text(&history);
        t.toggle();
        let live = t.visible(12, 20, &mut history);
        assert_eq!(text(&history), archived);
        let displayed = text(&live);
        assert!(displayed.contains("{}[];"));
        assert!(displayed.contains("{}[];|    |    omega"));
        t.toggle();
        let live = t.visible(10, 20, &mut history);
        assert!(text(&live).contains("{}[];"));
        t.flush(10, &mut history);
        assert!(text(&history).contains("{}[];"));
        t.clear();
        let begin = history.len();
        let mut read_rows = vec![Line::raw(""), Line::raw("Read source.txt")];
        read_rows.extend((0..12).map(|index| Line::raw(format!("saved row {index:02}"))));
        t.push_tool(vec![Line::raw("Read source.txt")], read_rows.clone());
        t.toggle();
        let _ = t.visible(40, 4, &mut history);
        let committed = history.len();
        let live = t.visible(20, 4, &mut history);
        assert_eq!(history.len(), committed, "resizing a Read must not emit its archived prefix again");
        assert_eq!(text(&live), "saved row 08|saved row 09|saved row 10|saved row 11");
        t.flush(20, &mut history);
        assert_eq!(text(&history[begin..]), text(&read_rows));
        t.clear();
        assert!(t.visible(40, 20, &mut history).is_empty());
        let snapshot = |content: &str| Arc::new(serde_json::from_value::<crate::todo::Snapshot>(serde_json::json!({
            "op": "init", "state": {"phases": [{"name": "阶段", "tasks": [{"content": content, "status": "in_progress"}]}]}, "touched": [content]
        })).unwrap());
        t.push_todo(snapshot("old task"));
        t.push_static(vec![Line::raw("between")]);
        t.push_todo(snapshot("new task"));
        let live = text(&t.visible(40, 30, &mut history));
        assert!(!live.contains("old task"));
        assert!(live.find("between").unwrap() < live.find("new task").unwrap());
        t.seal_todo();
        t.push_static(vec![Line::raw("new user")]);
        t.push_todo(snapshot("next task"));
        let live = text(&t.visible(40, 30, &mut history));
        assert!(live.find("new task").unwrap() < live.find("new user").unwrap());
        assert!(live.find("new user").unwrap() < live.find("next task").unwrap());
        t.clear();
        t.push_todo(snapshot("archived task"));
        let _ = t.visible(40, 1, &mut history);
        let archived = text(&history);
        t.push_todo(snapshot("later task"));
        let live = text(&t.visible(40, 30, &mut history));
        assert_eq!(text(&history), archived);
        assert!(live.contains("archived task") && live.contains("later task"));
    }
}
