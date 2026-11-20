//! Transactional, session-scoped todo state.
use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status { Pending, InProgress, Completed, Abandoned, Blocked }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task { pub content: String, pub status: Status, #[serde(default, skip_serializing_if = "Option::is_none")] pub reason: Option<String> }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Phase { pub name: String, pub tasks: Vec<Task> }
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct State { pub phases: Vec<Phase> }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot { pub op: String, pub state: State, #[serde(default)] pub touched: Vec<String> }

fn label(value: &Value, field: &str) -> Result<String, String> {
    let s = value.as_str().ok_or_else(|| format!("{field} must be a string"))?;
    if s.trim().is_empty() { return Err(format!("{field} must not be empty")); }
    Ok(s.to_owned())
}
/// Persist mutations before exposing them; views are best-effort history only.
pub fn commit(state: &mut State, session: &mut crate::session::Session, input: &Value) -> Result<Snapshot, String> {
    let snapshot = state.apply(input)?;
    if snapshot.op == "view" { let _ = session.todo_checked(&snapshot); }
    else { session.todo_checked(&snapshot)?; *state = snapshot.state.clone(); }
    Ok(snapshot)
}

fn items(value: &Value) -> Result<Vec<String>, String> {
    let values = value.as_array().ok_or("items must be an array")?;
    if values.is_empty() { return Err("items must not be empty".into()); }
    values.iter().map(|v| label(v, "item")).collect()
}
fn tasks(items: Vec<String>) -> Vec<Task> { items.into_iter().map(|content| Task { content, status: Status::Pending, reason: None }).collect() }

impl State {
    pub fn validate(&self) -> Result<(), String> {
        let (mut phases, mut tasks) = (HashSet::new(), HashSet::new());

        for phase in &self.phases {
            if phase.name.trim().is_empty() || !phases.insert(&phase.name) { return Err("empty or duplicate phase".into()); }
            for task in &phase.tasks {
                if task.content.trim().is_empty() || !tasks.insert(&task.content) { return Err("empty or duplicate task".into()); }

                if task.status != Status::Blocked && task.reason.is_some() { return Err("reason on non-blocked task".into()); }
            }
        }

        Ok(())
    }
    fn normalize(&mut self) {
        if self.phases.iter().flat_map(|phase| &phase.tasks).any(|task| task.status == Status::InProgress) { return; }
        if let Some(task) = self.phases.iter_mut().flat_map(|phase| &mut phase.tasks).find(|task| task.status == Status::Pending) { task.status = Status::InProgress; }
    }
    pub fn apply(&self, input: &Value) -> Result<Snapshot, String> {
        let op = input.get("op").and_then(Value::as_str).ok_or("op is required")?;
        let mut state = self.clone();
        let mut touched = Vec::new();
        match op {
            "view" => return Ok(Snapshot { op: op.into(), state, touched }),
            "init" => {
                if let Some(list) = input.get("list") {
                    if input.get("items").is_some() || input.get("phase").is_some() { return Err("list conflicts with items or phase".into()); }
                    let list = list.as_array().ok_or("list must be an array")?;
                    if list.is_empty() { return Err("list must not be empty".into()); }
                    state.phases = list.iter().map(|entry| Ok(Phase { name: label(&entry["phase"], "phase")?, tasks: tasks(items(&entry["items"])?) })).collect::<Result<_, String>>()?;
                } else {
                    let name = match input.get("phase") { Some(v) => label(v, "phase")?, None => "任务".into() };
                    state.phases = vec![Phase { name, tasks: tasks(items(&input["items"])?) }];
                }
                touched.extend(state.phases.iter().flat_map(|p| p.tasks.iter().map(|t| t.content.clone())));
            }
            "append" => {
                let new = items(&input["items"])?;
                let name = if let Some(v) = input.get("phase") { label(v, "phase")? } else {
                    state.phases.iter().find(|p| p.tasks.iter().any(|t| t.status == Status::InProgress))
                        .or_else(|| state.phases.iter().find(|p| p.tasks.iter().any(|t| t.status == Status::Pending)))
                        .or_else(|| state.phases.last()).map(|p| p.name.clone()).unwrap_or_else(|| "任务".into())
                };
                touched.extend(new.iter().cloned());
                if let Some(phase) = state.phases.iter_mut().find(|p| p.name == name) { phase.tasks.extend(tasks(new)); }
                else { state.phases.push(Phase { name, tasks: tasks(new) }); }
            }
            "start" => {
                let mut targets = Vec::new();
                if let Some(task) = input.get("task") { targets.push(label(task, "task")?); }
                if input.get("items").is_some() { targets.extend(items(&input["items"])?); }
                if targets.is_empty() { return Err("task or items is required".into()); }
                if targets.iter().any(|target| !state.phases.iter().flat_map(|phase| &phase.tasks).any(|task| &task.content == target)) { return Err("task not found".into()); }
                for task in state.phases.iter_mut().flat_map(|phase| &mut phase.tasks) {
                    if targets.iter().any(|target| target == &task.content) { task.status = Status::InProgress; task.reason = None; }
                }
                touched.extend(targets);
            }
            "done" | "drop" | "rm" | "block" | "unblock" => {
                let task = input.get("task").map(|v| label(v, "task")).transpose()?;
                let phase = if task.is_none() { input.get("phase").map(|v| label(v, "phase")).transpose()? } else { None };
                if task.is_none() && phase.is_none() && matches!(op, "block" | "unblock") { return Err("task or phase is required".into()); }
                if let Some(t) = &task && !state.phases.iter().flat_map(|p| &p.tasks).any(|x| &x.content == t) { return Err("task not found".into()); }
                if let Some(p) = &phase && !state.phases.iter().any(|x| &x.name == p) { return Err("phase not found".into()); }
                let reason = match input.get("reason") { Some(v) => { let s = v.as_str().ok_or("reason must be a string")?.split_whitespace().collect::<Vec<_>>().join(" "); if s.is_empty() { None } else { Some(s) } }, None => None };
                if op == "rm" && task.is_none() && phase.is_none() { touched.extend(state.phases.iter().flat_map(|p| p.tasks.iter().map(|t| t.content.clone()))); state.phases.clear(); }
                else {
                    for p in &mut state.phases {
                        if phase.as_ref().is_some_and(|name| name != &p.name) { continue; }
                        p.tasks.retain_mut(|t| {
                            if task.as_ref().is_some_and(|name| name != &t.content) { return true; }
                            touched.push(t.content.clone());
                            match op {
                                "rm" => return false,
                                "done" => { t.status = Status::Completed; t.reason = None; },
                                "drop" => { t.status = Status::Abandoned; t.reason = None; },
                                "block" if matches!(t.status, Status::Pending | Status::InProgress | Status::Blocked) => { t.status = Status::Blocked; t.reason = reason.clone(); },
                                "unblock" if t.status == Status::Blocked => { t.status = Status::Pending; t.reason = None; },
                                _ => {}
                            }
                            true
                        });
                    }
                }
            }
            _ => return Err(format!("unknown todo op: {op}")),
        }
        state.validate()?;
        state.normalize();
        Ok(Snapshot { op: op.into(), state, touched })
    }
}
impl Snapshot {
    pub fn summary(&self) -> String {
        let mut text = String::new();
        let mut counts = [0usize; 5];
        for phase in &self.state.phases {
            text.push_str(&format!("\n{}:\n", phase.name));
            for task in &phase.tasks {
                let (index, status) = match task.status { Status::Pending => (0, "pending"), Status::InProgress => (1, "in_progress"), Status::Completed => (2, "completed"), Status::Abandoned => (3, "abandoned"), Status::Blocked => (4, "blocked") };
                counts[index] += 1;
                text.push_str(&format!("- {} [{}]", task.content, status));
                if let Some(reason) = &task.reason { text.push_str(&format!(" ({reason})")); }
                text.push('\n');
            }
        }
        format!("Todo: {} pending, {} in_progress, {} completed, {} abandoned, {} blocked{}", counts[0], counts[1], counts[2], counts[3], counts[4], text)
    }
}
