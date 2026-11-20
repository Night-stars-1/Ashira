//! Inline todo card matching omp's transcript checklist: roman phase headers,
//! checkbox rows, and a collapsed walking viewport.
use ratatui::{style::{Color, Modifier, Style}, text::{Line, Span}};
use crate::{render::{sanitize, DIM}, todo::{Snapshot, Status, Task}, transcript::hard_wrap};

const ICON: &str = "\u{eab3}";
const BOX_ON: &str = "\u{f14a}";
const BOX_OFF: &str = "\u{f096}";
const TASK_CAP: usize = 8;

fn single_line(text: &str) -> String { sanitize(text).replace('\n', " ") }

fn roman(mut n: usize) -> String {
    const PAIRS: &[(usize, &str)] = &[
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    for (value, symbol) in PAIRS {
        while n >= *value { out.push_str(symbol); n -= value; }
    }
    out
}

fn closed(status: &Status) -> bool { matches!(status, Status::Completed | Status::Abandoned) }

/// Last closed task, then in-progress work, then the pending tasks after it.
fn collapsed(tasks: &[Task]) -> (Vec<&Task>, usize) {
    let open: Vec<&Task> = tasks.iter().filter(|task| !closed(&task.status)).collect();
    let settled = open.is_empty();
    let base: Vec<&Task> = if settled { tasks.iter().collect() } else { open };
    let (mut items, hidden) = within_cap(&base);
    if !settled
        && let Some(lead) = tasks.iter().rev().find(|task| closed(&task.status))
    {
        items.insert(0, lead);
    }
    (items, hidden)
}

fn within_cap<'a>(base: &[&'a Task]) -> (Vec<&'a Task>, usize) {
    if base.len() <= TASK_CAP + 1 { return (base.to_vec(), 0); }
    let active: Vec<&Task> = base.iter().copied().filter(|task| task.status == Status::InProgress).collect();
    if active.len() > TASK_CAP {
        let hidden = active.len() - TASK_CAP;
        return (active.into_iter().take(TASK_CAP).collect(), hidden);
    }
    let first = active.first().and_then(|task| base.iter().position(|item| std::ptr::eq(*item, *task))).unwrap_or(0);
    let mut fill = Vec::new();
    for task in base.iter().skip(first).copied() {
        if task.status == Status::InProgress { continue; }
        if active.len() + fill.len() >= TASK_CAP { break; }
        fill.push(task);
    }
    let shown = active.len() + fill.len();
    let mut items = active;
    items.extend(fill);
    (items, base.len() - shown)
}

fn task_row(task: &Task, indent: &str, last: bool) -> Line<'static> {
    let (box_mark, style, note) = match task.status {
        Status::Pending => (BOX_OFF, DIM, None),
        Status::InProgress => (BOX_OFF, Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD), None),
        Status::Completed => (BOX_ON, Style::new().fg(Color::Green).add_modifier(Modifier::CROSSED_OUT), None),
        Status::Abandoned => (BOX_OFF, Style::new().fg(Color::Red).add_modifier(Modifier::CROSSED_OUT), None),
        Status::Blocked => {
            let note = task.reason.as_ref().filter(|reason| !reason.is_empty()).map(|reason| format!("blocked: {}", single_line(reason)));
            (BOX_OFF, Style::new().fg(Color::Yellow), Some(note.unwrap_or_else(|| "blocked".to_owned())))
        }
    };
    let mut label = format!("{box_mark} {}", single_line(&task.content));
    if let Some(note) = note { label.push_str(&format!(" ({note})")); }
    let branch = if last { "└─" } else { "├─" };
    Line::from(vec![Span::styled(format!("{indent}{branch} "), DIM), Span::styled(label, style)])
}

pub fn rows(snapshot: &Snapshot, width: u16, expanded: bool) -> Vec<Line<'static>> {
    let phases: Vec<_> = snapshot.state.phases.iter().filter(|phase| !phase.tasks.is_empty()).collect();
    let total: usize = phases.iter().map(|phase| phase.tasks.len()).sum();
    let mut rows = vec![Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{ICON} "), Style::new().fg(Color::Cyan)),
        Span::styled("Todo", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {total} tasks"), DIM),
    ])];
    if total == 0 {
        rows.push(Line::styled(" 暂无待办", DIM));
        return hard_wrap(&rows, width);
    }
    let multi = phases.len() > 1;
    let touched_known = snapshot.touched.iter().any(|content| {
        phases.iter().any(|phase| phase.tasks.iter().any(|task| &task.content == content))
    });
    let mut hidden = false;
    for (index, phase) in phases.iter().enumerate() {
        let targeted = phase.tasks.iter().any(|task| {
            task.status == Status::InProgress || snapshot.touched.iter().any(|content| content == &task.content)
        });
        let open = expanded || targeted || !touched_known;
        let done = phase.tasks.iter().filter(|task| closed(&task.status)).count();
        let title = format!("{}. {}", roman(index + 1), single_line(&phase.name));
        let progress = format!("  {done}/{}", phase.tasks.len());
        if multi && !open {
            rows.push(Line::from(vec![
                Span::styled(format!(" {title}"), DIM.add_modifier(Modifier::BOLD)),
                Span::styled(progress, DIM),
            ]));
            hidden = true;
            continue;
        }
        if multi {
            rows.push(Line::from(vec![
                Span::styled(format!(" {title}"), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(progress, DIM),
            ]));
        }
        let (shown, omitted) = if expanded { (phase.tasks.iter().collect(), 0) } else { collapsed(&phase.tasks) };
        hidden |= omitted > 0;
        let indent = if multi { "   " } else { " " };
        let last_task = omitted == 0;
        for (offset, task) in shown.iter().enumerate() {
            rows.push(task_row(task, indent, last_task && offset + 1 == shown.len()));
        }
        if omitted > 0 {
            rows.push(Line::from(vec![
                Span::styled(format!("{indent}└─ "), DIM),
                Span::styled(format!("… {omitted} more todos"), DIM),
            ]));
        }
    }
    if expanded { rows.push(Line::styled(" Ctrl+O: Collapse", DIM)); }
    else if hidden { rows.push(Line::styled(" Ctrl+O: Expand", DIM)); }
    hard_wrap(&rows, width)
}
