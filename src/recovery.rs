//! Turn recovery, after oh-my-pi's: when a reply ends without finishing the work, the
//! agent adds a reminder and asks the model again instead of ending the turn.
//! - empty: no text and no tool call;
//! - length: the output hit the token limit, so its text or tool call is cut off;
//! - stop: text that announces the next step, then stops (a small model judges it);
//! - loop: the same tool calls with the same arguments five turns in a row.
//! Each kind retries at most `MAX_RETRIES` times in a row, then the turn ends as it is.
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::api::{self, Client};

const PROMPTS: &str = include_str!("prompts/recovery.md");
pub const MAX_RETRIES: u32 = 3;
/// Identical tool-call turns in a row before the loop reminder.
const LOOP_THRESHOLD: u32 = 5;
/// Tools that legitimately repeat with the same arguments while their state changes.
const LOOP_EXEMPT: [&str; 3] = ["wait", "task_status", "todo"];
/// The judge sees only the end of a long message and of the request.
const JUDGE_CHARS: usize = 2000;

/// One `# name` section of `prompts/recovery.md`.
fn section(name: &str) -> &'static str {
    let heading = format!("# {name}\n");
    let start = PROMPTS.find(&heading).map(|at| at + heading.len()).expect("recovery prompt section");
    let rest = &PROMPTS[start..];
    rest[..rest.find("\n# ").unwrap_or(rest.len())].trim()
}

/// The reminder for `kind` ("empty", "length" or "stop") on retry `attempt`.
pub fn reminder(kind: &str, attempt: u32) -> String {
    section(kind).replace("{attempt}", &attempt.to_string()).replace("{max}", &MAX_RETRIES.to_string())
}

/// Every reminder starts with this tag; history views show such a message as a note,
/// not as something the user typed.
pub const TAG: &str = "<system-reminder>";

/// User-role text the agent wrote itself rather than the user: a reminder, a compaction
/// summary, or delivered subagent results or peer messages.
pub fn is_internal(text: &str) -> bool {
    text.starts_with(TAG) || text.starts_with("This session continues from an earlier conversation") || text.starts_with("Untrusted ")
}

/// What a history view says in place of a reminder.
pub fn note(text: &str) -> &'static str {
    if text.contains("output token limit") { "回复超出输出长度上限，已丢弃并提醒模型分步完成" }
    else if text.contains("had no text and no tool call") { "模型返回了空回复，已提醒它继续" }
    else if text.contains("times in a row with identical arguments") { "模型在重复同样的工具调用，已提醒它换个做法" }
    else { "模型说要继续却停下了，已提醒它接着做" }
}

fn shorten(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit { return flat; }
    let mut cut: String = flat.chars().take(limit).collect();
    cut.push('…');
    cut
}

/// Arguments without the user-visible `i` title, with keys in order, so two calls that
/// differ only in their title still count as the same call.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.iter().filter(|(key, _)| key.as_str() != "i").map(|(key, value)| (key.clone(), canonical(value))).collect()),
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// Counts turns whose tool calls repeat the previous turn's exactly.
#[derive(Default)]
pub struct LoopGuard {
    last: Option<String>,
    count: u32,
}

impl LoopGuard {
    /// Records one turn's tool calls and their results; returns the reminder once the
    /// same calls have come `LOOP_THRESHOLD` times in a row, and again on every repeat after.
    pub fn record(&mut self, uses: &[Value], results: &[Value]) -> Option<String> {
        let counted: Vec<&Value> = uses.iter().filter(|call| !LOOP_EXEMPT.contains(&call["name"].as_str().unwrap_or(""))).collect();
        if counted.is_empty() {
            // Only polling tools ran: state may have moved on, so the run starts over.
            self.last = None;
            self.count = 0;
            return None;
        }
        let mut keys: Vec<String> = counted.iter().map(|call| json!([call["name"], canonical(&call["input"])]).to_string()).collect();
        keys.sort();
        let key = keys.join("\n");
        if self.last.as_deref() == Some(key.as_str()) { self.count += 1 } else { self.last = Some(key); self.count = 1; }
        if self.count < LOOP_THRESHOLD { return None; }
        let call = counted[0];
        let id = call["id"].as_str().unwrap_or("");
        let result = results.iter().find(|result| result["tool_use_id"].as_str() == Some(id)).map(|result| result_text(&result["content"])).unwrap_or_default();
        Some(section("loop")
            .replace("{tool}", call["name"].as_str().unwrap_or(""))
            .replace("{count}", &self.count.to_string())
            .replace("{args}", &shorten(&canonical(&call["input"]).to_string(), 400))
            .replace("{result}", &shorten(if result.is_empty() { "(no text result)" } else { &result }, 200)))
    }
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Whether `message`, which ended the turn without a tool call, announced work it then
/// did not do. A failed or unclear judgement counts as no, so the turn ends as before.
pub async fn stopped_early(
    client: &Client,
    request: &str,
    message: &str,
    cancel: &CancellationToken,
    on_request: impl FnOnce(&Result<api::Response, String>, std::time::Instant),
) -> bool {
    let tail = |text: &str| {
        let count = text.chars().count();
        text.chars().skip(count.saturating_sub(JUDGE_CHARS)).collect::<String>()
    };
    let input = json!({"request": tail(request), "message": tail(message)}).to_string();
    let started = std::time::Instant::now();
    let response = client.stream(section("judge"), &client.wire_tools(json!([])), &[api::user_text(&input)], None, cancel, |_| {}).await;
    on_request(&response, started);
    let Ok(response) = response else { return false };
    let answer: String = response.content.iter().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
    answer.trim().trim_matches(|c: char| !c.is_alphanumeric()).eq_ignore_ascii_case("yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str, input: Value) -> Value { json!({"type": "tool_use", "id": id, "name": name, "input": input}) }
    fn result(id: &str, text: &str) -> Value { json!({"type": "tool_result", "tool_use_id": id, "content": text}) }

    #[test]
    fn the_fifth_identical_turn_is_a_loop() {
        let mut guard = LoopGuard::default();
        for turn in 1..LOOP_THRESHOLD {
            // The title differs every time; the call itself does not.
            let uses = [call("a", "bash", json!({"command": "ls", "i": format!("step {turn}")}))];
            assert_eq!(guard.record(&uses, &[result("a", "x")]), None);
        }
        let reminder = guard.record(&[call("a", "bash", json!({"command": "ls"}))], &[result("a", "no such file")]).expect("loop");
        assert!(reminder.contains("`bash` 5 times") && reminder.contains(r#"{"command":"ls"}"#) && reminder.contains("no such file"));
    }

    #[test]
    fn a_changed_call_or_a_poll_restarts_the_count() {
        let mut guard = LoopGuard::default();
        let ls = || [call("a", "bash", json!({"command": "ls"}))];
        for _ in 0..LOOP_THRESHOLD - 1 { assert_eq!(guard.record(&ls(), &[]), None); }
        assert_eq!(guard.record(&[call("b", "bash", json!({"command": "ls -a"}))], &[]), None);
        for _ in 0..LOOP_THRESHOLD - 1 { assert_eq!(guard.record(&ls(), &[]), None); }
        // A wait may have delivered results, so it ends the run too.
        assert_eq!(guard.record(&[call("w", "wait", json!({}))], &[]), None);
        assert_eq!(guard.record(&ls(), &[]), None);
    }

    #[test]
    fn reminders_fill_their_counts() {
        let text = reminder("length", 2);
        assert!(text.contains("Attempt 2/3") && text.contains("output token limit") && !text.contains("# stop"));
        assert!(reminder("stop", 1).starts_with("<system-reminder>"));
        assert!(section("judge").ends_with("yes or no."));
    }
}
