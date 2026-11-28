use std::{borrow::Cow, error::Error, fmt, time::{Duration, Instant}};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{
    Deserialize, Serialize,
    ser::{SerializeMap, SerializeSeq, Serializer},
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{Api, Level, Resolved, resolve_key},
    openai, responses,
};

pub const CANCELLED: &str = "cancelled";
const MAX_TOKENS: u32 = 16_384;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    /// Tokens occupied in the context window after this response.
    pub fn context(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }

    fn merge(&mut self, u: &Value) {
        let get = |k: &str| u.get(k).and_then(Value::as_u64);
        if let Some(v) = get("input_tokens") {
            self.input = v;
        }
        if let Some(v) = get("output_tokens") {
            self.output = v;
        }
        if let Some(v) = get("cache_read_input_tokens") {
            self.cache_read = v;
        }
        if let Some(v) = get("cache_creation_input_tokens") {
            self.cache_write = v;
        }
    }
}

/// Assistant content in the canonical (Anthropic block) format, whichever
/// provider produced it.
pub struct Response {
    pub content: Vec<Value>,
    pub usage: Usage,
    pub usage_reported: bool,
    pub duration_ms: u64,
    /// Milliseconds from request start to the first non-empty provider output.
    pub first_output_ms: Option<u64>,
    /// The output stopped at the token limit (`max_tokens`, `length`, `max_output_tokens`):
    /// whatever it was writing, text or a tool call, is cut off.
    pub truncated: bool,
}

pub enum StreamEvent<'a> {
    TextDelta(&'a str),
    ReasoningDelta(&'a str),
    ReasoningEnd,
    /// Output with no user-visible text delta, e.g. tool names, arguments or opaque reasoning.
    OutputChunk,
}

/// One SSE event stream decoder per wire format.
pub trait Decoder {
    fn handle(&mut self, data: &str, on_text: &mut (dyn FnMut(StreamEvent<'_>) + Send)) -> Result<(), String>;
    fn done(&self) -> bool;
    fn finish(self) -> Result<Response, String>;
}

pub struct Client {
    http: reqwest::Client,
    cfg: Resolved,
    selector: String,
    /// Resolved on the first request so `!command` keys never delay startup.
    auth: tokio::sync::OnceCell<Vec<(&'static str, String)>>,
}

impl Client {
    pub fn new(cfg: Resolved) -> Result<Client, String> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if cfg.api == Api::AnthropicMessages {
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        for (k, v) in &cfg.headers {
            let name = HeaderName::from_bytes(k.as_bytes()).map_err(|e| format!("header `{k}`: {e}"))?;
            let mut value = HeaderValue::from_str(v).map_err(|e| format!("header `{k}`: {e}"))?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(300))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Client { selector: cfg.selector(), http, cfg, auth: tokio::sync::OnceCell::new() })
    }

    /// `provider/id[:level]`.
    pub fn name(&self) -> &str {
        &self.selector
    }

    /// The `models.yml` provider id this model belongs to.
    pub fn provider(&self) -> &str {
        &self.cfg.provider
    }

    pub fn fast_enabled(&self) -> bool { self.cfg.fast }

    /// Whether the next request sends `service_tier=fast`, including an explicit extraBody value.
    pub fn fast_active(&self) -> bool {
        if !crate::config::openai_fast_model(self.cfg.api, &self.cfg.id) { return false; }
        match self.cfg.compat.extra_body.get("service_tier").and_then(Value::as_str) {
            Some("fast") => true,
            Some(_) => false,
            None => self.cfg.fast,
        }
    }

    pub(crate) fn with_fast(&self, enabled: bool) -> Resolved {
        let mut cfg = self.cfg.clone();
        cfg.fast = enabled;
        cfg
    }

    pub fn display_name(&self) -> &str {
        self.cfg.display_name.as_deref().filter(|s| !s.is_empty()).unwrap_or(&self.cfg.id)
    }

    /// Estimate from the price snapshot captured when this client was resolved.
    pub fn estimate_cost(&self, usage: Usage) -> Option<f64> {
        self.cfg.price.clone().unwrap_or_default().cost(usage)
    }

    pub fn context_window(&self) -> u64 {
        self.cfg.context_window
    }

    /// Snapshot the exact model and cached credential for a private worker IPC channel.
    /// Never log or persist the returned value.
    pub async fn worker_snapshot(&self) -> Result<Resolved, String> {
        let headers = self.auth().await?;
        let mut cfg = self.cfg.clone();
        if cfg.key.is_some() {
            cfg.key = headers.iter().find_map(|(name, value)| match *name {
                "x-api-key" => Some(value.as_str()),
                "authorization" => value.strip_prefix("Bearer "),
                _ => None,
            }).map(|key| crate::config::Key::Value(key.to_owned()));
        }
        Ok(cfg)
    }

    /// Converts canonical tool definitions to the wire format once, at startup.
    pub fn wire_tools(&self, tools: Value) -> Value {
        match self.cfg.api {
            Api::AnthropicMessages => tools,
            Api::OpenaiCompletions => openai::tools(tools),
            Api::OpenaiResponses => responses::tools(tools),
        }
    }

    /// Opens the pooled TLS connection while the user is still typing.
    pub fn warm(&self) {
        let req = self.http.head(&self.cfg.base_url);
        tokio::spawn(async move {
            let _ = req.send().await;
        });
    }

    /// The provider's model id, without provider prefix or thinking level.
    pub fn model_id(&self) -> &str {
        &self.cfg.id
    }

    /// Speaks the Responses API, so it can try the provider's built-in image tool.
    pub fn on_responses(&self) -> bool {
        self.cfg.api == Api::OpenaiResponses
    }

    /// POSTs `body` to `{baseUrl}/{path}` with this model's credentials and headers and
    /// returns the JSON reply; for non-chat endpoints such as `images/generations`.
    pub async fn post_json(&self, path: &str, body: &Value, timeout: Duration, cancel: &CancellationToken) -> Result<Value, String> {
        let url = format!("{}/{}", self.cfg.base_url.trim_end_matches('/'), path.trim_start_matches('/'));
        let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        let mut req = self.http.post(&url).body(body).timeout(timeout);
        for (k, v) in self.auth().await? {
            req = req.header(*k, v);
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            r = req.send() => r.map_err(|e| format!("request failed: {}", chain(&e)))?,
        };
        let status = resp.status();
        let text = tokio::select! {
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            r = resp.text() => r.map_err(|e| format!("response failed: {}", chain(&e)))?,
        };
        if !status.is_success() {
            return Err(format!("API {status}: {}", error_message(&text)));
        }
        serde_json::from_str(&text).map_err(|e| format!("bad JSON response: {e}"))
    }

    /// Downloads `url` (an image a provider links to instead of inlining).
    pub async fn download(&self, url: &str, timeout: Duration, cancel: &CancellationToken) -> Result<Vec<u8>, String> {
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            r = self.http.get(url).timeout(timeout).send() => r.map_err(|e| format!("download failed: {}", chain(&e)))?,
        };
        if !resp.status().is_success() { return Err(format!("download failed: HTTP {}", resp.status())); }
        tokio::select! {
            _ = cancel.cancelled() => Err(CANCELLED.into()),
            r = resp.bytes() => r.map(|bytes| bytes.to_vec()).map_err(|e| format!("download failed: {}", chain(&e))),
        }
    }

    /// Anthropic keys go in `x-api-key`; OpenAI wires and `authHeader: true` use a bearer token.
    async fn auth(&self) -> Result<&[(&'static str, String)], String> {
        let cfg = &self.cfg;
        self.auth
            .get_or_try_init(|| async {
                let Some(key) = &cfg.key else { return Ok(Vec::new()) };
                let key = resolve_key(key).await?;
                let mut h = Vec::new();
                if cfg.api == Api::AnthropicMessages {
                    h.push(("x-api-key", key.clone()));
                }
                if cfg.api != Api::AnthropicMessages || cfg.auth_header {
                    h.push(("authorization", format!("Bearer {key}")));
                }
                Ok(h)
            })
            .await
            .map(Vec::as_slice)
    }

    /// Streams one model call. `tools` must come from `wire_tools`.
    /// `extra_user` appends a trailing user text without copying the history
    /// (compaction uses it, keeping `tools` unchanged so the cached prefix still hits).
    pub async fn stream(
        &self,
        system: &str,
        tools: &Value,
        messages: &[Value],
        extra_user: Option<&str>,
        cancel: &CancellationToken,
        mut on_text: impl FnMut(StreamEvent<'_>) + Send,
    ) -> Result<Response, String> {
        let started = Instant::now();
        let cfg = &self.cfg;
        let extra = &cfg.compat.extra_body;
        let unset = |k: &str| !extra.contains_key(k);
        let source = format!("{}/{}", cfg.provider, cfg.id);
        let body = match cfg.api {
            Api::AnthropicMessages => {
                let max_tokens = cfg.max_tokens.unwrap_or(MAX_TOKENS);
                serde_json::to_vec(&Body {
                    model: &cfg.id,
                    max_tokens: unset("max_tokens").then_some(max_tokens),
                    stream: true,
                    system: [TextBlock { r#type: "text", text: system, cache_control: EPHEMERAL }],
                    tools,
                    messages: Messages { messages, extra_user },
                    thinking: anthropic_thinking(cfg.level, max_tokens).filter(|_| unset("thinking")),
                    params: extra,
                })
            }
            Api::OpenaiCompletions => {
                let field = cfg.compat.max_tokens_field.as_deref().unwrap_or("max_tokens");
                let mut extra = Cow::Borrowed(extra);
                if let Some(n) = cfg.max_tokens.filter(|_| unset(field)) {
                    extra.to_mut().insert(field.into(), n.into());
                }
                if let Some(l) = cfg.level.filter(|_| unset("reasoning_effort")) {
                    extra.to_mut().insert("reasoning_effort".into(), effort(l).into());
                }
                insert_fast(&mut extra, cfg.api, &cfg.id, cfg.fast);
                let usage = cfg.compat.supports_usage_in_streaming.unwrap_or(true) && unset("stream_options");
                serde_json::to_vec(&openai::Body {
                    model: &cfg.id,
                    stream: true,
                    stream_options: usage.then_some(openai::StreamOptions { include_usage: true }),
                    tools,
                    messages: openai::Messages { system, history: messages, extra_user },
                    params: &extra,
                })
            }
            Api::OpenaiResponses => {
                let store = cfg.compat.supports_store.unwrap_or(true) && unset("store");
                let mut response_extra = Cow::Borrowed(extra);
                if let Some(reasoning) = extra.get("reasoning").and_then(Value::as_object)
                    && cfg.reasoning_capability != Some(false)
                    && !reasoning.contains_key("summary")
                    && !matches!(reasoning.get("effort").and_then(Value::as_str), Some("none" | "off"))
                    && (cfg.level != Some(Level::Off) || reasoning.get("effort").and_then(Value::as_str).is_some_and(|e| e != "none" && e != "off"))
                    && reasoning.get("enabled") != Some(&Value::Bool(false))
                    && reasoning.get("type").and_then(Value::as_str) != Some("disabled")
                {
                    let mut reasoning = reasoning.clone();
                    reasoning.insert("summary".into(), "auto".into());
                    response_extra.to_mut().insert("reasoning".into(), Value::Object(reasoning));
                }
                insert_fast(&mut response_extra, cfg.api, &cfg.id, cfg.fast);
                serde_json::to_vec(&responses::Body {
                    model: &cfg.id,
                    instructions: system,
                    input: responses::Input { history: messages, extra_user, source: &source },
                    tools,
                    stream: true,
                    store: store.then_some(false),
                    include: (store && unset("include")).then_some(["reasoning.encrypted_content"]),
                    reasoning: if unset("reasoning") {
                        let summary = cfg.reasoning_capability != Some(false) && cfg.level != Some(Level::Off)
                            && (cfg.reasoning_capability == Some(true) || cfg.level.is_some());
                        (cfg.level.is_some() || summary).then_some(responses::Reasoning {
                            effort: cfg.level.map(effort),
                            summary: summary.then_some("auto"),
                        })
                    } else { None },
                    max_output_tokens: cfg.max_tokens.filter(|_| unset("max_output_tokens")),
                    extra: &response_extra,
                })
            }
        }
        .map_err(|e| e.to_string())?;
        let mut req = self.http.post(&cfg.url).body(body);
        for (k, v) in self.auth().await? {
            req = req.header(*k, v);
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            r = req.send() => r.map_err(|e| format!("request failed: {}", chain(&e)))?,
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API {status}: {}", error_message(&text)));
        }
        let mut first_output_ms = None;
        let mut observe = |event: StreamEvent<'_>| {
            if first_output_ms.is_none() && match &event {
                StreamEvent::TextDelta(text) | StreamEvent::ReasoningDelta(text) => !text.is_empty(),
                StreamEvent::OutputChunk => true,
                StreamEvent::ReasoningEnd => false,
            } {
                first_output_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
            }
            if !matches!(event, StreamEvent::OutputChunk) { on_text(event); }
        };
        let mut response = match cfg.api {
            Api::AnthropicMessages => read_sse(resp, AnthropicStream::default(), cancel, &mut observe).await,
            Api::OpenaiCompletions => read_sse(resp, openai::Stream::default(), cancel, &mut observe).await,
            Api::OpenaiResponses => read_sse(resp, responses::Stream::new(&source), cancel, &mut observe).await,
        }?;
        response.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        response.first_output_ms = first_output_ms;
        Ok(response)
    }
}


fn insert_fast(extra: &mut Cow<'_, Map<String, Value>>, api: Api, id: &str, fast: bool) {
    if fast && crate::config::openai_fast_model(api, id) && !extra.contains_key("service_tier") {
        extra.to_mut().insert("service_tier".into(), "fast".into());
    }
}

/// Thinking levels are sent by name; `off` asks for the lowest effort providers accept.
fn effort(l: Level) -> &'static str {
    match l {
        Level::Off | Level::Minimal => "minimal",
        Level::Low => "low",
        Level::Medium => "medium",
        Level::High => "high",
        Level::Xhigh => "xhigh",
        Level::Max => "max",
    }
}

/// Budget-style extended thinking for Anthropic; the budget must stay below max_tokens.
fn anthropic_thinking(level: Option<Level>, max_tokens: u32) -> Option<Value> {
    let frac = match level? {
        Level::Off => return None,
        Level::Minimal => 8,
        Level::Low => 6,
        Level::Medium => 4,
        Level::High => 2,
        Level::Xhigh | Level::Max => 1,
    };
    let budget = (max_tokens / frac).min(max_tokens.saturating_sub(1024));
    (budget >= 1024).then(|| json!({ "type": "enabled", "budget_tokens": budget }))
}

async fn read_sse<D: Decoder>(resp: reqwest::Response, mut dec: D, cancel: &CancellationToken, on_text: &mut (dyn FnMut(StreamEvent<'_>) + Send)) -> Result<Response, String> {
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::<u8>::new();
    let mut data = String::new();
    while !dec.done() {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            c = stream.next() => match c {
                None => break,
                Some(Ok(b)) => b,
                Some(Err(e)) => return Err(format!("stream failed: {}", chain(&e))),
            },
        };
        buf.extend_from_slice(&chunk);
        let mut start = 0;
        while let Some(off) = buf[start..].iter().position(|&b| b == b'\n') {
            let line = &buf[start..start + off];
            start += off + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                if !data.is_empty() {
                    dec.handle(&data, on_text)?;
                    data.clear();
                }
            } else if let Some(d) = line.strip_prefix(b"data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(&String::from_utf8_lossy(d.strip_prefix(b" ").unwrap_or(d)));
            }
        }
        buf.drain(..start);
    }
    // A final event without the trailing blank line.
    if !data.is_empty() && !dec.done() {
        dec.handle(&data, on_text)?;
    }
    dec.finish()
}

#[derive(Default)]
struct AnthropicStream {
    content: Vec<Value>,
    json: Vec<String>,
    usage: Usage,
    input_reported: bool,
    output_reported: bool,
    done: bool,
    reasoning: bool,
    truncated: bool,
}

impl Decoder for AnthropicStream {
    fn handle(&mut self, data: &str, on_text: &mut (dyn FnMut(StreamEvent<'_>) + Send)) -> Result<(), String> {
        let ev: Value = serde_json::from_str(data).map_err(|e| format!("bad SSE data: {e}"))?;
        let idx = ev["index"].as_u64().unwrap_or(0) as usize;
        match ev["type"].as_str().unwrap_or("") {
            "message_start" => {
                let u = &ev["message"]["usage"];
                self.input_reported |= u["input_tokens"].as_u64().is_some();
                self.output_reported |= u["output_tokens"].as_u64().is_some();
                self.usage.merge(u);
            }
            "content_block_start" => {
                if self.reasoning {
                    on_text(StreamEvent::ReasoningEnd);
                    self.reasoning = false;
                }
                let mut block = ev["content_block"].clone();
                if block["type"] == "tool_use" {
                    if block["name"].as_str().is_some_and(|name| !name.is_empty()) { on_text(StreamEvent::OutputChunk); }
                    block["input"] = json!({});
                }
                if block["type"] == "thinking"
                    && let Some(thinking) = block["thinking"].as_str().filter(|s| !s.is_empty())
                {
                    on_text(StreamEvent::ReasoningDelta(thinking));
                    self.reasoning = true;
                }
                if block["type"] == "text" && let Some(text) = block["text"].as_str().filter(|s| !s.is_empty()) {
                    on_text(StreamEvent::TextDelta(text));
                } else if block["type"] == "redacted_thinking" && block["data"].as_str().is_some_and(|data| !data.is_empty()) {
                    on_text(StreamEvent::OutputChunk);
                }
                if self.content.len() <= idx {
                    self.content.resize(idx + 1, Value::Null);
                    self.json.resize(idx + 1, String::new());
                }
                self.content[idx] = block;
            }
            "content_block_delta" => {
                let Some(block) = self.content.get_mut(idx) else { return Ok(()) };
                let d = &ev["delta"];
                let s = |k: &str| d[k].as_str().unwrap_or("");
                match s("type") {
                    "text_delta" => {
                        if self.reasoning {
                            on_text(StreamEvent::ReasoningEnd);
                            self.reasoning = false;
                        }
                        append(block, "text", s("text"));
                        on_text(StreamEvent::TextDelta(s("text")));
                    }
                    "input_json_delta" => {
                        if self.reasoning {
                            on_text(StreamEvent::ReasoningEnd);
                            self.reasoning = false;
                        }
                        let data = s("partial_json");
                        if !data.is_empty() { on_text(StreamEvent::OutputChunk); }
                        self.json[idx].push_str(data);
                    }
                    "thinking_delta" if block["type"] == "thinking" => {
                        let text = s("thinking");
                        append(block, "thinking", text);
                        if !text.is_empty() {
                            on_text(StreamEvent::ReasoningDelta(text));
                            self.reasoning = true;
                        }
                    }
                    "signature_delta" => append(block, "signature", s("signature")),
                    _ => {}
                }
            }
            "content_block_stop" => {
                if self.reasoning {
                    on_text(StreamEvent::ReasoningEnd);
                    self.reasoning = false;
                }
                if let Some(block) = self.content.get_mut(idx)
                    && block["type"] == "tool_use"
                {
                    block["input"] = parse_input(std::mem::take(&mut self.json[idx]));
                }
            }
            "message_delta" => {
                let u = &ev["usage"];
                self.input_reported |= u["input_tokens"].as_u64().is_some();
                self.output_reported |= u["output_tokens"].as_u64().is_some();
                self.usage.merge(u);
                self.truncated |= ev["delta"]["stop_reason"] == "max_tokens";
            }
            "message_stop" => {
                if self.reasoning {
                    on_text(StreamEvent::ReasoningEnd);
                    self.reasoning = false;
                }
                self.done = true;
            }
            "error" => return Err(format!("API error: {}", ev["error"]["message"].as_str().unwrap_or(data))),
            _ => {}
        }
        Ok(())
    }

    fn done(&self) -> bool {
        self.done
    }

    fn finish(mut self) -> Result<Response, String> {
        if !self.done {
            return Err("stream ended before message_stop".into());
        }
        self.content.retain(|b| !b.is_null());
        Ok(Response { content: self.content, usage: self.usage, usage_reported: self.input_reported && self.output_reported, duration_ms: 0, first_output_ms: None, truncated: self.truncated })
    }
}

/// A `tool_result`'s text (prefixed `Error: ` when it failed) and its image blocks, for
/// wires whose tool outputs carry text only and send pictures in a following user turn.
pub(crate) fn tool_result_parts(block: &Value) -> (Cow<'_, str>, Vec<&Value>) {
    let (text, images): (Cow<'_, str>, Vec<&Value>) = match &block["content"] {
        Value::String(text) => (Cow::Borrowed(text.as_str()), Vec::new()),
        Value::Array(parts) => (
            Cow::Owned(parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")),
            parts.iter().filter(|p| p["type"] == "image").collect(),
        ),
        other => (Cow::Owned(other.to_string()), Vec::new()),
    };
    let text = if block["is_error"] == true { Cow::Owned(format!("Error: {text}")) } else { text };
    (text, images)
}

/// Parses accumulated tool-call JSON; output cut off by max_tokens is kept
/// under `_invalid_json` so the agent can report it to the model.
pub fn parse_input(raw: String) -> Value {
    if raw.trim().is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(&raw) {
        Ok(v @ Value::Object(_)) => v,
        _ => json!({ "_invalid_json": raw }),
    }
}

fn append(block: &mut Value, key: &str, s: &str) {
    match block.get_mut(key) {
        Some(Value::String(t)) => t.push_str(s),
        _ => block[key] = s.into(),
    }
}

fn chain(e: &dyn Error) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        out.push_str(": ");
        out.push_str(&s.to_string());
        src = s.source();
    }
    out
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).map(str::to_owned))
        .unwrap_or_else(|| body.chars().take(500).collect())
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    system: [TextBlock<'a>; 1],
    tools: &'a Value,
    messages: Messages<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
    #[serde(flatten)]
    params: &'a Map<String, Value>,
}

#[derive(Serialize)]
struct TextBlock<'a> {
    r#type: &'static str,
    text: &'a str,
    cache_control: Ephemeral,
}

#[derive(Serialize, Clone, Copy)]
struct Ephemeral {
    r#type: &'static str,
}

const EPHEMERAL: Ephemeral = Ephemeral { r#type: "ephemeral" };

/// Serializes history while adding the final cache breakpoint without copying images.
struct Messages<'a> {
    messages: &'a [Value],
    extra_user: Option<&'a str>,
}

impl Serialize for Messages<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Some((last, head)) = self.messages.split_last() else {
            let mut seq = s.serialize_seq(Some(self.extra_user.is_some() as usize))?;
            if let Some(text) = self.extra_user {
                seq.serialize_element(&cached(user_text(text)))?;
            }
            return seq.end();
        };
        let separate = self.extra_user.is_some() && last["role"] != "user";
        let mut seq = s.serialize_seq(Some(self.messages.len() + separate as usize))?;
        for m in head {
            match strip_unsigned(m) {
                Some(m) => seq.serialize_element(&m)?,
                None => seq.serialize_element(m)?,
            }
        }
        let stripped = strip_unsigned(last);
        let last = stripped.as_ref().unwrap_or(last);
        match self.extra_user {
            Some(text) if separate => {
                seq.serialize_element(last)?;
                seq.serialize_element(&cached(user_text(text)))?;
            }
            extra => seq.serialize_element(&CachedMessage { message: last, extra })?,
        }
        seq.end()
    }
}

struct CachedMessage<'a> {
    message: &'a Value,
    extra: Option<&'a str>,
}

impl Serialize for CachedMessage<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Some(map) = self.message.as_object() else { return self.message.serialize(s) };
        let mut out = s.serialize_map(Some(map.len()))?;
        for (key, value) in map {
            if key == "content" {
                if let Some(blocks) = value.as_array() {
                    out.serialize_entry(key, &CachedBlocks { blocks, extra: self.extra })?;
                } else {
                    out.serialize_entry(key, value)?;
                }
            } else {
                out.serialize_entry(key, value)?;
            }
        }
        out.end()
    }
}

struct CachedBlocks<'a> {
    blocks: &'a [Value],
    extra: Option<&'a str>,
}

impl Serialize for CachedBlocks<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.blocks.len() + self.extra.is_some() as usize))?;
        for (i, block) in self.blocks.iter().enumerate() {
            if self.extra.is_none() && i + 1 == self.blocks.len() {
                seq.serialize_element(&CachedBlock(block))?;
            } else {
                seq.serialize_element(block)?;
            }
        }
        if let Some(text) = self.extra {
            seq.serialize_element(&cached(user_text(text))["content"][0])?;
        }
        seq.end()
    }
}

struct CachedBlock<'a>(&'a Value);

impl Serialize for CachedBlock<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Some(map) = self.0.as_object() else { return self.0.serialize(s) };
        let mut out = s.serialize_map(Some(map.len() + usize::from(!map.contains_key("cache_control"))))?;
        for (key, value) in map {
            if key != "cache_control" {
                out.serialize_entry(key, value)?;
            }
        }
        out.serialize_entry("cache_control", &EPHEMERAL)?;
        out.end()
    }
}

/// A data URL serialized directly from the canonical image source.
pub(crate) struct DataUrl<'a> {
    media_type: &'a str,
    data: &'a str,
}

impl<'a> DataUrl<'a> {
    pub(crate) fn from_block(block: &'a Value) -> Result<Self, &'static str> {
        let source = &block["source"];
        if source["type"] != "base64" { return Err("unsupported image source"); }
        let media_type = source["media_type"].as_str().filter(|s| !s.is_empty()).ok_or("missing image media type")?;
        let data = source["data"].as_str().filter(|s| !s.is_empty()).ok_or("missing image data")?;
        Ok(Self { media_type, data })
    }
}

impl fmt::Display for DataUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "data:{};base64,{}", self.media_type, self.data)
    }
}

impl Serialize for DataUrl<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Reasoning from OpenAI-format models has no signature, which the Anthropic
/// API rejects; drop it when a session switches providers.
fn strip_unsigned(m: &Value) -> Option<Value> {
    let blocks = m["content"].as_array()?;
    let unsigned = |b: &Value| b["type"] == "reasoning_item" || (b["type"] == "thinking" && b.get("signature").is_none_or(|s| s.as_str().is_none_or(str::is_empty)));
    if !blocks.iter().any(unsigned) {
        return None;
    }
    let mut m = m.clone();
    if let Some(blocks) = m["content"].as_array_mut() {
        blocks.retain(|b| !unsigned(b));
        // The API rejects an empty assistant turn.
        if blocks.is_empty() {
            blocks.push(json!({ "type": "text", "text": "(reasoning omitted)" }));
        }
    }
    Some(m)
}

fn cached(mut msg: Value) -> Value {
    if let Some(block) = msg["content"].as_array_mut().and_then(|b| b.last_mut()) {
        block["cache_control"] = json!({ "type": "ephemeral" });
    }
    msg
}

pub fn user_text(text: &str) -> Value {
    json!({ "role": "user", "content": [{ "type": "text", "text": text }] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(api: Api, id: &str, fast: bool, preset: Option<&str>) -> Option<String> {
        let mut body = Map::new();
        if let Some(value) = preset { body.insert("service_tier".into(), value.into()); }
        let mut body = Cow::Borrowed(&body);
        insert_fast(&mut body, api, id, fast);
        body.get("service_tier").and_then(Value::as_str).map(str::to_owned)
    }

    #[test]
    fn openai_fast_sets_service_tier_only_for_gpt() {
        assert_eq!(tier(Api::OpenaiResponses, "gpt-6.1-sol", true, None).as_deref(), Some("fast"));
        assert_eq!(tier(Api::OpenaiCompletions, "GPT-5.4", true, None).as_deref(), Some("fast"));
        assert_eq!(tier(Api::OpenaiResponses, "openai/gpt-6.1-sol", true, None).as_deref(), Some("fast"));
        assert_eq!(tier(Api::OpenaiResponses, "gpt-6.1-sol", false, None), None);
        assert_eq!(tier(Api::OpenaiResponses, "grok-4.7", true, None), None);
        assert_eq!(tier(Api::OpenaiResponses, "grok/grok-4.7", true, None), None);
        assert_eq!(tier(Api::OpenaiResponses, "ft:gpt-4o", true, None), None);
        assert_eq!(tier(Api::AnthropicMessages, "claude-sonnet-5", true, None), None);
        assert_eq!(tier(Api::OpenaiResponses, "gpt-6.1-sol", true, Some("priority")).as_deref(), Some("priority"));
        assert_eq!(tier(Api::OpenaiResponses, "gpt-6.1-sol", false, Some("fast")).as_deref(), Some("fast"));
    }

    #[test]
    fn tool_result_images_follow_as_user_input_on_openai_wires() {
        let image = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}});
        let history = [
            json!({"role": "user", "content": [{"type": "text", "text": "look"}]}),
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "r1", "name": "read", "input": {"path": "a.png"}}]}),
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "r1", "content": [{"type": "text", "text": "a.png: 2×2"}, image], "is_error": false}]}),
        ];
        let chat = serde_json::to_value(openai::Messages { system: "s", history: &history, extra_user: None }).unwrap();
        let chat = chat.as_array().unwrap();
        let tool = chat.iter().position(|m| m["role"] == "tool").unwrap();
        assert_eq!(chat[tool]["content"], "a.png: 2×2");
        assert_eq!(chat[tool + 1]["role"], "user");
        assert_eq!(chat[tool + 1]["content"][0]["image_url"]["url"], "data:image/png;base64,AAAA");

        let items = serde_json::to_value(responses::Input { history: &history, extra_user: None, source: "p/m" }).unwrap();
        let items = items.as_array().unwrap();
        let output = items.iter().position(|i| i["type"] == "function_call_output").unwrap();
        assert_eq!(items[output]["output"], "a.png: 2×2");
        assert_eq!(items[output + 1]["content"][0]["type"], "input_image");
    }
}
