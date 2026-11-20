//! OpenAI Responses API (`api: openai-responses`). Requests are stateless
//! (`store: false`); reasoning items come back with `encrypted_content` and
//! are replayed verbatim, but only to the model that produced them.

use std::{borrow::Cow, collections::BTreeMap};

use serde::{
    Serialize,
    ser::{SerializeSeq, Serializer},
};
use serde_json::{Map, Value, json};

use crate::api::{DataUrl, Decoder, Response, StreamEvent, Usage, parse_input};

#[derive(Serialize)]
pub struct Body<'a> {
    pub model: &'a str,
    pub instructions: &'a str,
    pub input: Input<'a>,
    pub tools: &'a Value,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<[&'static str; 1]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(flatten)]
    pub extra: &'a Map<String, Value>,
}

#[derive(Serialize)]
pub struct Reasoning<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<&'a str>,
}

/// Preserve optional arguments instead of letting Responses normalize tools to strict schemas.
pub fn tools(tools: Value) -> Value {
    let Value::Array(list) = tools else { return tools };
    list.into_iter().map(|mut t| json!({ "type": "function", "name": t["name"].take(), "description": t["description"].take(), "parameters": t["input_schema"].take(), "strict": false })).collect()
}

pub struct Input<'a> {
    pub history: &'a [Value],
    pub extra_user: Option<&'a str>,
    /// `provider/id` of the model being called; reasoning items from other models are dropped.
    pub source: &'a str,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Item<'a> {
    Message { role: &'static str, content: Vec<Part<'a>> },
    FunctionCall { call_id: &'a str, name: &'a str, arguments: String },
    FunctionCallOutput { call_id: &'a str, output: Cow<'a, str> },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Part<'a> {
    InputText { text: &'a str },
    InputImage { image_url: DataUrl<'a> },
    OutputText { text: &'a str },
}

impl Serialize for Input<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        for m in self.history {
            let blocks = m["content"].as_array().map(Vec::as_slice).unwrap_or_default();
            if m["role"] == "assistant" {
                for b in blocks {
                    match b["type"].as_str() {
                        Some("reasoning_item") if b["source"] == self.source => seq.serialize_element(&b["item"])?,
                        Some("text") => {
                            let text = b["text"].as_str().unwrap_or_default();
                            if !text.is_empty() {
                                seq.serialize_element(&Item::Message { role: "assistant", content: vec![Part::OutputText { text }] })?;
                            }
                        }
                        Some("tool_use") => seq.serialize_element(&Item::FunctionCall {
                            call_id: b["id"].as_str().unwrap_or_default(),
                            name: b["name"].as_str().unwrap_or_default(),
                            arguments: match b["input"].get("_invalid_json") {
                                Some(raw) => raw.as_str().unwrap_or_default().to_owned(),
                                None => b["input"].to_string(),
                            },
                        })?,
                        _ => {}
                    }
                }
                continue;
            }
            for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                let text = match &b["content"] {
                    Value::String(t) => Cow::Borrowed(t.as_str()),
                    other => Cow::Owned(other.to_string()),
                };
                let output = if b["is_error"] == true { Cow::Owned(format!("Error: {text}")) } else { text };
                seq.serialize_element(&Item::FunctionCallOutput { call_id: b["tool_use_id"].as_str().unwrap_or_default(), output })?;
            }
            let mut parts = Vec::new();
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = b["text"].as_str() {
                            parts.push(Part::InputText { text });
                        }
                    }
                    Some("image") => parts.push(Part::InputImage { image_url: DataUrl::from_block(b).map_err(serde::ser::Error::custom)? }),
                    _ => {}
                }
            }
            if !parts.is_empty() {
                seq.serialize_element(&Item::Message { role: "user", content: parts })?;
            }
        }
        if let Some(text) = self.extra_user {
            seq.serialize_element(&Item::Message { role: "user", content: vec![Part::InputText { text }] })?;
        }
        seq.end()
    }
}

pub struct Stream {
    source: String,
    items: Vec<Value>,
    args: Vec<String>,
    text: Vec<String>,
    summaries: BTreeMap<(usize, usize), String>,
    active_summary: Option<(usize, usize)>,
    usage: Usage,
    usage_reported: bool,
    done: bool,
}

impl Stream {
    pub fn new(source: &str) -> Stream {
        Stream { source: source.into(), items: Vec::new(), args: Vec::new(), text: Vec::new(), summaries: BTreeMap::new(), active_summary: None, usage: Usage::default(), usage_reported: false, done: false }
    }

    fn slot(&mut self, ev: &Value) -> usize {
        let i = ev["output_index"].as_u64().unwrap_or(0) as usize;
        if self.items.len() <= i {
            self.items.resize(i + 1, Value::Null);
            self.args.resize(i + 1, String::new());
            self.text.resize(i + 1, String::new());
        }
        i
    }

    fn end_summary(&mut self, emit: &mut (dyn FnMut(StreamEvent<'_>) + Send)) {
        if self.active_summary.take().is_some() {
            emit(StreamEvent::ReasoningEnd);
        }
    }

    fn summary_delta(&mut self, key: (usize, usize), text: &str, emit: &mut (dyn FnMut(StreamEvent<'_>) + Send)) {
        if text.is_empty() { return; }
        if self.active_summary != Some(key) {
            self.end_summary(emit);
            self.active_summary = Some(key);
        }
        self.summaries.entry(key).or_default().push_str(text);
        emit(StreamEvent::ReasoningDelta(text));
    }

    fn summary_full(&mut self, key: (usize, usize), text: &str, emit: &mut (dyn FnMut(StreamEvent<'_>) + Send)) {
        let emitted = self.summaries.get(&key).map(String::as_str).unwrap_or("");
        if let Some(suffix) = text.strip_prefix(emitted) {
            self.summary_delta(key, suffix, emit);
        }
    }

    fn summary_item(&mut self, index: usize, item: &Value, emit: &mut (dyn FnMut(StreamEvent<'_>) + Send)) {
        if item["type"] != "reasoning" { return; }
        for (part, summary) in item["summary"].as_array().into_iter().flatten().enumerate() {
            if let Some(text) = summary["text"].as_str() {
                self.summary_full((index, part), text, emit);
                self.end_summary(emit);
            }
        }
    }

    fn complete(&mut self, resp: &Value) {
        if let Some(out) = resp["output"].as_array().filter(|o| !o.is_empty()) {
            let n = out.len().max(self.items.len());
            self.args.resize(n, String::new());
            self.text.resize(n, String::new());
            self.items = out.clone();
        }
        let u = &resp["usage"];
        if u.is_object() {
            self.usage_reported = u["input_tokens"].as_u64().is_some() && u["output_tokens"].as_u64().is_some();
            let get = |v: &Value| v.as_u64().unwrap_or(0);
            let input = get(&u["input_tokens"]);
            let cached = get(&u["input_tokens_details"]["cached_tokens"]).min(input);
            self.usage = Usage { input: input - cached, output: get(&u["output_tokens"]), cache_read: cached, cache_write: 0 };
        }
        self.done = true;
    }
}

fn message_text(item: &Value) -> String {
    let parts = item["content"].as_array().into_iter().flatten();
    parts.filter(|p| p["type"] == "output_text").filter_map(|p| p["text"].as_str()).collect()
}

impl Decoder for Stream {
    fn handle(&mut self, data: &str, on_text: &mut (dyn FnMut(StreamEvent<'_>) + Send)) -> Result<(), String> {
        if data.trim() == "[DONE]" {
            self.end_summary(on_text);
            self.done = true;
            return Ok(());
        }
        let ev: Value = serde_json::from_str(data).map_err(|e| format!("bad SSE data: {e}"))?;
        match ev["type"].as_str().unwrap_or("") {
            "response.output_item.added" => {
                self.end_summary(on_text);
                let i = self.slot(&ev);
                self.items[i] = ev["item"].clone();
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let key = (ev["output_index"].as_u64().unwrap_or(0) as usize, ev["summary_index"].as_u64().or_else(|| ev["content_index"].as_u64()).unwrap_or(0) as usize);
                self.summary_delta(key, ev["delta"].as_str().unwrap_or_default(), on_text);
            }
            "response.reasoning_summary_part.added" => {
                let key = (ev["output_index"].as_u64().unwrap_or(0) as usize, ev["summary_index"].as_u64().unwrap_or(0) as usize);
                if self.active_summary.is_some() && self.active_summary != Some(key) {
                    self.end_summary(on_text);
                }
                if let Some(text) = ev["part"]["text"].as_str() {
                    self.summary_full(key, text, on_text);
                }
            }
            "response.reasoning_summary_text.done" | "response.reasoning_summary_part.done" | "response.reasoning_text.done" => {
                let key = (ev["output_index"].as_u64().unwrap_or(0) as usize, ev["summary_index"].as_u64().or_else(|| ev["content_index"].as_u64()).unwrap_or(0) as usize);
                if let Some(text) = ev["text"].as_str().or_else(|| ev["part"]["text"].as_str()) {
                    self.summary_full(key, text, on_text);
                }
                if self.active_summary == Some(key) { self.end_summary(on_text); }
            }
            "response.output_text.delta" => {
                self.end_summary(on_text);
                let i = self.slot(&ev);
                let d = ev["delta"].as_str().unwrap_or_default();
                self.text[i].push_str(d);
                on_text(StreamEvent::TextDelta(d));
            }
            "response.function_call_arguments.delta" => {
                self.end_summary(on_text);
                let i = self.slot(&ev);
                self.args[i].push_str(ev["delta"].as_str().unwrap_or_default());
            }
            "response.output_item.done" => {
                let i = self.slot(&ev);
                let item = ev["item"].clone();
                self.summary_item(i, &item, on_text);
                self.end_summary(on_text);
                // Gateways that skip text deltas still get their text shown.
                if item["type"] == "message" && self.text[i].is_empty() {
                    let t = message_text(&item);
                    on_text(StreamEvent::TextDelta(&t));
                    self.text[i] = t;
                }
                self.items[i] = item;
            }
            "response.completed" | "response.incomplete" => {
                for (i, item) in ev["response"]["output"].as_array().into_iter().flatten().enumerate() {
                    self.summary_item(i, item, on_text);
                }
                self.end_summary(on_text);
                self.complete(&ev["response"]);
            }
            "response.failed" => {
                let e = &ev["response"]["error"];
                return Err(format!("API error: {}", e["message"].as_str().map(str::to_owned).unwrap_or_else(|| e.to_string())));
            }
            "error" => {
                let msg = ev["message"].as_str().or(ev["error"]["message"].as_str()).unwrap_or(data);
                return Err(format!("API error: {msg}"));
            }
            _ => {}
        }
        Ok(())
    }

    fn done(&self) -> bool {
        self.done
    }

    fn finish(self) -> Result<Response, String> {
        if !self.done {
            return Err("stream ended before response.completed".into());
        }
        let mut content = Vec::new();
        for (i, item) in self.items.into_iter().enumerate() {
            match item["type"].as_str() {
                // Without encrypted content a stateless request cannot replay the item.
                Some("reasoning") if item["encrypted_content"].is_string() => {
                    content.push(json!({ "type": "reasoning_item", "source": self.source, "item": item }));
                }
                Some("message") => {
                    let mut text = message_text(&item);
                    if text.is_empty() {
                        text = self.text[i].clone();
                    }
                    if !text.is_empty() {
                        content.push(json!({ "type": "text", "text": text }));
                    }
                }
                Some("function_call") => {
                    let args = match item["arguments"].as_str() {
                        Some(a) if !a.is_empty() => a.to_owned(),
                        _ => self.args[i].clone(),
                    };
                    let id = item["call_id"].as_str().or(item["id"].as_str()).map(str::to_owned).unwrap_or_else(|| format!("call_{i}"));
                    content.push(json!({ "type": "tool_use", "id": id, "name": item["name"], "input": parse_input(args) }));
                }
                _ => {}
            }
        }
        Ok(Response { content, usage: self.usage, usage_reported: self.usage_reported, duration_ms: 0 })
    }
}
