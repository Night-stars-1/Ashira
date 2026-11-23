//! OpenAI Chat Completions wire format. History stays in the canonical
//! (Anthropic block) format; it is converted while serializing, borrowing
//! from the history instead of copying it.

use std::borrow::Cow;

use serde::{
    Serialize,
    ser::{SerializeSeq, Serializer},
};
use serde_json::{Map, Value, json};

use crate::api::{DataUrl, Decoder, Response, StreamEvent, Usage, parse_input};

#[derive(Serialize)]
pub struct Body<'a> {
    pub model: &'a str,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    pub tools: &'a Value,
    pub messages: Messages<'a>,
    /// `compat.extraBody` plus max tokens / reasoning effort when set.
    #[serde(flatten)]
    pub params: &'a Map<String, Value>,
}

#[derive(Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// `{name, description, input_schema}` -> `{type: function, function: {name, description, parameters}}`.
pub fn tools(tools: Value) -> Value {
    let Value::Array(list) = tools else { return tools };
    list.into_iter()
        .map(|mut t| {
            json!({
                "type": "function",
                "function": {
                    "name": t["name"].take(),
                    "description": t["description"].take(),
                    "parameters": t["input_schema"].take(),
                }
            })
        })
        .collect()
}

pub struct Messages<'a> {
    pub system: &'a str,
    pub history: &'a [Value],
    pub extra_user: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum Msg<'a> {
    System {
        content: &'a str,
    },
    User {
        content: UserContent<'a>,
    },
    Tool {
        tool_call_id: &'a str,
        content: Cow<'a, str>,
    },
    Assistant {
        content: Option<Cow<'a, str>>,
        /// Returned by reasoning models (DeepSeek and others); thinking-mode
        /// tool calls require it to be sent back.
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<Cow<'a, str>>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<Call<'a>>,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum UserContent<'a> {
    Text(Cow<'a, str>),
    Parts(Vec<UserPart<'a>>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserPart<'a> {
    Text { text: &'a str },
    ImageUrl { image_url: ImageUrl<'a> },
}

#[derive(Serialize)]
struct ImageUrl<'a> {
    url: DataUrl<'a>,
}

#[derive(Serialize)]
struct Call<'a> {
    id: &'a str,
    r#type: &'static str,
    function: Func<'a>,
}

#[derive(Serialize)]
struct Func<'a> {
    name: &'a str,
    arguments: String,
}

impl Serialize for Messages<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        seq.serialize_element(&Msg::System { content: self.system })?;
        for m in self.history {
            let blocks = m["content"].as_array().map(Vec::as_slice).unwrap_or_default();
            let of = |kind: &'static str| blocks.iter().filter(move |b| b["type"] == kind);
            if m["role"] == "assistant" {
                let calls: Vec<Call> = of("tool_use")
                    .map(|b| Call {
                        id: b["id"].as_str().unwrap_or_default(),
                        r#type: "function",
                        function: Func {
                            name: b["name"].as_str().unwrap_or_default(),
                            arguments: match b["input"].get("_invalid_json") {
                                Some(raw) => raw.as_str().unwrap_or_default().to_owned(),
                                None => b["input"].to_string(),
                            },
                        },
                    })
                    .collect();
                let text = join(of("text").filter_map(|b| b["text"].as_str()));
                // Signed thinking is Anthropic's and cannot be replayed here.
                let reasoning = join(of("thinking").filter(|b| b.get("signature").is_none()).filter_map(|b| b["thinking"].as_str()));
                let content = if text.is_none() && calls.is_empty() { Some(Cow::Borrowed("")) } else { text };
                seq.serialize_element(&Msg::Assistant { content, reasoning_content: reasoning, tool_calls: calls })?;
                continue;
            }
            // Tool results must directly follow the assistant's tool calls; pictures they
            // return go into the user message after them, since tool messages are text only.
            let mut tool_images = Vec::new();
            for b in of("tool_result") {
                let (content, images) = crate::api::tool_result_parts(b);
                tool_images.extend(images);
                seq.serialize_element(&Msg::Tool { tool_call_id: b["tool_use_id"].as_str().unwrap_or_default(), content })?;
            }
            if !tool_images.is_empty() || blocks.iter().any(|b| b["type"] == "image") {
                let mut parts = Vec::new();
                for b in tool_images.into_iter().chain(blocks.iter().filter(|b| b["type"] != "tool_result")) {
                    match b["type"].as_str() {
                        Some("text") => parts.push(UserPart::Text { text: b["text"].as_str().unwrap_or_default() }),
                        Some("image") => parts.push(UserPart::ImageUrl { image_url: ImageUrl { url: DataUrl::from_block(b).map_err(serde::ser::Error::custom)? } }),
                        _ => {}
                    }
                }
                seq.serialize_element(&Msg::User { content: UserContent::Parts(parts) })?;
            } else if let Some(text) = join(of("text").filter_map(|b| b["text"].as_str())) {
                seq.serialize_element(&Msg::User { content: UserContent::Text(text) })?;
            }
        }
        if let Some(text) = self.extra_user {
            seq.serialize_element(&Msg::User { content: UserContent::Text(Cow::Borrowed(text)) })?;
        }
        seq.end()
    }
}

/// Borrows a single part; allocates only when several parts must be joined.
fn join<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<Cow<'a, str>> {
    let first = parts.find(|p| !p.is_empty())?;
    let mut out = Cow::Borrowed(first);
    for p in parts.filter(|p| !p.is_empty()) {
        let s = out.to_mut();
        s.push('\n');
        s.push_str(p);
    }
    Some(out)
}

#[derive(Default)]
struct PendingCall {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
pub struct Stream {
    reasoning: String,
    text: String,
    calls: Vec<PendingCall>,
    usage: Usage,
    usage_reported: bool,
    finished: bool,
    reasoning_open: bool,
    done: bool,
    truncated: bool,
}

impl Decoder for Stream {
    fn handle(&mut self, data: &str, on_text: &mut (dyn FnMut(StreamEvent<'_>) + Send)) -> Result<(), String> {
        if data.trim() == "[DONE]" {
            if self.reasoning_open {
                on_text(StreamEvent::ReasoningEnd);
                self.reasoning_open = false;
            }
            self.done = true;
            return Ok(());
        }
        let ev: Value = serde_json::from_str(data).map_err(|e| format!("bad SSE data: {e}"))?;
        if let Some(err) = ev.get("error").filter(|e| !e.is_null()) {
            return Err(format!("API error: {}", err["message"].as_str().map(str::to_owned).unwrap_or_else(|| err.to_string())));
        }
        if let Some(u) = ev.get("usage").filter(|u| u.is_object()) {
            self.usage_reported = u["prompt_tokens"].as_u64().is_some() && u["completion_tokens"].as_u64().is_some();
            let get = |v: &Value| v.as_u64().unwrap_or(0);
            let prompt = get(&u["prompt_tokens"]);
            let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().or(u["prompt_cache_hit_tokens"].as_u64()).unwrap_or(0).min(prompt);
            self.usage = Usage { input: prompt - cached, output: get(&u["completion_tokens"]), cache_read: cached, cache_write: 0 };
        }
        let Some(choice) = ev["choices"].as_array().and_then(|c| c.first()) else { return Ok(()) };
        let d = &choice["delta"];
        if let Some(r) = ["reasoning_content", "reasoning", "reasoning_text"].iter()
            .filter_map(|key| d[*key].as_str()).find(|text| !text.is_empty())
        {
            self.reasoning.push_str(r);
            if !r.is_empty() {
                on_text(StreamEvent::ReasoningDelta(r));
                self.reasoning_open = true;
            }
        }
        if let Some(t) = d["content"].as_str()
            && !t.is_empty()
        {
            if self.reasoning_open {
                on_text(StreamEvent::ReasoningEnd);
                self.reasoning_open = false;
            }
            self.text.push_str(t);
            on_text(StreamEvent::TextDelta(t));
        }
        if d["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()) && self.reasoning_open {
            on_text(StreamEvent::ReasoningEnd);
            self.reasoning_open = false;
        }
        for tc in d["tool_calls"].as_array().into_iter().flatten() {
            let has_id = tc["id"].as_str().is_some_and(|s| !s.is_empty());
            let idx = match tc["index"].as_u64() {
                Some(i) => i as usize,
                None if has_id || self.calls.is_empty() => self.calls.len(),
                None => self.calls.len() - 1,
            };
            if self.calls.len() <= idx {
                self.calls.resize_with(idx + 1, PendingCall::default);
            }
            let call = &mut self.calls[idx];
            if has_id {
                call.id = tc["id"].as_str().unwrap_or_default().to_owned();
            }
            let f = &tc["function"];
            if let Some(n) = f["name"].as_str() {
                call.name.push_str(n);
            }
            if let Some(a) = f["arguments"].as_str() {
                call.args.push_str(a);
            }
        }
        if choice["finish_reason"].is_string() {
            if self.reasoning_open {
                on_text(StreamEvent::ReasoningEnd);
                self.reasoning_open = false;
            }
            self.finished = true;
            self.truncated |= choice["finish_reason"] == "length";
        }
        Ok(())
    }

    fn done(&self) -> bool {
        self.done
    }

    fn finish(self) -> Result<Response, String> {
        if !self.done && !self.finished {
            return Err("stream ended before finish_reason".into());
        }
        let mut content = Vec::new();
        if !self.reasoning.is_empty() {
            content.push(json!({ "type": "thinking", "thinking": self.reasoning }));
        }
        if !self.text.is_empty() {
            content.push(json!({ "type": "text", "text": self.text }));
        }
        for (i, c) in self.calls.into_iter().enumerate().filter(|(_, c)| !c.name.is_empty()) {
            let id = if c.id.is_empty() { format!("call_{i}") } else { c.id };
            content.push(json!({ "type": "tool_use", "id": id, "name": c.name, "input": parse_input(c.args) }));
        }
        Ok(Response { content, usage: self.usage, usage_reported: self.usage_reported, duration_ms: 0, truncated: self.truncated })
    }
}
