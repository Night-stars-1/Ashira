use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::ChildStdin,
    sync::{Mutex, mpsc::UnboundedSender, oneshot},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
    pub retryable: bool,
}

pub enum Spec {
    Stdio { command: String, args: Vec<String>, env: Vec<(String, String)>, cwd: PathBuf },
    Http { url: String, headers: Vec<(String, String)> },
    Sse { url: String, headers: Vec<(String, String)> },
}

/// Answers a server->client request: `Ok(result)` or `Err((code, message))`.
pub type RequestHandler = dyn Fn(&str, &Value) -> Result<Value, (i64, String)> + Send + Sync;

pub struct Hooks {
    pub notifications: UnboundedSender<(String, Value)>,
    pub on_request: Arc<RequestHandler>,
}

type Pending = StdMutex<HashMap<u64, oneshot::Sender<Result<Value, Error>>>>;
struct State {
    hooks: Hooks,
    pending: Pending,
    next: AtomicU64,
    closed: CancellationToken,
    closing: AtomicBool,
    kind: Kind,
}
enum Kind {
    Stdio { stdin: Mutex<Option<ChildStdin>>, tree: Option<crate::tools::Tree>, stderr: Arc<StdMutex<Vec<u8>>> },
    Http { client: reqwest::Client, url: String, headers: Vec<(String, String)>, session: StdMutex<Option<String>>, version: StdMutex<Option<String>>, listening: AtomicBool },
    Sse { client: reqwest::Client, endpoint: String, headers: Vec<(String, String)> },
}
pub struct Transport {
    state: Arc<State>,
}

fn err(message: impl Into<String>, retryable: bool) -> Error {
    Error { message: message.into(), retryable }
}
fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn fail(state: &State, error: Error) {
    state.closed.cancel();
    for (_, tx) in lock(&state.pending).drain() {
        let _ = tx.send(Err(error.clone()));
    }
}
fn resolve(command: &str) -> String {
    #[cfg(windows)]
    if std::path::Path::new(command).extension().is_none() && !command.contains(['/', '\\']) {
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            for ext in ["exe", "cmd", "bat"] {
                let p = dir.join(format!("{command}.{ext}"));
                if p.is_file() {
                    return p.to_string_lossy().into_owned();
                }
            }
        }
    }
    command.to_owned()
}
fn client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).build().map_err(|e| err(e.to_string(), false))
}
fn configured(mut request: reqwest::RequestBuilder, headers: &[(String, String)]) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    request
}
fn http_request(state: &State, method: reqwest::Method) -> reqwest::RequestBuilder {
    let Kind::Http { client, url, headers, session, version, .. } = &state.kind else { unreachable!() };
    let mut req = client.request(method, url);
    let session = lock(session).clone();
    let version = lock(version).clone();
    for (name, value) in headers {
        if !(name.eq_ignore_ascii_case("mcp-session-id") && session.is_some())
            && !(name.eq_ignore_ascii_case("mcp-protocol-version") && version.is_some())
            && !name.eq_ignore_ascii_case("accept")
            && !name.eq_ignore_ascii_case("content-type")
        {
            req = req.header(name.as_str(), value.as_str());
        }
    }
    if let Some(s) = session {
        req = req.header("Mcp-Session-Id", s);
    }
    if let Some(v) = version {
        req = req.header("MCP-Protocol-Version", v);
    }
    req
}
fn capture_session(state: &State, response: &reqwest::Response) {
    if let Kind::Http { session, .. } = &state.kind
        && let Some(value) = response.headers().get("Mcp-Session-Id").and_then(|v| v.to_str().ok())
    {
        *lock(session) = Some(value.to_owned());
    }
}
async fn check_status(state: &State, response: reqwest::Response, had_session: bool) -> Result<reqwest::Response, Error> {
    capture_session(state, &response);
    if response.status().is_success() {
        return Ok(response);
    }
    if response.status() == reqwest::StatusCode::NOT_FOUND && had_session {
        return Err(err("MCP session expired", true));
    }
    let status = response.status();
    let auth = response.headers().get(reqwest::header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let body = response.text().await.unwrap_or_default();
    let body: String = body.trim().chars().take(500).collect();
    let suffix = auth.map(|a| format!(" (WWW-Authenticate: {a})")).unwrap_or_default();
    Err(err(format!("HTTP {status}: {body}{suffix}"), false))
}
fn is_sse(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/event-stream"))
}

#[derive(Default)]
struct Event {
    event: String,
    data: String,
    id: String,
    has_data: bool,
}
struct Decoder {
    buf: Vec<u8>,
    event: Event,
}
impl Decoder {
    fn new() -> Self {
        Self { buf: Vec::new(), event: Event::default() }
    }
    fn line(&mut self, bytes: &[u8]) -> Option<Event> {
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        if bytes.is_empty() {
            let event = std::mem::take(&mut self.event);
            return event.has_data.then_some(event);
        }
        if bytes[0] == b':' {
            return None;
        }
        let split = bytes.iter().position(|b| *b == b':').unwrap_or(bytes.len());
        let field = &bytes[..split];
        let value = if split < bytes.len() { &bytes[split + 1..] } else { &[] };
        let value = value.strip_prefix(b" ").unwrap_or(value);
        let value = String::from_utf8_lossy(value);
        match field {
            b"data" => {
                if self.event.has_data {
                    self.event.data.push('\n');
                }
                self.event.has_data = true;
                self.event.data.push_str(&value);
            }
            b"event" => self.event.event = value.into_owned(),
            b"id" => self.event.id = value.into_owned(),
            _ => {}
        }
        None
    }
    fn push(&mut self, chunk: &[u8]) -> Vec<Event> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.buf.drain(..=end).collect();
            if let Some(event) = self.line(&line[..end]) {
                events.push(event);
            }
        }
        events
    }
    fn finish(&mut self) -> Option<Event> {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            if let Some(e) = self.line(&line) {
                return Some(e);
            }
        }
        let event = std::mem::take(&mut self.event);
        event.has_data.then_some(event)
    }
}

fn dispatch(state: &Arc<State>, msg: Value) {
    if let Value::Array(items) = msg {
        for item in items {
            dispatch(state, item);
        }
        return;
    }
    let Some(object) = msg.as_object() else { return };
    if let Some(method) = object.get("method").and_then(Value::as_str) {
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = object.get("id") {
            let reply = match (state.hooks.on_request)(method, &params) {
                Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
                Err((code, message)) => json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}}),
            };
            let state = state.clone();
            tokio::spawn(async move {
                let _ = send(&state, reply, false, None).await;
            });
        } else {
            let _ = state.hooks.notifications.send((method.to_owned(), params));
        }
    } else if (object.contains_key("result") || object.contains_key("error"))
        && let Some(id) = object.get("id").and_then(Value::as_u64)
        && let Some(tx) = lock(&state.pending).remove(&id)
    {
        let result = if let Some(e) = object.get("error") {
            Err(err(format!("MCP error {}: {}", e.get("code").unwrap_or(&Value::Null), e.get("message").and_then(Value::as_str).unwrap_or("")), false))
        } else {
            Ok(object.get("result").cloned().unwrap_or(Value::Null))
        };
        let _ = tx.send(result);
    }
}
fn dispatch_event(state: &Arc<State>, event: Event) {
    if let Ok(msg) = serde_json::from_str::<Value>(&event.data) {
        dispatch(state, msg);
    }
}
async fn consume_sse(state: Arc<State>, response: reqwest::Response) {
    let mut stream = response.bytes_stream();
    let mut decoder = Decoder::new();
    loop {
        let chunk = tokio::select! { _ = state.closed.cancelled() => return, chunk = stream.next() => chunk };
        match chunk {
            Some(Ok(bytes)) => {
                for event in decoder.push(&bytes) {
                    dispatch_event(&state, event);
                }
            }
            _ => break,
        }
    }
    if let Some(event) = decoder.finish() {
        dispatch_event(&state, event);
    }
}

fn send<'a>(state: &'a Arc<State>, msg: Value, expect_response: bool, written: Option<&'a AtomicBool>) -> std::pin::Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
        if state.closed.is_cancelled() {
            return Err(err("transport closed", true));
        }
        let response_id = if expect_response { msg.get("id").and_then(Value::as_u64) } else { None };
        let body = serde_json::to_vec(&msg).map_err(|e| err(e.to_string(), false))?;
        match &state.kind {
            Kind::Stdio { stdin, .. } => {
                let mut stdin = stdin.lock().await;
                let Some(stdin) = stdin.as_mut() else {
                    return Err(err("transport closed", true));
                };
                let mut line = body;
                line.push(b'\n');
                stdin.write_all(&line).await.map_err(|e| err(format!("MCP write failed: {e}"), false))?;
                if let Some(written) = written {
                    written.store(true, Ordering::Release);
                }
                stdin.flush().await.map_err(|e| err(format!("MCP write failed: {e}"), false))
            }
            Kind::Http { session, .. } => {
                let had_session = lock(session).is_some()
                    || match &state.kind {
                        Kind::Http { headers, .. } => headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("mcp-session-id")),
                        _ => false,
                    };
                let response = http_request(state, reqwest::Method::POST)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
                    .body(body)
                    .send()
                    .await
                    .map_err(|e| err(e.to_string(), e.is_connect()))?;
                let response = check_status(state, response, had_session).await?;
                if is_sse(&response) {
                    let state = state.clone();
                    tokio::spawn(async move {
                        consume_sse(state.clone(), response).await;
                        if let Some(id) = response_id
                            && let Some(tx) = lock(&state.pending).remove(&id)
                        {
                            let _ = tx.send(Err(err("HTTP SSE stream closed", false)));
                        }
                    });
                } else if expect_response {
                    let bytes = response.bytes().await.map_err(|e| err(e.to_string(), false))?;
                    let msg = serde_json::from_slice(&bytes).map_err(|e| err(format!("invalid MCP response: {e}"), false))?;
                    dispatch(state, msg);
                }
                Ok(())
            }
            Kind::Sse { client, endpoint, headers } => {
                let response = configured(client.post(endpoint), headers)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .send()
                    .await
                    .map_err(|e| err(e.to_string(), e.is_connect()))?;
                check_status(state, response, false).await?;
                Ok(())
            }
        }
    })
}

impl Transport {
    pub async fn connect(spec: Spec, hooks: Hooks, timeout: Option<Duration>) -> Result<Transport, Error> {
        match spec {
            Spec::Stdio { command, args, env, cwd } => {
                #[cfg(windows)]
                let program = {
                    let command = command.clone();
                    tokio::task::spawn_blocking(move || resolve(&command)).await.map_err(|e| err(e.to_string(), true))?
                };
                #[cfg(not(windows))]
                let program = resolve(&command);
                let mut cmd = crate::tools::command(program);
                cmd.args(args).envs(env).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| err(format!("{command}: {e}"), true))?;
                let tree = crate::tools::Tree::attach(&child);
                let stdin = child.stdin.take().ok_or_else(|| err("missing stdin", false))?;
                let stdout = child.stdout.take().ok_or_else(|| err("missing stdout", false))?;
                let mut stderr = child.stderr.take().ok_or_else(|| err("missing stderr", false))?;
                let tail = Arc::new(StdMutex::new(Vec::new()));
                let state = Arc::new(State {
                    hooks,
                    pending: StdMutex::new(HashMap::new()),
                    next: AtomicU64::new(1),
                    closed: CancellationToken::new(),
                    closing: AtomicBool::new(false),
                    kind: Kind::Stdio { stdin: Mutex::new(Some(stdin)), tree, stderr: tail.clone() },
                });
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = stderr.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let mut tail = lock(&tail);
                        tail.extend_from_slice(&buf[..n]);
                        let excess = tail.len().saturating_sub(2000);
                        tail.drain(..excess);
                    }
                });
                let s = state.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stdout);
                    let mut line = Vec::new();
                    loop {
                        tokio::select! {
                            _ = s.closed.cancelled() => break,
                            result = reader.read_until(b'\n', &mut line) => match result { Ok(0) | Err(_) => break, Ok(_) => { if let Ok(msg) = serde_json::from_slice(&line) { dispatch(&s, msg); } line.clear(); } },
                            status = child.wait() => {
                                let status = status.ok().map(|s| s.to_string());
                                let tail = match &s.kind { Kind::Stdio { stderr, .. } => String::from_utf8_lossy(&lock(stderr)).into_owned(), _ => String::new() };
                                fail(&s, err(format!("MCP server exited{}: {}", status.map(|v| format!(" ({v})")).unwrap_or_default(), tail), false));
                                return;
                            }
                        }
                    }
                    if !s.closed.is_cancelled() {
                        let status = child.try_wait().ok().flatten().map(|s| format!(" ({s})")).unwrap_or_default();
                        let tail = match &s.kind {
                            Kind::Stdio { stderr, .. } => String::from_utf8_lossy(&lock(stderr)).into_owned(),
                            _ => String::new(),
                        };
                        fail(&s, err(format!("MCP server exited{status}: {tail}"), false));
                    }
                });
                Ok(Transport { state })
            }
            Spec::Http { url, headers } => Ok(Transport {
                state: Arc::new(State {
                    hooks,
                    pending: StdMutex::new(HashMap::new()),
                    next: AtomicU64::new(1),
                    closed: CancellationToken::new(),
                    closing: AtomicBool::new(false),
                    kind: Kind::Http { client: client()?, url, headers, session: StdMutex::new(None), version: StdMutex::new(None), listening: AtomicBool::new(false) },
                }),
            }),
            Spec::Sse { url, headers } => {
                let client = client()?;
                let base = reqwest::Url::parse(&url).map_err(|e| err(e.to_string(), false))?;
                let connect = async {
                    let response =
                        configured(client.get(&url), &headers).header(reqwest::header::ACCEPT, "text/event-stream").send().await.map_err(|e| err(e.to_string(), e.is_connect()))?;
                    if !response.status().is_success() {
                        return Err(err(format!("HTTP {}", response.status()), false));
                    }
                    let mut stream = response.bytes_stream();
                    let mut decoder = Decoder::new();
                    loop {
                        let bytes = stream.next().await.ok_or_else(|| err("legacy SSE stream closed", false))?.map_err(|e| err(e.to_string(), false))?;
                        let mut events = decoder.push(&bytes).into_iter();
                        while let Some(event) = events.next() {
                            if event.event == "endpoint" {
                                let endpoint = base.join(event.data.trim()).map_err(|e| err(e.to_string(), false))?;
                                if endpoint.origin() != base.origin() {
                                    return Err(err("legacy SSE endpoint has different origin", false));
                                }
                                let state = Arc::new(State {
                                    hooks,
                                    pending: StdMutex::new(HashMap::new()),
                                    next: AtomicU64::new(1),
                                    closed: CancellationToken::new(),
                                    closing: AtomicBool::new(false),
                                    kind: Kind::Sse { client: client.clone(), endpoint: endpoint.to_string(), headers: headers.clone() },
                                });
                                for event in events {
                                    dispatch_event(&state, event);
                                }
                                let s = state.clone();
                                tokio::spawn(async move {
                                    loop {
                                        let item = tokio::select! { _ = s.closed.cancelled() => return, item = stream.next() => item };
                                        match item {
                                            Some(Ok(bytes)) => {
                                                for event in decoder.push(&bytes) {
                                                    if event.event != "endpoint" {
                                                        dispatch_event(&s, event);
                                                    }
                                                }
                                            }
                                            _ => break,
                                        }
                                    }
                                    if let Some(event) = decoder.finish()
                                        && event.event != "endpoint"
                                    {
                                        dispatch_event(&s, event);
                                    }
                                    fail(&s, err("legacy SSE stream closed", false));
                                });
                                return Ok(Transport { state });
                            }
                        }
                    }
                };
                match timeout {
                    Some(t) => tokio::time::timeout(t, connect).await.map_err(|_| err("legacy SSE connection timed out", false))?,
                    None => connect.await,
                }
            }
        }
    }
    pub async fn request(&self, method: &str, params: Value, timeout: Option<Duration>, cancel: &CancellationToken) -> Result<Value, Error> {
        if self.state.closed.is_cancelled() {
            return Err(err("transport closed", true));
        }
        let id = self.state.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = lock(&self.state.pending);
            if self.state.closed.is_cancelled() {
                return Err(err("transport closed", true));
            }
            pending.insert(id, tx);
        }
        let sent = Arc::new(AtomicBool::new(false));
        let operation = async {
            send(&self.state, json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}), true, Some(&sent)).await?;
            rx.await.unwrap_or_else(|_| Err(err("transport closed", false)))
        };
        let deadline = async {
            match timeout {
                Some(t) => tokio::time::sleep(t).await,
                None => std::future::pending::<()>().await,
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(err(crate::api::CANCELLED, false)),
            _ = deadline => Err(err(format!("MCP `{method}` timed out"), false)),
            result = operation => result,
        };
        lock(&self.state.pending).remove(&id);
        if result.as_ref().err().is_some_and(|e| e.message == crate::api::CANCELLED || e.message == format!("MCP `{method}` timed out"))
            && method != "initialize"
            && sent.load(Ordering::Acquire)
            && matches!(&self.state.kind, Kind::Stdio { .. })
        {
            let state = self.state.clone();
            tokio::spawn(async move {
                let _ = send(&state, json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"cancelled by user"}}), false, None).await;
            });
        }
        result
    }
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        send(&self.state, json!({"jsonrpc":"2.0","method":method,"params":params}), false, None).await
    }
    pub fn set_protocol_version(&self, version: &str) {
        if let Kind::Http { version: v, .. } = &self.state.kind {
            *lock(v) = Some(version.to_owned());
        }
    }
    pub fn start_listener(&self) {
        let Kind::Http { listening, .. } = &self.state.kind else { return };
        if listening.swap(true, Ordering::AcqRel) || self.state.closed.is_cancelled() {
            return;
        }
        let state = self.state.clone();
        tokio::spawn(async move {
            let response = tokio::select! { _ = state.closed.cancelled() => return, r = http_request(&state, reqwest::Method::GET).header(reqwest::header::ACCEPT, "text/event-stream").send() => r };
            if let Ok(response) = response {
                capture_session(&state, &response);
                if response.status().is_success() && is_sse(&response) {
                    consume_sse(state, response).await;
                }
            }
        });
    }
    pub fn closed(&self) -> CancellationToken {
        self.state.closed.clone()
    }
    pub async fn close(&self) {
        if self.state.closing.swap(true, Ordering::AcqRel) {
            return;
        }
        fail(&self.state, err("transport closed", false));
        match &self.state.kind {
            Kind::Stdio { stdin, tree, .. } => {
                if let Some(tree) = tree {
                    tree.kill();
                }
                stdin.lock().await.take();
            }
            Kind::Http { session, .. } => {
                let has_session = lock(session).is_some();
                if has_session {
                    let _ = tokio::time::timeout(Duration::from_secs(2), http_request(&self.state, reqwest::Method::DELETE).send()).await;
                }
            }
            _ => {}
        }
    }
    pub fn kind(&self) -> &'static str {
        match &self.state.kind {
            Kind::Stdio { .. } => "stdio",
            Kind::Http { .. } => "http",
            Kind::Sse { .. } => "sse",
        }
    }
}
