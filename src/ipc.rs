//! Versioned, private JSONL transport for worker processes.
//! Initialization may contain credentials and must never be logged.

use std::{collections::HashMap, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader}, sync::{mpsc, oneshot}};
use tokio_util::sync::CancellationToken;

const VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

async fn read_frame(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Envelope>, String> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|_| "worker IPC read failed".to_owned())?;
        if available.is_empty() {
            return if frame.is_empty() { Ok(None) } else { Err("worker IPC ended in a partial frame".into()) };
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let length = newline.unwrap_or(available.len());
        if length > MAX_FRAME_BYTES.saturating_sub(frame.len()) { return Err("worker IPC frame exceeds size limit".into()); }
        frame.extend_from_slice(&available[..length]);
        reader.consume(length + usize::from(newline.is_some()));
        if newline.is_some() {
            let envelope = serde_json::from_slice(&frame).map_err(|_| "invalid worker IPC frame".to_owned())?;
            return Ok(Some(envelope));
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope { version: u32, frame: Frame }

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Init { data: Value },
    Request { id: u64, method: String, params: Value },
    Reply { id: u64, result: Result<Value, String> },
    Progress { activity: String },
    Finished { result: Result<String, String> },
    Cancel,
}

type OutgoingFrame = (Frame, Option<oneshot::Sender<Result<(), String>>>);

struct Inner {
    outgoing: mpsc::UnboundedSender<OutgoingFrame>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next: AtomicU64,
    closed: CancellationToken,
    reason: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct Connection(Arc<Inner>);

impl Connection {
    /// Exactly one writer serializes frames; the independent reader handles replies
    /// while requests or tools are waiting. `incoming` carries non-reply frames.
    pub fn new<R, W>(reader: R, mut writer: W) -> (Self, mpsc::UnboundedReceiver<Frame>)
    where R: AsyncRead + Unpin + Send + 'static, W: AsyncWrite + Unpin + Send + 'static {
        let (outgoing, mut rx) = mpsc::unbounded_channel::<OutgoingFrame>();
        let (tx, incoming) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner { outgoing, pending: Mutex::new(HashMap::new()), next: AtomicU64::new(1), closed: CancellationToken::new(), reason: Mutex::new(None) });
        let reader_inner = inner.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let reason = loop {
                let envelope = match read_frame(&mut reader).await {
                    Ok(Some(frame)) => frame,
                    Ok(None) => break "worker IPC reached EOF".to_owned(),
                    Err(error) => break error,
                };
                if envelope.version != VERSION { break "unsupported worker IPC version".to_owned(); }
                match envelope.frame {
                    Frame::Reply { id, result } => {
                        let reply = reader_inner.pending.lock().unwrap().remove(&id);
                        if let Some(reply) = reply { let _ = reply.send(result); }
                    }
                    frame => if tx.send(frame).is_err() { break "worker IPC receiver closed".to_owned(); },
                }
            };
            *reader_inner.reason.lock().unwrap() = Some(reason);
            reader_inner.closed.cancel();
            reader_inner.pending.lock().unwrap().clear();
        });
        let writer_inner = inner.clone();
        tokio::spawn(async move {
            loop {
                let (frame, ack) = tokio::select! { _ = writer_inner.closed.cancelled() => break, item = rx.recv() => match item { Some(item) => item, None => break } };
                let result = async {
                    let mut bytes = serde_json::to_vec(&Envelope { version: VERSION, frame }).map_err(|_| "worker IPC serialization failed".to_owned())?;
                    if bytes.len() > MAX_FRAME_BYTES { return Err("worker IPC frame exceeds size limit".to_owned()); }
                    bytes.push(b'\n');
                    writer.write_all(&bytes).await.map_err(|_| "worker IPC write failed".to_owned())?;
                    writer.flush().await.map_err(|_| "worker IPC flush failed".to_owned())
                }.await;
                let failed = result.is_err();
                if let Some(ack) = ack { let _ = ack.send(result); }
                if failed { break; }
            }
            writer_inner.closed.cancel();
            writer_inner.pending.lock().unwrap().clear();
        });
        (Self(inner), incoming)
    }

    pub fn closed(&self) -> CancellationToken { self.0.closed.clone() }

    pub fn close_reason(&self) -> String {
        self.0.reason.lock().unwrap().clone().unwrap_or_else(|| "worker IPC closed".into())
    }

    pub fn send(&self, frame: Frame) -> Result<(), String> {
        if self.0.closed.is_cancelled() { return Err("worker IPC closed".into()); }
        self.0.outgoing.send((frame, None)).map_err(|_| "worker IPC closed".into())
    }

    /// Wait for the frame to be flushed before exiting a worker process.
    pub async fn send_flushed(&self, frame: Frame) -> Result<(), String> {
        if self.0.closed.is_cancelled() { return Err("worker IPC closed".into()); }
        let (tx, rx) = oneshot::channel();
        self.0.outgoing.send((frame, Some(tx))).map_err(|_| "worker IPC closed".to_owned())?;
        rx.await.unwrap_or_else(|_| Err("worker IPC closed".into()))
    }

    pub async fn request(&self, method: &str, params: Value, cancel: &CancellationToken) -> Result<Value, String> {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.0.pending.lock().unwrap().insert(id, tx);
        if let Err(error) = self.send(Frame::Request { id, method: method.into(), params }) {
            self.0.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        let result = tokio::select! {
            result = rx => result.unwrap_or_else(|_| Err(self.close_reason())),
            _ = cancel.cancelled() => Err("cancelled".into()),
            _ = self.0.closed.cancelled() => Err(self.close_reason()),
        };
        self.0.pending.lock().unwrap().remove(&id);
        result
    }
}

/// An execution permit owned by one worker run. Waiting for a message or a
/// human decision must release it, then reacquire before executing more work.
pub struct RunSlot {
    manager: Arc<crate::subagents::Manager>,
    endpoint: crate::subagents::Endpoint,
    permit: Mutex<Option<tokio::sync::OwnedSemaphorePermit>>,
    acquisition: tokio::sync::Mutex<()>,
}

impl RunSlot {
    pub async fn acquire(manager: Arc<crate::subagents::Manager>, endpoint: crate::subagents::Endpoint, cancel: &CancellationToken) -> Result<Arc<Self>, String> {
        let slot = Arc::new(Self { manager, endpoint, permit: Mutex::new(None), acquisition: tokio::sync::Mutex::new(()) });
        slot.resume(cancel).await?;
        Ok(slot)
    }

    pub fn suspend(&self) { self.permit.lock().unwrap().take(); }

    pub async fn resume(&self, cancel: &CancellationToken) -> Result<(), String> {
        let _gate = tokio::select! { _ = cancel.cancelled() => return Err("cancelled".into()), gate = self.acquisition.lock() => gate };
        if cancel.is_cancelled() { return Err("cancelled".into()); }
        if !self.manager.endpoint_current(&self.endpoint) { return Err("stale worker endpoint".into()); }
        if self.permit.lock().unwrap().is_some() { return Ok(()); }
        let permit = self.manager.slot(cancel).await.ok_or_else(|| "cancelled".to_owned())?;
        if cancel.is_cancelled() { return Err("cancelled".into()); }
        if !self.manager.endpoint_current(&self.endpoint) { return Err("stale worker endpoint".into()); }
        *self.permit.lock().unwrap() = Some(permit);
        Ok(())
    }
}

/// Supervise one generation. The dispatcher owns committing the returned result.
pub async fn run_worker(shared: Arc<crate::agent::Shared>, run: crate::subagents::RunLease, scope: Option<Arc<Mutex<crate::agent::StatsScope>>>) -> Result<String, String> {
    use std::process::Stdio;
    let manager = shared.subagents.clone();
    let cancel = &run.cancel;
    let slot = RunSlot::acquire(manager.clone(), run.endpoint.clone(), cancel).await?;
    let init = tokio::select! {
        _ = cancel.cancelled() => return Err("cancelled".into()),
        result = crate::agent::worker_init(&shared, &run) => result?,
    };
    if cancel.is_cancelled() { return Err("cancelled".into()); }
    if !manager.endpoint_current(&run.endpoint) { return Err("stale worker endpoint".into()); }
    let executable = std::env::current_exe().map_err(|e| format!("worker executable: {e}"))?;
    let mut command = crate::tools::command(executable);
    command.current_dir(shared.cwd());
    command.arg("--worker-stdio").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("worker spawn failed: {e}"))?;
    let guard = match crate::process_guard::ProcessGuard::attach(&child) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(format!("worker process ownership failed: {error}"));
        }
    };
    let mut stderr_task = child.stderr.take().map(|mut stderr| tokio::spawn(async move {
        // Do not retain stderr content: it may contain credentials or private data.
        // Continue draining even after the bounded diagnostic counter saturates.
        let mut count = 0usize;
        let mut buffer = [0u8; 4096];
        loop {
            match stderr.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(n) => count = count.saturating_add(n).min(1024 * 1024),
            }
        }
        count
    }));
    manager.set_worker_pid(&run.endpoint, child.id());
    let rpc_cancel = CancellationToken::new();
    let mut handlers = tokio::task::JoinSet::new();
    let mut pending_stats: HashMap<String, (crate::stats::RequestStats, std::time::Instant)> = HashMap::new();
    let result = async {
        if cancel.is_cancelled() { return Err("cancelled".into()); }
        if !manager.endpoint_current(&run.endpoint) { return Err("stale worker endpoint".into()); }
        let stdout = child.stdout.take().ok_or("worker stdout unavailable")?;
        let stdin = child.stdin.take().ok_or("worker stdin unavailable")?;
        let (connection, mut incoming) = Connection::new(stdout, stdin);
        let closed = connection.closed();
        tokio::select! {
            _ = cancel.cancelled() => return Err("cancelled".into()),
            result = connection.send_flushed(Frame::Init { data: init }) => result?,
        }
        manager.update(&run.endpoint, crate::subagents::Status::Running, "starting");
        let finished = loop {
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled() => { let _ = connection.send(Frame::Cancel); return Err("cancelled".into()); },
                frame = incoming.recv() => frame.ok_or_else(|| connection.close_reason())?,
                joined = handlers.join_next(), if !handlers.is_empty() => {
                    if let Some(Err(_)) = joined { return Err("worker RPC handler failed".into()); }
                    continue;
                },
                _ = closed.cancelled() => match incoming.try_recv() { Ok(frame) => frame, Err(_) => return Err(connection.close_reason()) },
            };
            match frame {
                Frame::Request { id, method, params } => {
                    if method == "stats_begin" {
                        let parsed = (|| {
                            let request_id = params["id"].as_str()?.to_owned();
                            let model = params["model"].as_str()?.to_owned();
                            let kind = serde_json::from_value(params["kind"].clone()).ok()?;
                            Some(crate::stats::RequestStats { id: request_id, model, kind, usage: None, cost: None, duration_ms: 0, child: true })
                        })();
                        let result = if let Some(record) = parsed {
                            pending_stats.insert(record.id.clone(), (record, std::time::Instant::now()));
                            Ok(Value::Null)
                        } else { Err("invalid worker statistics request".into()) };
                        let _ = connection.send(Frame::Reply { id, result });
                        continue;
                    }
                    if method == "stats_finish" {
                        let result = match serde_json::from_value::<crate::stats::RequestStats>(params) {
                            Ok(mut record) if pending_stats.contains_key(&record.id) => {
                                record.child = true;
                                pending_stats.remove(&record.id);
                                if let Some(scope) = &scope { shared.record_statistics(scope, &record); }
                                Ok(Value::Null)
                            }
                            _ => Err("invalid worker statistics settlement".into()),
                        };
                        let _ = connection.send(Frame::Reply { id, result });
                        continue;
                    }
                    let shared = shared.clone();
                    let run = run.clone();
                    let slot = slot.clone();
                    let connection = connection.clone();
                    let rpc_cancel = rpc_cancel.clone();
                    handlers.spawn(async move {
                        let result = crate::agent::handle_worker_request(shared, &run, slot, &method, params, &rpc_cancel).await;
                        let _ = connection.send(Frame::Reply { id, result });
                    });
                }
                Frame::Progress { activity } => manager.update(&run.endpoint, crate::subagents::Status::Running, &activity),
                Frame::Finished { result } => {
                    break result;
                }
                _ => return Err("unexpected worker IPC frame".into()),
            }
        };
        let status = tokio::select! {
            _ = cancel.cancelled() => return Err("cancelled".into()),
            status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()) => status.map_err(|_| "worker did not exit after result".to_owned())?.map_err(|e| format!("worker wait failed: {e}"))?,
        };
        if !status.success() { return Err(format!("worker exited abnormally: {status}")); }
        finished
    }.await;
    if let Some(scope) = &scope {
        for (_, (mut record, started)) in pending_stats {
            record.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            shared.record_statistics(scope, &record);
        }
    }
    // This token is deliberately distinct from the user cancellation token:
    // worker crashes cancel active tools but remain failed runs, not cancellations.
    rpc_cancel.cancel();
    drop(guard);
    let _ = child.start_kill();
    let _ = child.wait().await;
    if tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while handlers.join_next().await.is_some() {}
    }).await.is_err() {
        handlers.abort_all();
        while handlers.join_next().await.is_some() {}
    }
    manager.set_worker_pid(&run.endpoint, None);
    let stderr_count = if let Some(task) = &mut stderr_task {
        match tokio::time::timeout(std::time::Duration::from_secs(1), &mut *task).await {
            Ok(Ok(count)) => count,
            _ => { task.abort(); 0 }
        }
    } else { 0 };
    match result {
        Err(error) if stderr_count > 0 && !cancel.is_cancelled() => Err(format!("{error} (worker stderr emitted {}{} bytes; content suppressed)", if stderr_count == 1024 * 1024 { "at least " } else { "" }, stderr_count)),
        result => result,
    }
}

/// Entry point selected before CLI parsing, config loading, MCP, or UI startup.
pub async fn worker_main() -> Result<(), String> {
    let (connection, mut incoming) = Connection::new(tokio::io::stdin(), tokio::io::stdout());
    let closed = connection.closed();
    let init = tokio::select! {
        biased;
        frame = incoming.recv() => match frame { Some(Frame::Init { data }) => data, _ => return Err("worker expected initialization".into()) },
        _ = closed.cancelled() => return Err("worker parent disconnected before initialization".into()),
    };
    let cancel = CancellationToken::new();
    let work = crate::agent::worker_loop(connection.clone(), init, cancel.clone());
    tokio::pin!(work);
    let result = tokio::select! {
        biased;
        result = &mut work => result,
        frame = incoming.recv() => match frame {
            Some(Frame::Cancel) | None => { cancel.cancel(); return Err("worker parent disconnected or cancelled".into()); },
            _ => { cancel.cancel(); return Err("unexpected parent IPC frame".into()); },
        },
        _ = closed.cancelled() => { cancel.cancel(); return Err("worker parent disconnected".into()); },
    };
    connection.send_flushed(Frame::Finished { result }).await
}
