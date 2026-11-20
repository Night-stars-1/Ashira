//! Single-column report for global memory.

use std::path::Path;

use ratatui::{style::{Color, Style}, text::{Line, Span}};

use crate::{memory::Snapshot, render::DIM};

pub fn body(snapshot: &Snapshot, path: &Path) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(format!("存储位置：{}", path.display()), DIM), Line::raw("")];
    if snapshot.entries.is_empty() {
        lines.push(Line::raw("尚无全局记忆。"));
        lines.push(Line::styled("使用 /memory set <键> <内容> 保存记忆。", DIM));
    } else {
        for (key, value) in &snapshot.entries {
            lines.push(Line::from(Span::styled(key.clone(), Style::new().fg(Color::Cyan))));
            for line in value.split('\n') {
                lines.push(Line::raw(format!("  {line}")));
            }
            lines.push(Line::raw(""));
        }
    }
    lines
}
