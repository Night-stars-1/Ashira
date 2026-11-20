//! Names a session from its opening exchange with one short model request.
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::api::{self, Client};

const SYSTEM: &str = "You name conversations. Reply with only a short title for the conversation below: \
in the user's language, at most 20 Chinese characters or 8 English words, \
describing the task rather than quoting it. No quotes, no trailing punctuation, no explanation.";

/// Input is clipped so a pasted log cannot make naming expensive.
const MAX_INPUT_CHARS: usize = 2000;

pub async fn summarize(
    client: &Client,
    user: &str,
    assistant: &str,
    cancel: &CancellationToken,
    on_request: impl FnOnce(&Result<api::Response, String>, std::time::Instant),
) -> Result<String, String> {
    let clip = |text: &str| text.chars().take(MAX_INPUT_CHARS).collect::<String>();
    let input = format!("<user>\n{}\n</user>\n<assistant>\n{}\n</assistant>", clip(user), clip(assistant));
    let started = std::time::Instant::now();
    let response = client.stream(SYSTEM, &client.wire_tools(json!([])), &[api::user_text(&input)], None, cancel, |_| {}).await;
    on_request(&response, started);
    let text: String = response?.content.iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    clean(&text).ok_or_else(|| "empty title".into())
}

/// First line, without wrapping quotes, a "标题：" prefix or trailing punctuation.
fn clean(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = line.strip_prefix("标题：").or_else(|| line.strip_prefix("Title:")).unwrap_or(line).trim();
    // Quotes and trailing punctuation can nest in any order (`「标题」。`), so strip them together.
    let wrap = |c: char| matches!(c, '"' | '\'' | '“' | '”' | '「' | '」' | '《' | '》' | '*' | '#' | '`');
    let line = line.trim_start_matches(wrap)
        .trim_end_matches(|c: char| wrap(c) || matches!(c, '。' | '.' | '！' | '!' | '？' | '?' | '，' | ',' | '：' | ':'))
        .trim();
    let title: String = line.chars().filter(|c| !c.is_control()).take(40).collect();
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_output_becomes_a_clean_title() {
        assert_eq!(super::clean("「修复登录页的 Grok 思考显示」。\n多余的解释").as_deref(), Some("修复登录页的 Grok 思考显示"));
        assert_eq!(super::clean("\n标题：配置文件格式选择\n").as_deref(), Some("配置文件格式选择"));
        assert_eq!(super::clean("**Fix flaky CI tests.**").as_deref(), Some("Fix flaky CI tests"));
        assert_eq!(super::clean("  \n \"\" "), None);
        assert_eq!(super::clean(&"长".repeat(100)).map(|t| t.chars().count()), Some(40));
    }
}
