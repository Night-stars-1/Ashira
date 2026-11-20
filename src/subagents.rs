use std::{collections::{HashMap, VecDeque}, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}, time::{Instant, SystemTime, UNIX_EPOCH}};
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use crate::{agent::{Event, Model}, session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role { Task, Scout }
impl Role { pub fn as_str(self) -> &'static str { match self { Self::Task => "task", Self::Scout => "scout" } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Queued, Running, WaitingApproval, WaitingMessage, Cancelling, Completed, Failed, Cancelled }
impl Status {
    pub fn as_str(self) -> &'static str { match self { Self::Queued => "queued", Self::Running => "running", Self::WaitingApproval => "waiting_approval", Self::WaitingMessage => "waiting_message", Self::Cancelling => "cancelling", Self::Completed => "completed", Self::Failed => "failed", Self::Cancelled => "cancelled" } }
    pub fn active(self) -> bool { matches!(self, Self::Queued | Self::Running | Self::WaitingApproval | Self::WaitingMessage | Self::Cancelling) }
}
pub struct Identity { pub id: String, pub name: String, pub role: Role }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint { pub owner: u64, pub id: String, pub generation: u64 }
#[derive(Clone)]
pub struct RunLease { pub endpoint: Endpoint, pub identity: Arc<Identity>, pub model: Arc<Model>, pub context: Arc<str>, pub task: Arc<str>, pub transcript: Arc<PathBuf>, pub cancel: CancellationToken, pub resume: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerMessage { pub id: String, pub owner: u64, pub from: String, pub from_name: String, pub from_role: String, pub to: String, pub body: String }
#[derive(Clone)]
pub struct Snapshot { pub identity: Arc<Identity>, pub task: Arc<str>, pub model: Arc<str>, pub activity: Arc<str>, pub status: Status, pub elapsed_ms: u64, pub transcript: Arc<PathBuf>, pub error: Option<Arc<str>>, pub generation: u64, pub worker_pid: Option<u32>, pub pending_messages: usize, pub closed: bool }
#[derive(Clone)]
pub struct Completion { pub owner: u64, pub identity: Arc<Identity>, pub status: Status, pub elapsed_ms: u64, pub error: Option<Arc<str>>, pub generation: u64 }
struct Job { snapshot: Snapshot, started: Instant, cancel: CancellationToken, model: Arc<Model>, context: Arc<str>, report: PathBuf, finishing: bool, inbox: VecDeque<PeerMessage> }
struct Receipt { from: String, to: String, status: &'static str }
struct State { owner: u64, jobs: Vec<Job>, pending: VecDeque<Value>, root_inbox: VecDeque<PeerMessage>, receipts: HashMap<String, Receipt>, launches: VecDeque<RunLease>, suppressed: bool, suppressed_through: Option<u64>, retired: bool, handles: Vec<tokio::task::JoinHandle<()>> }
pub struct Manager { state: Mutex<State>, notify: Notify, slots: Arc<Semaphore>, root: CancellationToken, events: tokio::sync::mpsc::UnboundedSender<Event> }
static NEXT: AtomicU64 = AtomicU64::new(0);
fn unique() -> String { let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(); format!("{n:x}-{:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)) }

/// Recognize the scheme even when malformed so it can never become a file path.
pub fn is_agent_uri(path: &str) -> bool { path.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("agent:")) }
/// Strictly parse an agent URI; URI components are opaque identifiers, not paths.
pub fn parse_agent_uri(path: &str) -> Result<Vec<&str>, String> {
    let rest = path.strip_prefix("agent://").ok_or("invalid agent URI")?;
    if rest.is_empty() { return Ok(Vec::new()); }
    let parts: Vec<_> = rest.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) { return Err("invalid agent URI".into()); }
    Ok(parts)
}
impl Manager {
    pub fn new(root: CancellationToken, events: tokio::sync::mpsc::UnboundedSender<Event>) -> Self { Self { state: Mutex::new(State { owner: 0, jobs: Vec::new(), pending: VecDeque::new(), root_inbox: VecDeque::new(), receipts: HashMap::new(), launches: VecDeque::new(), suppressed: false, suppressed_through: None, retired: false, handles: Vec::new() }), notify: Notify::new(), slots: Arc::new(Semaphore::new(8)), root, events } }
    fn changed(&self) { self.notify.notify_waiters(); self.notify.notify_one(); let _ = self.events.send(Event::SubagentsChanged); }
    pub(crate) fn signal(&self) { self.changed(); }
    pub fn owner(&self) -> u64 { self.state.lock().unwrap().owner }
    pub fn root_endpoint(&self) -> Endpoint { Endpoint { owner: self.owner(), id: "parent".into(), generation: 0 } }
    fn valid(s: &State, e: &Endpoint) -> bool { !s.retired && s.owner == e.owner && if e.id == "parent" { e.generation == 0 } else { s.jobs.iter().any(|j| j.snapshot.identity.id == e.id && j.snapshot.generation == e.generation && j.snapshot.status.active() && !j.snapshot.closed && !j.cancel.is_cancelled()) } }
    pub fn endpoint_current(&self, e: &Endpoint) -> bool { Self::valid(&self.state.lock().unwrap(), e) }
    pub fn dismiss_completion_notices(&self) { let mut s = self.state.lock().unwrap(); s.suppressed_through = Some(s.owner); }
    pub fn accepts_completion(&self, c: &Completion) -> bool { let s = self.state.lock().unwrap(); !s.retired && s.owner == c.owner && s.suppressed_through.is_none_or(|o| c.owner > o) }
    pub fn accepts_message_notice(&self, m: &PeerMessage) -> bool { let s = self.state.lock().unwrap(); !s.retired && s.owner == m.owner && m.to == "parent" && s.suppressed_through.is_none_or(|o| m.owner > o) }
    pub fn auto_resume_suppressed(&self) -> bool { self.state.lock().unwrap().suppressed }
    pub fn snapshots(&self) -> Vec<Snapshot> { let s = self.state.lock().unwrap(); if s.retired { return Vec::new(); } s.jobs.iter().map(|j| { let mut v = j.snapshot.clone(); v.pending_messages = j.inbox.len(); if v.status.active() { v.elapsed_ms = j.started.elapsed().as_millis().min(u64::MAX as u128) as u64; } v }).collect() }
    pub fn hud(&self) -> ([usize; 4], Vec<Snapshot>) {
        let s = self.state.lock().unwrap();
        let mut counts = [0; 4];
        if s.retired { return (counts, Vec::new()); }
        for j in &s.jobs {
            counts[match j.snapshot.status { Status::Queued => 0, Status::Running => 1, Status::WaitingApproval | Status::WaitingMessage => 2, Status::Cancelling => 3, _ => continue }] += 1;
        }
        let visible = s.jobs.iter().rev().filter(|j| j.snapshot.status == Status::Running)
            .chain(s.jobs.iter().rev().filter(|j| j.snapshot.status.active() && j.snapshot.status != Status::Running))
            .take(3).map(|j| { let mut snapshot = j.snapshot.clone(); snapshot.pending_messages = j.inbox.len(); snapshot.elapsed_ms = j.started.elapsed().as_millis().min(u64::MAX as u128) as u64; snapshot }).collect();
        (counts, visible)
    }
    pub fn active_count(&self) -> usize { let s = self.state.lock().unwrap(); if s.retired { 0 } else { s.jobs.iter().filter(|j| j.snapshot.status.active()).count() } }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if s.retired { return Err(format!("unknown job `{id}`")); }
        let owner = s.owner;
        let j = s.jobs.iter_mut().find(|j| j.snapshot.identity.id == id).ok_or_else(|| format!("unknown job `{id}`"))?;
        j.snapshot.closed = true;
        let queued = j.snapshot.status == Status::Queued;
        if j.snapshot.status.active() { j.snapshot.status = Status::Cancelling; j.cancel.cancel(); }
        let ids: Vec<_> = j.inbox.drain(..).map(|m| m.id).collect();
        let completion = if queued {
            j.snapshot.status = Status::Cancelled;
            j.snapshot.activity = Arc::from("cancelled");
            let c = Completion { owner, identity: j.snapshot.identity.clone(), status: Status::Cancelled, elapsed_ms: 0, error: None, generation: j.snapshot.generation };
            let value = json!({"id":id,"name":c.identity.name,"agent":c.identity.role.as_str(),"generation":c.generation,"status":"cancelled","report":"","report_path":j.report,"transcript":j.snapshot.transcript.as_ref(),"error":null});
            Some((c, value))
        } else { None };
        for id in ids { if let Some(r) = s.receipts.get_mut(&id) { r.status = "cancelled"; } }
        if let Some((_, value)) = &completion { s.pending.push_back(value.clone()); }
        drop(s);
        if let Some((c, _)) = completion && self.accepts_completion(&c) { let _ = self.events.send(Event::SubagentFinished(c)); }
        self.changed(); Ok(())
    }
    pub async fn transcript(&self, id: &str) -> Result<String, String> { let path = { let s = self.state.lock().unwrap(); if s.retired { return Err(format!("unknown job `{id}`")); } s.jobs.iter().find(|j| j.snapshot.identity.id == id).map(|j| j.snapshot.transcript.clone()).ok_or_else(|| format!("unknown job `{id}`"))? }; tokio::fs::read_to_string(path.as_ref()).await.map_err(|e| e.to_string()) }
    pub(crate) fn create(&self, owner: u64, name: String, role: Role, task: String, context: Arc<str>, model: Arc<Model>) -> Result<Arc<Identity>, String> { let mut s = self.state.lock().unwrap(); if s.owner != owner { return Err("stale owner".into()); } s.retired = false; let id = unique(); let identity = Arc::new(Identity { id: id.clone(), name, role }); let dir = session::home().join("subagents").join(&id); let transcript = Arc::new(dir.join("transcript.txt")); let task: Arc<str> = task.into(); let cancel = self.root.child_token(); let snapshot = Snapshot { identity: identity.clone(), task: task.clone(), model: Arc::from(model.client.name()), activity: Arc::from("queued"), status: Status::Queued, elapsed_ms: 0, transcript: transcript.clone(), error: None, generation: 1, worker_pid: None, pending_messages: 0, closed: false }; let endpoint = Endpoint { owner, id, generation: 1 }; s.launches.push_back(RunLease { endpoint, identity: identity.clone(), model: model.clone(), context: context.clone(), task, transcript, cancel: cancel.clone(), resume: false }); s.jobs.push(Job { snapshot, started: Instant::now(), cancel, model, context, report: dir.join("report-1.txt"), finishing: false, inbox: VecDeque::new() }); drop(s); self.changed(); Ok(identity) }
    fn queue_resume(s: &mut State, index: usize, root: &CancellationToken) { let j = &mut s.jobs[index]; if j.snapshot.closed || j.snapshot.status.active() || j.inbox.is_empty() { return; } j.snapshot.generation += 1; j.snapshot.status = Status::Queued; j.snapshot.activity = Arc::from("queued"); j.snapshot.error = None; j.snapshot.elapsed_ms = 0; j.snapshot.worker_pid = None; j.started = Instant::now(); j.finishing = false; j.cancel = root.child_token(); j.report = j.snapshot.transcript.parent().unwrap().join(format!("report-{}.txt", j.snapshot.generation)); let lease = RunLease { endpoint: Endpoint { owner: s.owner, id: j.snapshot.identity.id.clone(), generation: j.snapshot.generation }, identity: j.snapshot.identity.clone(), model: j.model.clone(), context: j.context.clone(), task: j.snapshot.task.clone(), transcript: j.snapshot.transcript.clone(), cancel: j.cancel.clone(), resume: true }; s.launches.push_back(lease); }
    pub(crate) fn take_launches(&self) -> Vec<RunLease> { let mut s = self.state.lock().unwrap(); let launches = std::mem::take(&mut s.launches); launches.into_iter().filter(|l| Self::valid(&s, &l.endpoint) && !l.cancel.is_cancelled()).collect() }
    pub(crate) fn track(&self, handle: tokio::task::JoinHandle<()>) { self.state.lock().unwrap().handles.push(handle); }
    pub(crate) async fn slot(&self, cancel: &CancellationToken) -> Option<tokio::sync::OwnedSemaphorePermit> { tokio::select! { _ = cancel.cancelled() => None, p = self.slots.clone().acquire_owned() => p.ok() } }
    pub(crate) fn update(&self, e: &Endpoint, status: Status, activity: &str) { let mut s = self.state.lock().unwrap(); if Self::valid(&s, e) && let Some(j) = s.jobs.iter_mut().find(|j| j.snapshot.identity.id == e.id) { j.snapshot.status = status; j.snapshot.activity = Arc::from(activity); } drop(s); self.changed(); }
    pub(crate) fn set_worker_pid(&self, e: &Endpoint, pid: Option<u32>) { let mut s = self.state.lock().unwrap(); if !s.retired && s.owner == e.owner && let Some(j) = s.jobs.iter_mut().find(|j| j.snapshot.identity.id == e.id && j.snapshot.generation == e.generation) { j.snapshot.worker_pid = pid; } drop(s); self.changed(); }
    pub(crate) async fn finish(&self, lease: &RunLease, result: Result<String, String>) { let e = &lease.endpoint; let report_path = { let mut s = self.state.lock().unwrap(); if s.retired || s.owner != e.owner { return; } let Some(j) = s.jobs.iter_mut().find(|j| j.snapshot.identity.id == e.id && j.snapshot.generation == e.generation && j.snapshot.status.active() && !j.finishing) else { return }; j.finishing = true; j.report.clone() }; let (report, error) = match result { Ok(r) => (r, None), Err(e) => (String::new(), Some(e)) }; if let Some(dir) = report_path.parent() { let _ = tokio::fs::create_dir_all(dir).await; } let write = tokio::fs::write(&report_path, &report).await; let report = crate::tools::truncate(report, write.is_ok().then_some(report_path.as_path())); let completion = { let mut s = self.state.lock().unwrap(); if s.retired || s.owner != e.owner { return; } let Some(index) = s.jobs.iter().position(|j| j.snapshot.identity.id == e.id && j.snapshot.generation == e.generation && j.snapshot.status.active() && j.finishing) else { return }; let j = &mut s.jobs[index]; let error = error.or_else(|| write.err().map(|e| format!("report persistence failed: {e}"))); j.snapshot.status = if j.cancel.is_cancelled() { Status::Cancelled } else if error.is_some() { Status::Failed } else { Status::Completed }; j.snapshot.worker_pid = None; j.snapshot.error = error.as_deref().map(Arc::from); j.snapshot.elapsed_ms = j.started.elapsed().as_millis().min(u64::MAX as u128) as u64; j.snapshot.activity = Arc::from(j.snapshot.status.as_str()); let completion = Completion { owner: e.owner, identity: j.snapshot.identity.clone(), status: j.snapshot.status, elapsed_ms: j.snapshot.elapsed_ms, error: j.snapshot.error.clone(), generation: e.generation }; let value = json!({"id":e.id,"name":completion.identity.name,"agent":completion.identity.role.as_str(),"generation":e.generation,"status":completion.status.as_str(),"report":report,"report_path":report_path,"transcript":lease.transcript.as_ref(),"error":error}); s.pending.push_back(value); if completion.status == Status::Completed { Self::queue_resume(&mut s, index, &self.root); } completion }; if self.accepts_completion(&completion) { let _ = self.events.send(Event::SubagentFinished(completion)); } self.changed(); }
    pub(crate) fn metadata(&self, ids: Option<&[String]>) -> Result<Vec<Value>, String> {
        let s = self.state.lock().unwrap();
        if s.retired {
            return if ids.is_some_and(|ids| !ids.is_empty()) { Err("unknown job in retired owner".into()) } else { Ok(Vec::new()) };
        }
        if let Some(ids) = ids {
            for id in ids {
                if !s.jobs.iter().any(|j| j.snapshot.identity.id == *id) { return Err(format!("unknown job `{id}`")); }
            }
        }
        Ok(s.jobs.iter().filter(|j| ids.is_none_or(|ids| ids.contains(&j.snapshot.identity.id))).map(|j| json!({"id":j.snapshot.identity.id,"name":j.snapshot.identity.name,"agent":j.snapshot.identity.role.as_str(),"model":j.snapshot.model.as_ref(),"status":j.snapshot.status.as_str(),"generation":j.snapshot.generation,"worker_pid":j.snapshot.worker_pid,"pending_messages":j.inbox.len(),"closed":j.snapshot.closed,"transcript":j.snapshot.transcript.as_ref(),"report_path":j.report})).collect())
    }
    pub(crate) async fn wait(&self, e: &Endpoint, ids: Option<&[String]>, cancel: &CancellationToken) -> Result<Vec<Value>, String> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.endpoint_current(e) { return Err("stale endpoint".into()); }
            if ids.is_some_and(|ids| ids.iter().any(|id| id == &e.id)) { return Err("cannot wait for self".into()); }
            let jobs = self.metadata(ids)?;
            let (pending, mail) = {
                let s = self.state.lock().unwrap();
                (s.pending.iter().any(|v| ids.is_none_or(|ids| v["id"].as_str().is_some_and(|id| ids.iter().any(|i| i == id)))),
                 if e.id == "parent" { !s.root_inbox.is_empty() } else { s.jobs.iter().find(|j| j.snapshot.identity.id == e.id).is_some_and(|j| !j.inbox.is_empty()) })
            };
            let active = jobs.iter().any(|j| matches!(j["status"].as_str(), Some("queued" | "running" | "waiting_approval" | "waiting_message" | "cancelling")));
            let settled = jobs.iter().any(|j| matches!(j["status"].as_str(), Some("completed" | "failed" | "cancelled")));
            if mail || (e.id == "parent" && (pending || !active)) || (e.id != "parent" && ids.is_some() && (settled || !active)) { return Ok(jobs); }
            tokio::select! { _ = cancel.cancelled() => return Err("cancelled".into()), _ = &mut notified => {} }
        }
    }
    pub fn inbox(&self, e: &Endpoint) -> Result<Vec<PeerMessage>, String> { let s = self.state.lock().unwrap(); if !Self::valid(&s, e) { return Err("stale endpoint".into()); } Ok(if e.id == "parent" { s.root_inbox.iter().cloned().collect() } else { s.jobs.iter().find(|j| j.snapshot.identity.id == e.id).unwrap().inbox.iter().cloned().collect() }) }
    pub fn has_messages(&self, e: &Endpoint) -> bool { let s = self.state.lock().unwrap(); Self::valid(&s, e) && if e.id == "parent" { !s.root_inbox.is_empty() } else { s.jobs.iter().find(|j| j.snapshot.identity.id == e.id).is_some_and(|j| !j.inbox.is_empty()) } }
    pub fn ack_messages(&self, e: &Endpoint, ids: &[String]) -> Result<(), String> { let mut s = self.state.lock().unwrap(); if !Self::valid(&s, e) { return Err("stale endpoint".into()); } let inbox = if e.id == "parent" { &mut s.root_inbox } else { &mut s.jobs.iter_mut().find(|j| j.snapshot.identity.id == e.id).unwrap().inbox }; let mut acked = Vec::new(); inbox.retain(|m| { if ids.contains(&m.id) { acked.push(m.id.clone()); false } else { true } }); for id in acked { if let Some(r) = s.receipts.get_mut(&id) { r.status = "delivered"; } } drop(s); self.changed(); Ok(()) }
    pub fn send_message(&self, e: &Endpoint, target: &str, body: &str) -> Result<Value, String> { if body.len() > 64 * 1024 { return Err("message exceeds 65536 bytes".into()); } let mut s = self.state.lock().unwrap(); if !Self::valid(&s, e) { return Err("stale endpoint".into()); } let (name, role) = if e.id == "parent" { ("parent".to_string(), "parent".to_string()) } else { let j = s.jobs.iter().find(|j| j.snapshot.identity.id == e.id).unwrap(); (j.snapshot.identity.name.clone(), j.snapshot.identity.role.as_str().to_string()) }; if target == "all" && e.id != "parent" { return Err("broadcast is parent-only".into()); } let targets: Vec<String> = if target == "all" { s.jobs.iter().filter(|j| !j.snapshot.closed && j.snapshot.status != Status::Cancelling && j.snapshot.status != Status::Cancelled).map(|j| j.snapshot.identity.id.clone()).collect() } else { vec![target.to_string()] }; for t in &targets { if t == "parent" { continue; } let j = s.jobs.iter().find(|j| j.snapshot.identity.id == *t).ok_or_else(|| format!("unknown agent `{t}`"))?; if j.snapshot.closed || j.snapshot.status == Status::Cancelling || j.snapshot.status == Status::Cancelled { return Err(format!("agent `{t}` is closed")); } } let mut results = Vec::new(); let mut notices = Vec::new(); for t in targets { let m = PeerMessage { id: unique(), owner: e.owner, from: e.id.clone(), from_name: name.clone(), from_role: role.clone(), to: t.clone(), body: body.into() }; results.push(json!({"id":m.id,"from":m.from,"to":m.to,"status":"accepted"})); s.receipts.insert(m.id.clone(), Receipt { from: m.from.clone(), to: m.to.clone(), status: "accepted" }); if t == "parent" { s.root_inbox.push_back(m.clone()); notices.push(m); } else { let index = s.jobs.iter().position(|j| j.snapshot.identity.id == t).unwrap(); s.jobs[index].inbox.push_back(m); Self::queue_resume(&mut s, index, &self.root); } } drop(s); for m in notices { if self.accepts_message_notice(&m) { let _ = self.events.send(Event::PeerMessage(m)); } } self.changed(); if target == "all" { Ok(json!({"messages":results})) } else { Ok(results.remove(0)) } }
    pub async fn read_agent(&self, e: &Endpoint, uri: &str) -> Result<String, String> { let parts = parse_agent_uri(uri)?; if !self.endpoint_current(e) { return Err("stale endpoint".into()); } match parts.as_slice() { [] => { let mut jobs = self.metadata(None)?; jobs.insert(0, json!({"id":"parent","name":"parent","status":"running","generation":0})); Ok(json!(jobs).to_string()) }, ["messages", id] => { let s = self.state.lock().unwrap(); let r = s.receipts.get(*id).ok_or("unknown message")?; if r.from != e.id && r.to != e.id { return Err("receipt access denied".into()); } Ok(json!({"id":id,"from":r.from,"to":r.to,"status":r.status}).to_string()) }, [id, "transcript"] if *id != "parent" => self.transcript(id).await, ["parent"] => { let s = self.state.lock().unwrap(); Ok(json!({"id":"parent","name":"parent","status":"running","generation":0,"pending_messages":s.root_inbox.len(),"closed":false}).to_string()) }, [id] => { let jobs = self.metadata(Some(&[id.to_string()]))?; Ok(jobs[0].to_string()) }, _ => Err("invalid agent URI".into()) } }
    pub(crate) fn drain(&self) -> Vec<Value> { self.state.lock().unwrap().pending.drain(..).collect() }
    pub(crate) fn take_pending(&self) -> (Vec<Value>, bool, bool) { let mut s = self.state.lock().unwrap(); let resume = !s.suppressed && (s.pending.iter().any(|v| v["status"] != "cancelled") || !s.root_inbox.is_empty()); let active = !s.retired && s.jobs.iter().any(|j| j.snapshot.status.active()); (s.pending.drain(..).collect(), resume, active) }
    pub(crate) fn suppress(&self, yes: bool) { self.state.lock().unwrap().suppressed = yes; }
    pub(crate) fn resumable(&self) -> bool { let s = self.state.lock().unwrap(); !s.suppressed && (s.pending.iter().any(|v| v["status"] != "cancelled") || !s.root_inbox.is_empty()) }
    pub(crate) async fn changed_wait(&self) { self.notify.notified().await; }
    pub(crate) async fn retire(&self) { let handles = { let mut s = self.state.lock().unwrap(); s.owner += 1; s.pending.clear(); s.root_inbox.clear(); s.receipts.clear(); s.launches.clear(); s.suppressed = true; s.retired = true; for j in &s.jobs { j.cancel.cancel(); } std::mem::take(&mut s.handles) }; self.changed(); for handle in handles { let _ = handle.await; } { let mut s = self.state.lock().unwrap(); s.jobs.clear(); s.retired = false; } self.changed(); }
}
