//! Append-only JSONL sessions under `~/.ashira/sessions/<cwd-slug>/<id>.jsonl`.
//! Records: `meta`, `message`, `usage`, `request_stats`, `compact`, `fast` and a stats format boundary.

use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{api::Usage, config::Registry, stats::{Ledger, RequestStats}};

#[derive(Serialize)]
struct MessageRecord<'a> {
    r#type: &'static str,
    message: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<&'a str>,
}

#[derive(Clone)]
pub struct Session {
    pub path: PathBuf,
    writer: Arc<Mutex<Writer>>,
}

struct Writer {
    file: Option<File>,
    meta: Option<Value>,
    needs_stats_boundary: bool,
}

pub struct Loaded {
    pub session: Session,
    pub history: Vec<Value>,
    pub reply_stats: HashMap<usize, ReplyStats>,
    pub usage: Usage,
    pub todo: crate::todo::State,
    pub ledger: Ledger,
    /// Model from `meta` or the last `/models` switch.
    pub model: Option<String>,
    /// Last `/fast` choice. Older sessions and records without it stay off.
    pub fast: bool,
}

pub struct ReplyStats {
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub cost: Option<f64>,
    pub duration_ms: Option<u64>,
}

impl From<RequestStats> for ReplyStats {
    fn from(stats: RequestStats) -> Self {
        Self { model: Some(stats.model), usage: stats.usage, cost: stats.cost, duration_ms: Some(stats.duration_ms) }
    }
}

pub struct Target {
    pub cwd: PathBuf,
    pub path: PathBuf,
}

/// Read-only snapshot of the append-only records, including pre-compaction messages.
pub struct Replay {
    lines: std::io::Lines<BufReader<std::io::Take<File>>>,
}

impl Replay {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { lines: BufReader::new(file.take(len)).lines() })
    }

    pub fn next(&mut self) -> std::io::Result<Option<Value>> {
        for line in self.lines.by_ref() {
            let line = line?;
            if let Ok(record) = serde_json::from_str::<Value>(&line) { return Ok(Some(record)); }
        }
        Ok(None)
    }
}

pub fn home() -> PathBuf {
    env::var_os("ASHIRA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")).unwrap_or_else(|| ".".into());
        PathBuf::from(home).join(".ashira")
    })
}

fn project_dir(cwd: &Path) -> PathBuf {
    let slug: String = cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    home().join("sessions").join(slug.trim_matches('-'))
}

impl Session {
    /// Creates a session; nothing touches the disk until the first record.
    pub fn new(cwd: &Path, model: &str) -> Session {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = now.as_secs();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!("{}-{:x}-{:x}-{:x}", timestamp(secs), std::process::id(), now.as_nanos(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        Session { path: project_dir(cwd).join(format!("{id}.jsonl")), writer: Arc::new(Mutex::new(Writer { file: None, meta: Some(json!({ "type": "meta", "cwd": cwd, "model": model, "created": secs })), needs_stats_boundary: true })) }
    }

    pub fn id(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn message(&mut self, msg: &Value) {
        self.append(&MessageRecord { r#type: "message", message: msg, request_id: None });
    }

    pub fn reply(&mut self, msg: &Value, request_id: &str) {
        self.append(&MessageRecord { r#type: "message", message: msg, request_id: Some(request_id) });
    }

    pub fn message_checked(&mut self, msg: &Value) -> Result<(), String> {
        self.append_checked(&MessageRecord { r#type: "message", message: msg, request_id: None }).map_err(|error| format!("session persistence failed: {error}"))
    }

    pub fn todo_checked(&mut self, snapshot: &crate::todo::Snapshot) -> Result<(), String> {
        self.append_checked(&json!({"type": "todo", "snapshot": snapshot})).map_err(|error| format!("session persistence failed: {error}"))
    }

    pub fn usage(&mut self, u: &Usage) {
        self.append(&json!({
            "type": "usage",
            "input": u.input, "output": u.output,
            "cache_read": u.cache_read, "cache_write": u.cache_write,
        }));
    }

    pub fn request_stats(&mut self, stats: &RequestStats) {
        // `at` (unix seconds) lets the all-sessions report bucket usage by day.
        let at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.append(&json!({ "type": "request_stats", "stats": stats, "at": at }));
    }

    pub fn compact(&mut self, summary: &Value) {
        self.append(&MessageRecord { r#type: "compact", message: summary, request_id: None });
    }

    /// Records a model switch; `--resume` continues with the last one.
    pub fn model(&mut self, name: &str) {
        self.append(&json!({ "type": "model", "model": name }));
    }

    /// Persists the OpenAI fast choice. The last record wins on resume.
    pub fn fast(&mut self, enabled: bool) {
        self.append(&json!({ "type": "fast", "enabled": enabled }));
    }

    pub fn persist(&mut self) -> Result<(), String> {
        self.open_writer().map(|_| ()).map_err(|error| format!("session persistence failed: {error}"))
    }

    /// Best-effort: a failing disk must not break the conversation.
    fn append(&mut self, record: &impl Serialize) { let _ = self.append_checked(record); }

    fn append_checked(&mut self, record: &impl Serialize) -> std::io::Result<()> {
        self.open_writer()?.write_line(record)
    }

    fn open_writer(&mut self) -> std::io::Result<std::sync::MutexGuard<'_, Writer>> {
        let mut writer = self.writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if writer.file.is_none() {
            if let Some(dir) = self.path.parent() { fs::create_dir_all(dir)?; }
            writer.file = Some(OpenOptions::new().create(true).append(true).open(&self.path)?);
        }
        if let Some(meta) = writer.meta.take()
            && let Err(error) = writer.write_line(&meta)
        { writer.meta = Some(meta); return Err(error); }
        if writer.needs_stats_boundary {
            writer.write_line(&json!({ "type": "request_stats_boundary" }))?;
            writer.needs_stats_boundary = false;
        }
        Ok(writer)
    }
}

impl Writer {
    fn write_line(&mut self, value: &impl Serialize) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        line.push(b'\n');
        self.file.as_mut().ok_or_else(|| std::io::Error::other("session file is not open"))?.write_all(&line)
    }
}

/// Resolves `--resume [id|path]`: empty means the newest session for `cwd`.
pub fn find(cwd: &Path, arg: &str) -> Result<PathBuf, String> {
    let as_path = Path::new(arg);
    if !arg.is_empty() && as_path.is_file() {
        return Ok(as_path.to_path_buf());
    }
    let dir = project_dir(cwd);
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(&dir).map_err(|_| format!("no sessions in {}", dir.display()))?.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.ends_with(".jsonl") || !name.starts_with(arg) {
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, p)| p).ok_or_else(|| format!("no session matching `{arg}` in {}", dir.display()))
}

/// Unlike CLI resume, HTTP only accepts exact IDs, never paths or prefixes.
pub fn locate(id: &str) -> Result<Target, String> {
    let missing = || format!("unknown session `{id}`");
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(missing());
    }
    let filename = format!("{id}.jsonl");
    for entry in fs::read_dir(home().join("sessions")).map_err(|_| missing())?.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) { continue; }
        let path = entry.path().join(&filename);
        if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) { continue; }
        let item = read_listing(&path, UNIX_EPOCH).ok_or_else(|| format!("invalid session metadata: {id}"))?;
        return Ok(Target { cwd: PathBuf::from(item.cwd), path });
    }
    Err(missing())
}

/// `registry` prices requests that were logged before their model had a price.
pub fn load(path: &Path, registry: &Registry) -> Result<Loaded, String> {
    load_records(path, false, registry)
}

pub fn read_http(path: &Path, registry: &Registry) -> Result<Loaded, String> {
    load_records(path, true, registry)
}

fn load_records(path: &Path, replay: bool, registry: &Registry) -> Result<Loaded, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut history = Vec::new();
    let mut usage = Usage::default();
    let mut ledger = Ledger::default();
    let mut stats_boundary = false;
    let mut todo = crate::todo::State::default();
    let mut model = None;
    let mut fast = false;
    let mut reply_stats = HashMap::new();
    // Older logs wrote the main chat request immediately before its assistant message.
    // Child request records may be interleaved and never consume the candidate.
    let mut pending = None::<RequestStats>;
    let mut legacy_reply = None;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        // A torn final line from a crash is skipped rather than failing the resume.
        let Ok(mut rec) = serde_json::from_str::<Value>(&line) else {
            pending = None;
            legacy_reply = None;
            continue;
        };
        let previous_reply = legacy_reply.take();
        match rec["type"].as_str() {
            Some("todo") => {
                if let Some(snapshot) = todo_snapshot(&rec) && snapshot.op != "view" { todo = snapshot.state; }
                pending = None;
            }
            Some("message") => {
                let message = rec["message"].take();
                if replay && message["role"] == "assistant" {
                    let explicit = rec["request_id"].as_str();
                    if let Some(stats) = pending.take().filter(|stats| explicit.is_none_or(|id| id == stats.id)) {
                        reply_stats.insert(history.len(), stats.into());
                    } else if explicit.is_none() {
                        legacy_reply = Some(history.len());
                    }
                } else { pending = None; }
                history.push(message);
            }
            Some("compact") => {
                pending = None;
                if !replay { history.clear(); }
                history.push(rec["message"].take());
                usage = Usage::default();
            }
            Some("usage") => {
                pending = None;
                let g = |k: &str| rec[k].as_u64().unwrap_or(0);
                usage = Usage { input: g("input"), output: g("output"), cache_read: g("cache_read"), cache_write: g("cache_write") };
                // Before request records, each model response wrote one `usage`, under the session's model.
                let cost = model.as_deref().and_then(|model| registry.price_of(model)).and_then(|price| price.cost(usage));
                if !stats_boundary { ledger.legacy(usage, cost); }
                if let Some(index) = previous_reply {
                    reply_stats.insert(index, ReplyStats { model: model.clone(), usage: Some(usage), cost, duration_ms: None });
                }
            }
            Some("request_stats_boundary") => { stats_boundary = true; pending = None; }
            Some("request_stats") => {
                if let Ok(mut stats) = serde_json::from_value::<RequestStats>(rec["stats"].take()) {
                    stats.price_missing(registry);
                    if ledger.record(&stats) && replay && !stats.child {
                        pending = matches!(stats.kind, crate::stats::Kind::Chat).then_some(stats);
                    }
                }
            }
            Some("meta" | "model") => {
                pending = None;
                model = rec["model"].as_str().map(str::to_owned);
            }
            Some("fast") => {
                pending = None;
                fast = rec["enabled"].as_bool().unwrap_or(false);
            }
            _ => pending = None,
        }
    }
    let mut session = Session { path: path.to_path_buf(), writer: Arc::new(Mutex::new(Writer { file: None, meta: None, needs_stats_boundary: true })) };
    if !replay && let Some(fix) = repair(&history) {
        session.message(&fix);
        history.push(fix);
    }
    Ok(Loaded { session, history, reply_stats, usage, ledger, model, todo, fast })
}

/// Parse a persisted snapshot using the same validation for resume and replay.
pub fn todo_snapshot(record: &Value) -> Option<crate::todo::Snapshot> {
    if record["type"] != "todo" { return None; }
    let snapshot: crate::todo::Snapshot = serde_json::from_value(record.get("snapshot")?.clone()).ok()?;
    if !matches!(snapshot.op.as_str(), "init" | "start" | "done" | "drop" | "rm" | "block" | "unblock" | "append" | "view") || snapshot.state.validate().is_err() { return None; }
    Some(snapshot)
}

/// Session ids for `cwd`, newest first, with their first user message.
pub fn list(cwd: &Path) -> Vec<(String, String)> {
    let Ok(rd) = fs::read_dir(project_dir(cwd)) else { return Vec::new() };
    let mut items: Vec<(SystemTime, String, String)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| {
            let path = e.path();
            let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            (modified, id, first_prompt(&path))
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.0));
    items.into_iter().map(|(_, id, p)| (id, p)).collect()
}

struct ListedSession {
    id: String,
    prompt: String,
    cwd: String,
    modified: SystemTime,
}

/// Sessions grouped the way Codex groups them: one project per chosen folder,
/// named by that folder, with its threads listed underneath. Nested path
/// segments are not extra levels. Two folders with the same name are
/// disambiguated by their parent.
pub fn catalog() -> Value {
    let library = Library::load();
    let mut order = Vec::new();
    let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
    let mut archived_sessions = Vec::new();
    for item in list_all() {
        let modified = item.modified.duration_since(UNIX_EPOCH).map(|time| time.as_millis() as u64).unwrap_or(0);
        // Your name first, then the AI summary, then the first prompt.
        let renamed = library.titles.get(&item.id).cloned();
        let title = renamed.clone().or_else(|| library.summaries.get(&item.id).cloned()).unwrap_or(item.prompt);
        let row = json!({"id": item.id, "prompt": title, "renamed": renamed.is_some(), "modified": modified, "cwd": item.cwd});
        if library.archived_sessions.contains(&item.id) || library.archived_folders.contains(&item.cwd) {
            if library.archived_sessions.contains(&item.id) { archived_sessions.push(row); }
            continue;
        }
        if !groups.contains_key(&item.cwd) { order.push(item.cwd.clone()); }
        groups.entry(item.cwd.clone()).or_default().push(row);
    }
    let names = project_names(&order);
    let aliases = load_aliases();
    let label = |cwd: &str| aliases.get(cwd).map(|alias| alias.trim()).filter(|alias| !alias.is_empty()).map(|alias| alias.to_owned())
        .unwrap_or_else(|| names.get(cwd).cloned().unwrap_or_else(|| folder_name(cwd)));
    let roots = order.iter().map(|cwd| {
        json!({"name": label(cwd), "path": cwd, "folded": library.folded_folders.contains(cwd), "sessions": groups.remove(cwd).unwrap_or_default(), "children": []})
    }).collect::<Vec<_>>();
    let folders: Vec<Value> = library.archived_folders.iter().map(|cwd| json!({"name": label(cwd), "path": cwd})).collect();
    json!({"roots": roots, "archived": {"sessions": archived_sessions, "folders": folders}})
}

/// Session titles and the archive, kept apart from the session files so
/// archiving never moves or rewrites a conversation.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Library {
    /// Names the user set; they win over everything.
    #[serde(default)]
    titles: HashMap<String, String>,
    /// Titles the model wrote after the first exchange.
    #[serde(default)]
    summaries: HashMap<String, String>,
    #[serde(default, rename = "archivedSessions")]
    archived_sessions: std::collections::BTreeSet<String>,
    #[serde(default, rename = "archivedFolders")]
    archived_folders: std::collections::BTreeSet<String>,
    /// Folders folded in the session list; shared by every web page and App on this server.
    #[serde(default, rename = "foldedFolders")]
    folded_folders: std::collections::BTreeSet<String>,
}

/// The background titler and API handlers both edit the file; serialize them.
static LIBRARY: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Library {
    fn load() -> Library {
        fs::read_to_string(home().join("library.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Load, change and save under the lock so concurrent edits don't drop each other.
    fn update(change: impl FnOnce(&mut Library) -> Result<(), String>) -> Result<(), String> {
        let _guard = LIBRARY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut library = Library::load();
        change(&mut library)?;
        let dir = home();
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let text = serde_json::to_string_pretty(&library).map_err(|error| error.to_string())?;
        // Write then rename so a crash never leaves a half-written file.
        let temp = dir.join("library.json.tmp");
        fs::write(&temp, text).map_err(|error| error.to_string())?;
        fs::rename(&temp, dir.join("library.json")).map_err(|error| error.to_string())
    }
}

/// Empty `title` drops your name; the AI title (or first prompt) shows again.
pub fn set_title(id: &str, title: &str) -> Result<(), String> {
    locate(id)?;
    let title = title.trim();
    if title.chars().count() > 120 { return Err("title is too long".into()); }
    if title.chars().any(char::is_control) { return Err("title cannot contain control characters".into()); }
    Library::update(|library| {
        if title.is_empty() { library.titles.remove(id); } else { library.titles.insert(id.to_owned(), title.to_owned()); }
        Ok(())
    })
}

/// Whether the session still needs an AI title.
pub fn needs_summary(id: &str) -> bool {
    let library = Library::load();
    !library.summaries.contains_key(id) && !library.titles.contains_key(id)
}

pub fn set_summary(id: &str, title: &str) -> Result<(), String> {
    Library::update(|library| { library.summaries.insert(id.to_owned(), title.to_owned()); Ok(()) })
}

/// Archive or restore one session (`kind = "session"`, `key` = id) or a whole
/// folder (`kind = "folder"`, `key` = its cwd as listed).
pub fn set_archived(kind: &str, key: &str, archived: bool) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() { return Err("key is required".into()); }
    match kind {
        "session" => { locate(key)?; }
        "folder" => {}
        _ => return Err("kind must be session or folder".into()),
    }
    Library::update(|library| {
        let set = if kind == "session" { &mut library.archived_sessions } else { &mut library.archived_folders };
        if archived { set.insert(key.to_owned()); } else { set.remove(key); }
        Ok(())
    })
}

/// Fold or unfold one folder (its cwd as listed) in the session list.
pub fn set_folded(path: &str, folded: bool) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() { return Err("path is required".into()); }
    Library::update(|library| {
        if folded { library.folded_folders.insert(path.to_owned()); } else { library.folded_folders.remove(path); }
        Ok(())
    })
}

fn folder_name(cwd: &str) -> String {
    cwd.rsplit(['\\', '/']).find(|part| !part.is_empty()).unwrap_or(cwd).to_owned()
}

fn parent_name(cwd: &str) -> String {
    let mut parts = cwd.split(['\\', '/']).filter(|part| !part.is_empty());
    let _leaf = parts.next_back();
    parts.next_back().unwrap_or("").to_owned()
}

fn project_names(paths: &[String]) -> HashMap<String, String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for path in paths {
        *counts.entry(folder_name(path)).or_default() += 1;
    }
    paths.iter().map(|path| {
        let leaf = folder_name(path);
        let name = if counts[&leaf] > 1 {
            let parent = parent_name(path);
            if parent.is_empty() { leaf } else { format!("{parent}/{leaf}") }
        } else {
            leaf
        };
        (path.clone(), name)
    }).collect()
}

fn alias_file() -> PathBuf {
    home().join("aliases.json")
}

fn load_aliases() -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(alias_file()) else { return HashMap::new() };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn set_alias(path: &str, alias: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() { return Err("path is required".into()); }
    let alias = alias.trim();
    if alias.chars().count() > 80 { return Err("alias is too long".into()); }
    if alias.chars().any(|ch| ch.is_control()) { return Err("alias cannot contain control characters".into()); }
    let mut aliases = load_aliases();
    if alias.is_empty() { aliases.remove(path); } else { aliases.insert(path.to_owned(), alias.to_owned()); }
    let dir = home();
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(&aliases).map_err(|error| error.to_string())?;
    fs::write(dir.join("aliases.json"), text).map_err(|error| error.to_string())
}

/// Every saved session file, archived ones included.
pub(crate) fn all_files() -> Vec<PathBuf> {
    let Ok(dirs) = fs::read_dir(home().join("sessions")) else { return Vec::new() };
    dirs.flatten().filter_map(|dir| fs::read_dir(dir.path()).ok()).flatten().flatten()
        .map(|file| file.path()).filter(|path| path.extension().is_some_and(|ext| ext == "jsonl")).collect()
}

fn list_all() -> Vec<ListedSession> {
    let mut items = Vec::new();
    let Ok(dirs) = fs::read_dir(home().join("sessions")) else { return items };
    for dir in dirs.flatten() {
        let Ok(files) = fs::read_dir(dir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "jsonl")
                && let Some(item) = read_listing(&path, file.metadata().and_then(|meta| meta.modified()).unwrap_or(UNIX_EPOCH))
            {
                items.push(item);
            }
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.modified));
    items
}

fn read_listing(path: &Path, modified: SystemTime) -> Option<ListedSession> {
    let file = File::open(path).ok()?;
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let mut cwd = String::new();
    let mut prompt = String::new();
    for line in BufReader::new(file).lines().take(40).map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
        if cwd.is_empty() && record["type"] == "meta" {
            cwd = plain_cwd(record["cwd"].as_str().unwrap_or(""));
        }
        if prompt.is_empty() && record["type"] == "message" && record["message"]["role"] == "user" {
            let text = record["message"]["content"][0]["text"].as_str().unwrap_or("");
            if !text.starts_with("This session continues") && !text.starts_with("Untrusted peer message") {
                prompt = text.lines().next().unwrap_or("").chars().take(80).collect();
            }
        }
        if !cwd.is_empty() && !prompt.is_empty() { break; }
    }
    if cwd.is_empty() { return None; }
    Some(ListedSession { id, prompt, cwd, modified })
}

fn plain_cwd(raw: &str) -> String {
    let text = raw.strip_prefix(r"\\?\").unwrap_or(raw);
    let text = if let Some(unc) = text.strip_prefix(r"UNC\") { format!(r"\\{unc}") } else { text.to_owned() };
    text.trim_end_matches(['\\', '/']).to_owned()
}


fn first_prompt(path: &Path) -> String {
    let Ok(file) = File::open(path) else { return String::new() };
    for line in BufReader::new(file).lines().take(20).map_while(Result::ok) {
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
        if rec["type"] == "message" && rec["message"]["role"] == "user" {
            let text = rec["message"]["content"][0]["text"].as_str().unwrap_or("");
            let text = crate::attachment::parse(text).map_or(text, |(name, _)| name);
            return text.lines().next().unwrap_or("").chars().take(80).collect();
        }
    }
    String::new()
}

pub fn today() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let t = timestamp(secs);
    format!("{}-{}-{}", &t[0..4], &t[4..6], &t[6..8])
}

/// A crash between a tool call and its result leaves dangling `tool_use` blocks,
/// which the API rejects; answer them as interrupted.
pub(crate) fn repair(history: &[Value]) -> Option<Value> {
    let last = history.last()?;
    if last["role"] != "assistant" {
        return None;
    }
    let results: Vec<Value> = last["content"]
        .as_array()?
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| json!({ "type": "tool_result", "tool_use_id": b["id"], "content": "interrupted: the session ended before this tool finished", "is_error": true }))
        .collect();
    (!results.is_empty()).then(|| json!({ "role": "user", "content": results }))
}

/// `YYYYMMDD-HHMMSS` in UTC.
pub(crate) fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without models.yml no request gets a price filled in, so logged costs stay as written.
    fn load(path: &Path) -> Result<Loaded, String> {
        super::load(path, &Registry::empty())
    }

    #[test]
    fn todo_lifecycle_persists_compaction_and_isolates_sessions() {
        use crate::todo::{self, State, Status};
        let dir = env::temp_dir().join(format!("ashira-todo-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let make = |path| Session { path, writer: Arc::new(Mutex::new(Writer { file: None, meta: None, needs_stats_boundary: true })) };
        let mut session = make(path.clone());
        let mut state = State::default();
        let mut snapshots = Vec::new();
        let init = json!({"op":"init","list":[{"phase":"甲","items":["一","二"]},{"phase":"乙","items":["三"]}]});
        snapshots.push(todo::commit(&mut state, &mut session, &init).unwrap());
        let before = state.clone();
        assert!(todo::commit(&mut state, &mut session, &json!({"op":"append","phase":"乙","items":["四","一"]})).is_err());
        assert_eq!(state, before);
        for input in [json!({"op":"start","task":"二"}), json!({"op":"done","task":"二"}), json!({"op":"block","task":"一","reason":"等待\n输入"}), json!({"op":"append","phase":"乙","items":["四"]}), json!({"op":"unblock","task":"一"}), json!({"op":"drop","task":"三"}), json!({"op":"rm","task":"四"})] {
            snapshots.push(todo::commit(&mut state, &mut session, &input).unwrap());
        }
        assert_eq!(state.phases[0].tasks[0].status, Status::InProgress);
        snapshots.push(todo::commit(&mut state, &mut session, &json!({"op":"append","phase":"甲","items":["五"]})).unwrap());
        snapshots.push(todo::commit(&mut state, &mut session, &json!({"op":"start","items":["五","一"]})).unwrap());
        assert_eq!(state.phases[0].tasks.iter().filter(|task| task.status == Status::InProgress).count(), 2);
        session.compact(&json!({"role":"user","content":"summary"}));
        snapshots.push(todo::commit(&mut state, &mut session, &json!({"op":"view"})).unwrap());
        let mut replay = Replay::open(&path).unwrap();
        let mut replayed = Vec::new();
        while let Some(record) = replay.next().unwrap() { if let Some(snapshot) = todo_snapshot(&record) { replayed.push(snapshot); } }
        assert_eq!(replayed, snapshots);
        assert_eq!(load(&path).unwrap().todo, state);
        let mut fresh = make(dir.join("fresh.jsonl"));
        let mut fresh_state = State::default();
        todo::commit(&mut fresh_state, &mut fresh, &json!({"op":"view"})).unwrap();
        assert_eq!(load(&fresh.path).unwrap().todo, State::default());
        assert_eq!(load(&path).unwrap().todo, state);
        let blocked = dir.join("not-a-directory");
        fs::write(&blocked, "file").unwrap();
        let mut broken = make(blocked.join("session.jsonl"));
        let old = state.clone();
        assert!(todo::commit(&mut state, &mut broken, &json!({"op":"done","task":"一"})).is_err());
        assert_eq!(state, old);
        assert_eq!(load(&path).unwrap().todo, old);
        drop((session, fresh, broken));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persisted_compacted_conversation_replays_and_appends_in_isolation() {
        use crate::stats::Kind;

        let dir = env::temp_dir().join(format!("ashira-replay-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let fresh_path = dir.join("cleared.jsonl");
        let first = json!({"role":"user","content":[{"type":"text","text":"before"}]});
        let answer = json!({"role":"assistant","content":[{"type":"text","text":"answer"}]});
        let summary = json!({"role":"user","content":[{"type":"text","text":"summary"}]});
        let later = json!({"role":"user","content":[{"type":"text","text":"after"}]});
        let next = json!({"role":"assistant","content":[{"type":"text","text":"new"}]});

        // A pre-ledger session has reported tokens but no recoverable price.
        fs::write(&path, format!("{}\n{}\n", json!({"type":"message","message":first}), json!({"type":"usage","input":10,"output":1,"cache_read":0,"cache_write":0}))).unwrap();
        let mut loaded = load(&path).unwrap();
        let old = loaded.ledger.snapshot();
        assert_eq!((old.usage.input, old.usage.output), (10, 1));
        assert!(old.main_unknown);
        let main_usage = Usage { input: 100, output: 20, cache_read: 10, cache_write: 5 };
        let child_usage = Usage { input: 40, output: 4, cache_read: 2, cache_write: 1 };
        let compact_usage = Usage { input: 30, output: 5, cache_read: 3, cache_write: 0 };
        let main = RequestStats { id: "main-1".into(), model: "model-a".into(), kind: Kind::Chat, usage: Some(main_usage), cost: Some(1.25), duration_ms: 1000, child: false };
        let child = RequestStats { id: "child-1".into(), model: "model-b".into(), kind: Kind::Chat, usage: Some(child_usage), cost: Some(0.75), duration_ms: 500, child: true };
        let compact = RequestStats { id: "compact-1".into(), model: "model-b".into(), kind: Kind::Compact, usage: Some(compact_usage), cost: Some(2.5), duration_ms: 500, child: false };
        loaded.session.message(&answer);
        loaded.session.request_stats(&main);
        loaded.session.usage(&main_usage);
        loaded.session.request_stats(&child);
        loaded.session.request_stats(&child); // A repeated IPC report must not double count.
        loaded.session.request_stats(&compact);
        loaded.session.compact(&summary);
        loaded.session.message(&later);
        loaded.session.usage(&compact_usage);
        let mut late_writer = loaded.session.clone();
        drop(loaded);

        let mut loaded = load(&path).unwrap();
        assert_eq!(loaded.history, vec![summary.clone(), later.clone()]);
        assert_eq!((loaded.usage.input, loaded.usage.output), (30, 5));
        let total = loaded.ledger.snapshot();
        assert_eq!((total.usage.input, total.usage.output, total.usage.cache_read, total.usage.cache_write), (180, 30, 15, 6));
        assert_eq!((total.main_requests, total.child_requests), (3, 1));
        assert_eq!((total.main_cost, total.child_cost), (3.75, 0.75));
        assert!(total.main_unknown);
        assert!(!total.child_unknown);
        assert_eq!(total.last_rate, Some(20.0));

        let mut replay = Replay::open(&path).unwrap();
        loaded.session.message(&next);
        let mut messages = Vec::new();
        while let Some(record) = replay.next().unwrap() {
            if matches!(record["type"].as_str(), Some("message" | "compact")) { messages.push(record["message"].clone()); }
        }
        assert_eq!(messages, vec![first, answer, summary.clone(), later.clone()]);
        assert_eq!(load(&path).unwrap().history, vec![summary, later, next]);

        // Clearing selects an independent, still-lazy session. A late child
        // result belongs to the old writer, not the new session or ledger.
        let mut fresh = Session { path: fresh_path.clone(), writer: Arc::new(Mutex::new(Writer { file: None, meta: None, needs_stats_boundary: true })) };
        let late = RequestStats { id: "child-late".into(), model: "model-a".into(), kind: Kind::Chat, usage: Some(child_usage), cost: Some(0.2), duration_ms: 500, child: true };
        late_writer.request_stats(&late);
        assert!(!fresh_path.exists());
        let old_after_late = load(&path).unwrap().ledger.snapshot();
        assert_eq!((old_after_late.main_requests, old_after_late.child_requests), (3, 2));
        assert!((old_after_late.child_cost - 0.95).abs() < 1e-12);
        assert_eq!(old_after_late.main_cost, 3.75);

        fresh.message(&json!({"role":"user","content":[{"type":"text","text":"fresh"}]}));
        fresh.request_stats(&main);
        fresh.usage(&main_usage);
        let fresh_loaded = load(&fresh_path).unwrap();
        let fresh_total = fresh_loaded.ledger.snapshot();
        assert_eq!((fresh_total.main_requests, fresh_total.child_requests), (1, 0));
        assert_eq!((fresh_total.usage.input, fresh_total.usage.output), (100, 20));
        assert_eq!((fresh_total.main_cost, fresh_total.child_cost), (1.25, 0.0));
        assert!(!fresh_total.main_unknown && !fresh_total.child_unknown);
        drop(fresh_loaded); drop(fresh); drop(late_writer); drop(replay); drop(loaded);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fast_preference_survives_resume_and_later_disable() {
        let dir = env::temp_dir().join(format!("ashira-fast-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).unwrap();
        let make = |name: &str| Session { path: dir.join(name), writer: Arc::new(Mutex::new(Writer { file: None, meta: None, needs_stats_boundary: true })) };
        let mut plain = make("plain.jsonl");
        plain.model("cpa/gpt-6.1-sol");
        assert!(!load(&plain.path).unwrap().fast);
        let mut session = make("fast.jsonl");
        session.fast(true);
        assert!(load(&session.path).unwrap().fast);
        session.fast(false);
        assert!(!load(&session.path).unwrap().fast);
        drop((plain, session));
        fs::remove_dir_all(dir).unwrap();
    }
}
