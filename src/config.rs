//! Model registry loaded from `models.yml`: `providers.<id>` carry the wire
//! `api`, `baseUrl`, credentials, `headers`, `compat`, static `models`,
//! `modelOverrides` and `discovery`. Which model a workload uses comes from
//! `config.yml` `modelRoles` (`default`, `task`, and the background jobs `compact`,
//! `title`, `memory`; `image` for `generate_image`), as selectors
//! `provider/modelId[:thinkingLevel]`.
//!
//! Discovered model lists are cached in `models-cache.json`; startup never
//! waits on the network unless no model can be chosen at all.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::RwLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use std::{io::{self, Write}, sync::atomic::{AtomicU64, AtomicU8, Ordering}};

use futures_util::future::join_all;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{catalog, session};

const DEFAULT_CONTEXT: u64 = 128_000;
/// Anthropic needs an explicit max_tokens; catalog output limits above this
/// would make every request reserve far more than a coding turn needs.
const ANTHROPIC_MAX_CAP: u32 = 32_000;
const CACHE_TTL: u64 = 2 * 60 * 60;

pub const EXAMPLE: &str = "providers:
  anthropic:
    api: anthropic-messages
    baseUrl: https://api.anthropic.com
    apiKey: ANTHROPIC_API_KEY      # env var name, or the literal key, or \"!command\"
    models:
      - id: claude-sonnet-4-5
        contextWindow: 200000
  openai:
    api: openai-responses          # or openai-completions
    baseUrl: https://api.openai.com/v1
    apiKey: OPENAI_API_KEY
    discovery:
      type: openai-models-list

# and in config.yml:
modelRoles:
  default: anthropic/claude-sonnet-4-5";

/// Supported wire formats and their `api` configuration values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Api {
    AnthropicMessages,
    OpenaiCompletions,
    OpenaiResponses,
}

impl Api {
    pub(crate) fn parse(s: &str) -> Option<Api> {
        Some(match s {
            "anthropic-messages" => Api::AnthropicMessages,
            "openai-completions" => Api::OpenaiCompletions,
            "openai-responses" => Api::OpenaiResponses,
            _ => return None,
        })
    }

    /// Body fields ashira always sets; `compat.extraBody` may not contain them.
    /// Optional fields (max tokens, reasoning, ...) are skipped when extraBody sets them.
    fn reserved(self) -> &'static [&'static str] {
        match self {
            Api::AnthropicMessages => &["model", "messages", "system", "tools", "stream"],
            Api::OpenaiCompletions => &["model", "messages", "tools", "stream"],
            Api::OpenaiResponses => &["model", "input", "instructions", "tools", "stream"],
        }
    }
}

/// OpenAI Fast mode is `service_tier=fast` for GPT models on Chat Completions and Responses.
/// Other vendors can share those wire formats, so the model id has to identify GPT.
pub fn openai_fast_model(api: Api, id: &str) -> bool {
    if !matches!(api, Api::OpenaiCompletions | Api::OpenaiResponses) { return false; }
    let bare = id.rsplit('/').next().unwrap_or(id);
    bare.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("gpt-"))
}

/// Thinking levels selected with the `:level` suffix.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Level {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Level {
    /// `Some(None)` for `auto`/`inherit`, which mean "provider default".
    pub fn parse(s: &str) -> Option<Option<Level>> {
        Some(Some(match s {
            "off" => Level::Off,
            "minimal" => Level::Minimal,
            "low" => Level::Low,
            "medium" => Level::Medium,
            "high" => Level::High,
            "xhigh" => Level::Xhigh,
            "max" => Level::Max,
            "auto" | "inherit" => return Some(None),
            _ => return None,
        }))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Minimal => "minimal",
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
            Level::Xhigh => "xhigh",
            Level::Max => "max",
        }
    }

    /// Cycle order for Shift+Tab; `None` (provider default) sits between max and off.
    pub const ALL: [Level; 7] = [Level::Off, Level::Minimal, Level::Low, Level::Medium, Level::High, Level::Xhigh, Level::Max];

    pub fn label(level: Option<Level>) -> &'static str {
        level.map_or("默认", Level::as_str)
    }
}

/// `base` (a selector without level) with `level` appended.
pub fn with_level(base: &str, level: Option<Level>) -> String {
    match level {
        Some(l) => format!("{base}:{}", l.as_str()),
        None => base.to_owned(),
    }
}

/// Splits `provider/id:high`; a suffix that is not a level stays in the id
/// (`qwen3-coder:30b`).
pub fn split_level(selector: &str) -> (&str, Option<Level>) {
    match selector.rsplit_once(':') {
        Some((head, tail)) if !head.is_empty() => match Level::parse(tail) {
            Some(level) => (head, level),
            None => (selector, None),
        },
        _ => (selector, None),
    }
}

/// Request-shaping overrides; provider `compat` is the baseline, model and
/// override `compat` are merged on top (`extraBody` key by key). Other
/// compatibility keys are accepted and ignored.
#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Compat {
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    pub max_tokens_field: Option<String>,
    pub supports_usage_in_streaming: Option<bool>,
    pub supports_store: Option<bool>,
    #[serde(flatten)]
    _rest: Map<String, Value>,
}

impl Compat {
    fn merged(&self, over: &Compat) -> Compat {
        let mut extra_body = self.extra_body.clone();
        extra_body.extend(over.extra_body.clone());
        Compat {
            extra_body,
            max_tokens_field: over.max_tokens_field.clone().or_else(|| self.max_tokens_field.clone()),
            supports_usage_in_streaming: over.supports_usage_in_streaming.or(self.supports_usage_in_streaming),
            supports_store: over.supports_store.or(self.supports_store),
            _rest: Map::new(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct File {
    #[serde(default)]
    providers: IndexMap<String, Provider>,
    /// Unknown root keys are ignored.
    #[serde(flatten)]
    _rest: Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Provider {
    base_url: Option<String>,
    api: Option<String>,
    api_key: Option<String>,
    #[serde(default)]
    auth_header: bool,
    auth: Option<String>,
    #[serde(default)]
    headers: IndexMap<String, String>,
    #[serde(default)]
    compat: Compat,
    discovery: Option<Discovery>,
    #[serde(default)]
    model_overrides: IndexMap<String, ModelDef>,
    #[serde(default)]
    models: Vec<ModelDef>,
    /// `generate_image` for chats on this provider: a selector (any provider's model);
    /// unset falls back to `modelRoles.image`.
    image_model: Option<String>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelDef {
    id: Option<String>,
    name: Option<String>,
    api: Option<String>,
    base_url: Option<String>,
    context_window: Option<u64>,
    max_tokens: Option<u32>,
    #[serde(default)]
    headers: IndexMap<String, String>,
    #[serde(default)]
    compat: Compat,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

impl ModelDef {
    /// One whole price card; a present but unreadable card blocks lower-priority cards.
    fn price(&self) -> Option<catalog::Price> {
        let card = self.rest.get("cost")?;
        Some(catalog::Price::parse(card).unwrap_or(catalog::Price { unsupported: true, ..Default::default() }))
    }

    // Other shapes have no defined capability meaning and block lower-priority guesses.
    fn reasoning_capability(&self) -> Option<Option<bool>> {
        self.rest.get("reasoning").or_else(|| self.rest.get("thinking")).map(Value::as_bool)
    }
}

/// The card both billing and model listings use: override, then the model, then models.dev.
fn price_card(ov: Option<&ModelDef>, def: Option<&ModelDef>, cat: Option<&catalog::Info>) -> Option<catalog::Price> {
    ov.and_then(ModelDef::price).or_else(|| def.and_then(ModelDef::price)).or_else(|| cat.and_then(|c| c.price.clone()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Discovery {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default = "yes")]
    inject_v1: bool,
    timeout_ms: Option<u64>,
    #[serde(flatten)]
    _rest: Map<String, Value>,
}

fn yes() -> bool {
    true
}

/// Accepted compatibility fields; anything else is reported as a likely typo.
const PROVIDER_KEYS: &[&str] = &["disableStrictTools", "transport", "guardrailIdentifier", "guardrailVersion", "guardrailTrace", "requestMetadata", "remoteCompaction"];
const MODEL_KEYS: &[&str] = &[
    "reasoning",
    "thinking",
    "input",
    "imageInputDecoder",
    "imageCountBudget",
    "tokenizer",
    "cost",
    "maxContextWindow",
    "kind",
    "supportsTools",
    "promptCache",
    "premiumMultiplier",
    "omitMaxOutputTokens",
    "preferWebsockets",
    "contextPromotionTarget",
    "compactionModel",
    "remoteCompaction",
];

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(default)]
    model_roles: IndexMap<String, String>,
    #[serde(flatten)]
    _rest: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Cache(BTreeMap<String, CachedProvider>);

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CachedProvider {
    fetched_at: u64,
    /// Discovery URL the rows came from; a changed baseUrl invalidates them.
    source: String,
    models: Vec<Discovered>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Discovered {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// Per-model wire from `discovery.type: proxy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    api: Option<String>,
}

/// How the credential is obtained; commands run on first use, not at load.
#[derive(Clone, Serialize, Deserialize)]
pub enum Key {
    Value(String),
    Command(String),
}

/// Everything needed to talk to one model.
#[derive(Clone, Serialize, Deserialize)]
pub struct Resolved {
    pub provider: String,
    pub id: String,
    pub api: Api,
    pub url: String,
    pub base_url: String,
    pub headers: Vec<(String, String)>,
    pub key: Option<Key>,
    pub auth_header: bool,
    pub context_window: u64,
    pub max_tokens: Option<u32>,
    pub compat: Compat,
    pub level: Option<Level>,
    /// Session choice for OpenAI Fast mode. Not part of the model selector.
    #[serde(default)]
    pub fast: bool,
    pub reasoning_capability: Option<bool>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub price: Option<catalog::Price>,
}

impl Resolved {
    /// `provider/id[:level]`, the form stored in sessions and shown in the UI.
    pub fn selector(&self) -> String {
        match self.level {
            Some(l) => format!("{}/{}:{}", self.provider, self.id, l.as_str()),
            None => format!("{}/{}", self.provider, self.id),
        }
    }
}

/// One row of the model list.
#[derive(Clone)]
pub struct Entry {
    pub provider: String,
    pub id: String,
    pub name: Option<String>,
    pub context_window: u64,
    pub max_tokens: Option<u32>,
    /// USD per million tokens (input, output): the base rates of the card billing uses.
    pub cost: Option<(f64, f64)>,
    /// Reasoning efforts the model accepts (from models.dev); empty when unknown.
    pub efforts: Vec<String>,
    /// `/fast` applies: a GPT model on Chat Completions or Responses.
    pub fast: bool,
}

impl Entry {
    pub fn selector(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }
}

/// A configured provider as shown in the model hub.
pub struct ProviderInfo {
    pub id: String,
    pub api: String,
    pub models: usize,
    pub status: ProviderStatus,
}

pub enum ProviderStatus {
    Ready,
    /// Discovery configured but not finished yet.
    Pending,
    Failed(String),
    Unsupported(String),
}

pub struct Registry {
    source: PathBuf,
    providers: IndexMap<String, Provider>,
    roles: IndexMap<String, String>,
    cache_path: PathBuf,
    cache: RwLock<Cache>,
    /// Last discovery error per provider; cleared on success.
    errors: RwLock<BTreeMap<String, String>>,
    pub warnings: Vec<String>,
    load_error: Option<String>,
    /// Modification times of the config files at load, to detect edits.
    stamp: Vec<Option<SystemTime>>,
}

/// Path of the settings file shared by model roles and memory settings.
pub fn settings_path() -> PathBuf {
    session::home().join("config.yml")
}

static SETTINGS_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn read_settings(path: &Path) -> Result<Value, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Value::Object(Map::new())),
        Err(e) => return Err(format!("Cannot read settings {}: {e}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let value: Value = serde_saphyr::from_str(&text)
        .map_err(|e| format!("Invalid settings {}: {e}", path.display()))?;
    if !value.is_object() {
        return Err(format!("Invalid settings {}: root must be a mapping", path.display()));
    }
    Ok(value)
}

fn settings_memory_auto(settings: &Map<String, Value>, path: &Path) -> Result<bool, String> {
    let Some(memory) = settings.get("memory") else { return Ok(false) };
    let memory = memory.as_object().ok_or_else(|| format!("Invalid settings {}: memory must be a mapping", path.display()))?;
    match memory.get("auto") {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| format!("Invalid settings {}: memory.auto must be a boolean", path.display())),
    }
}

fn memory_auto_at(path: &Path) -> Result<bool, String> {
    settings_memory_auto(read_settings(path)?.as_object().expect("settings root validated"), path)
}

/// Read the current automatic memory setting without depending on the model registry.
pub async fn memory_auto_enabled() -> Result<bool, String> {
    tokio::task::spawn_blocking(move || memory_auto_at(&settings_path()))
        .await.map_err(|e| format!("Settings task failed: {e}"))?
}

/// Persist the automatic memory setting, preserving all other settings values.
pub async fn set_memory_auto(enabled: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || set_memory_auto_at(&settings_path(), enabled))
        .await.map_err(|e| format!("Settings task failed: {e}"))?
}

fn set_memory_auto_at(path: &Path, enabled: bool) -> Result<(), String> {
    edit_settings(path, |root| {
        settings_memory_auto(root, path)?;
        if root.get("memory").and_then(|m| m.get("auto")).and_then(Value::as_bool) == Some(enabled) {
            return Ok(false);
        }
        let memory = root.entry("memory").or_insert_with(|| Value::Object(Map::new()));
        memory.as_object_mut().expect("memory mapping validated").insert("auto".into(), Value::Bool(enabled));
        Ok(true)
    })
}

/// config.yml `web:`, the folder `--web` serves and `ashira update` keeps current.
pub fn web_dir() -> Option<PathBuf> {
    let settings = read_settings(&settings_path()).ok()?;
    settings.get("web")?.as_str().map(str::trim).filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

pub fn set_web_dir(dir: &Path) -> Result<(), String> {
    let dir = dir.to_string_lossy().into_owned();
    edit_settings(&settings_path(), |root| {
        if root.get("web").and_then(Value::as_str) == Some(dir.as_str()) { return Ok(false); }
        root.insert("web".into(), Value::String(dir));
        Ok(true)
    })
}

/// config.yml `shell:`, the shell the bash tool runs commands with; `ASHIRA_SHELL` overrides it.
pub fn shell_setting() -> Option<String> {
    let settings = read_settings(&settings_path()).ok()?;
    settings.get("shell")?.as_str().map(str::trim).filter(|shell| !shell.is_empty()).map(str::to_owned)
}

/// Saves config.yml `shell:`; empty removes it so the shell is found automatically again.
pub fn set_shell(shell: &str) -> Result<(), String> {
    let shell = shell.trim();
    edit_settings(&settings_path(), |root| {
        if shell.is_empty() { return Ok(root.remove("shell").is_some()); }
        if root.get("shell").and_then(Value::as_str) == Some(shell) { return Ok(false); }
        root.insert("shell".into(), Value::String(shell.to_owned()));
        Ok(true)
    })
}

/// config.yml `approval:`: the mode new sessions start in, and whether the pages hide their
/// switch (then sessions keep that mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalSettings { pub default: crate::agent::ApprovalMode, pub hidden: bool }

/// Unreadable or missing values fall back to manual and shown, the safe side.
pub fn approval_settings() -> ApprovalSettings {
    let settings = read_settings(&settings_path()).ok();
    let approval = settings.as_ref().and_then(|s| s.get("approval"));
    ApprovalSettings {
        default: approval.and_then(|a| a.get("default")).and_then(Value::as_str).and_then(crate::agent::ApprovalMode::parse).unwrap_or(crate::agent::ApprovalMode::Manual),
        hidden: approval.and_then(|a| a.get("hidden")).and_then(Value::as_bool).unwrap_or(false),
    }
}

pub fn set_approval_settings(value: ApprovalSettings) -> Result<(), String> {
    edit_settings(&settings_path(), |root| {
        let next = serde_json::json!({"default": value.default.as_str(), "hidden": value.hidden});
        if root.get("approval") == Some(&next) { return Ok(false); }
        root.insert("approval".into(), next);
        Ok(true)
    })
}

/// config.yml `proactiveQuestions`, read once and kept in step with [set_proactive_questions].
/// 0: not read yet, 1: off, 2: on.
static PROACTIVE_QUESTIONS: AtomicU8 = AtomicU8::new(0);

/// Whether the model asks about open choices before it starts work (设置 → 通用 → 主动提问).
/// Missing or unreadable means on.
pub fn proactive_questions() -> bool {
    match PROACTIVE_QUESTIONS.load(Ordering::Acquire) {
        0 => {
            let on = read_settings(&settings_path()).ok().and_then(|s| s.get("proactiveQuestions").and_then(Value::as_bool)).unwrap_or(true);
            PROACTIVE_QUESTIONS.store(if on { 2 } else { 1 }, Ordering::Release);
            on
        }
        value => value == 2,
    }
}

pub fn set_proactive_questions(on: bool) -> Result<(), String> {
    edit_settings(&settings_path(), |root| {
        if root.get("proactiveQuestions").and_then(Value::as_bool) == Some(on) { return Ok(false); }
        root.insert("proactiveQuestions".into(), Value::Bool(on));
        Ok(true)
    })?;
    PROACTIVE_QUESTIONS.store(if on { 2 } else { 1 }, Ordering::Release);
    Ok(())
}

/// Changes config.yml under its lock, keeping every other key; `edit` returns whether it
/// changed anything, and an unchanged file is not rewritten.
fn edit_settings(path: &Path, edit: impl FnOnce(&mut Map<String, Value>) -> Result<bool, String>) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings directory {}: {e}", parent.display()))?;
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    let lock = settings_private_options().read(true).write(true).create(true).truncate(false).open(&lock_path)
        .map_err(|e| format!("Cannot open settings lock {}: {e}", lock_path.display()))?;
    lock.lock().map_err(|e| format!("Cannot lock settings {}: {e}", lock_path.display()))?;
    let mut value = read_settings(path)?;
    if !edit(value.as_object_mut().expect("settings root validated"))? {
        return Ok(());
    }
    let yaml = serde_saphyr::to_string(&value).map_err(|e| format!("Cannot serialize settings {}: {e}", path.display()))?;
    write_settings_atomic(path, yaml.as_bytes())
}

fn settings_private_options() -> fs::OpenOptions {
    let options = fs::OpenOptions::new();
    #[cfg(unix)]
    let options = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = options;
        options.mode(0o600);
        options
    };
    options
}

pub(crate) fn write_settings_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| "Settings path has no file name".to_string())?;
    let (temp_path, mut file) = loop {
        let sequence = SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let temp_path = parent.join(temp_name);
        match settings_private_options().write(true).create_new(true).open(&temp_path) {
            Ok(file) => break (temp_path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create temporary settings {}: {e}", temp_path.display())),
        }
    };
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("Cannot write settings {}: {e}", path.display()));
    }
    Ok(())
}

fn config_files() -> [PathBuf; 3] {
    let home = session::home();
    [home.join("models.yml"), home.join("models.yaml"), home.join("config.yml")]
}

fn stamp() -> Vec<Option<SystemTime>> {
    config_files().iter().map(|p| fs::metadata(p).and_then(|m| m.modified()).ok()).collect()
}

impl Registry {
    /// An inert registry for an isolated worker with a pinned model.
    /// This does not inspect configuration, caches, or the filesystem.
    pub fn empty() -> Registry {
        let home = session::home();
        Registry {
            source: home.join("models.yml"),
            providers: IndexMap::new(),
            roles: IndexMap::new(),
            cache_path: home.join("models-cache.json"),
            cache: RwLock::default(),
            errors: RwLock::default(),
            warnings: Vec::new(),
            load_error: None,
            stamp: Vec::new(),
        }
    }

    /// A registry from models.yml text alone, for tests.
    #[cfg(test)]
    pub fn from_yaml(text: &str) -> Registry {
        let home = std::env::temp_dir();
        Registry::parse(text, home.join("models.yml"), None, &home.join("config.yml"), home.join("models-cache.json"), Cache::default()).expect("models.yml")
    }

    pub fn changed_on_disk(&self) -> bool {
        !self.stamp.is_empty() && stamp() != self.stamp
    }

    /// Reads `models.yml` (then `models.yaml`) and `config.yml` from `$ASHIRA_HOME`.
    /// Never fails: a missing or broken file gives an empty registry whose
    /// `warnings` say what to fix, so the TUI can still start.
    pub fn load() -> Registry {
        // Taken before reading so an edit during the read triggers another reload.
        let stamp = stamp();
        let mut r = Registry::read();
        r.stamp = stamp;
        r
    }

    fn read() -> Registry {
        let home = session::home();
        let yml = home.join("models.yml");
        let Some(source) = [yml.clone(), home.join("models.yaml")].into_iter().find(|p| p.is_file()) else {
            return Registry::failed(yml.clone(), format!("找不到 {}：请创建它（示例见 ashira --help 或 README），然后执行 /models", yml.display()));
        };
        let text = match fs::read_to_string(&source) {
            Ok(t) => t,
            Err(e) => return Registry::failed(source.clone(), format!("{}: {e}", source.display())),
        };
        let settings_path = home.join("config.yml");
        let settings = match fs::read_to_string(&settings_path) {
            Ok(t) if !t.trim().is_empty() => Some(t),
            Ok(_) => None,
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Registry::failed(source, format!("{}: {e}", settings_path.display())),
        };
        let cache_path = home.join("models-cache.json");
        let cache = fs::read(&cache_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Registry::parse(&text, source.clone(), settings.as_deref(), &settings_path, cache_path, cache).unwrap_or_else(|e| Registry::failed(source, e))
    }

    fn failed(source: PathBuf, warning: String) -> Registry {
        let cache_path = source.with_file_name("models-cache.json");
        Registry {
            source,
            providers: IndexMap::new(),
            roles: IndexMap::new(),
            cache_path,
            cache: RwLock::default(),
            errors: RwLock::default(),
            load_error: Some(warning.clone()),
            warnings: vec![warning],
            stamp: Vec::new(),
        }
    }

    fn parse(text: &str, source: PathBuf, settings: Option<&str>, settings_path: &Path, cache_path: PathBuf, cache: Cache) -> Result<Registry, String> {
        let at = source.display().to_string();
        let file: File = if text.trim().is_empty() { File::default() } else { serde_saphyr::from_str(text).map_err(|e| format!("{at}: {e}"))? };
        let roles = match settings {
            Some(t) => serde_saphyr::from_str::<Settings>(t).map_err(|e| format!("{}: {e}", settings_path.display()))?.model_roles,
            None => IndexMap::new(),
        };
        let mut warnings = Vec::new();
        if file.providers.is_empty() {
            warnings.push(format!("{at}：没有定义任何 provider，示例见 ashira --help 或 README"));
        }
        for (pid, p) in &file.providers {
            for key in p.rest.keys().filter(|k| !PROVIDER_KEYS.contains(&k.as_str())) {
                warnings.push(format!("{at}: providers.{pid}.{key} is not a known field"));
            }
            match &p.api {
                Some(api) if Api::parse(api).is_none() => warnings.push(format!("{at}: providers.{pid}.api `{api}` is not supported by ashira")),
                _ => {}
            }
            if let Some(d) = &p.discovery
                && !matches!(d.kind.as_str(), "openai-models-list" | "proxy")
            {
                warnings.push(format!("{at}: providers.{pid}.discovery.type `{}` is not supported by ashira (openai-models-list, proxy)", d.kind));
            }
            for (i, m) in p.models.iter().enumerate() {
                if m.id.as_deref().is_none_or(str::is_empty) {
                    return Err(format!("{at}: providers.{pid}.models[{i}].id is required"));
                }
            }
            let defs = p.models.iter().map(|m| (m.id.as_deref().unwrap_or_default(), m)).chain(p.model_overrides.iter().map(|(k, m)| (k.as_str(), m)));
            for (id, m) in defs {
                for key in m.rest.keys().filter(|k| !MODEL_KEYS.contains(&k.as_str())) {
                    warnings.push(format!("{at}: providers.{pid} model `{id}`: {key} is not a known field"));
                }
                for api in [m.api.as_deref(), p.api.as_deref()].into_iter().flatten().take(1).filter_map(Api::parse) {
                    let compat = p.compat.merged(&m.compat);
                    if let Some(k) = compat.extra_body.keys().find(|k| api.reserved().contains(&k.as_str())) {
                        return Err(format!("{at}: providers.{pid} model `{id}`: compat.extraBody.{k} is set by ashira"));
                    }
                }
            }
        }
        Ok(Registry { source, providers: file.providers, roles, cache_path, cache: RwLock::new(cache), errors: RwLock::default(), warnings, load_error: None, stamp: Vec::new() })
    }

    pub fn load_error(&self) -> Option<&str> { self.load_error.as_deref() }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn role(&self, name: &str) -> Option<&str> {
        self.roles.get(name).map(String::as_str)
    }

    /// First resolvable selector of a comma-separated role value.
    pub fn resolve_role(&self, name: &str) -> Option<Result<Resolved, String>> {
        let value = self.roles.get(name)?;
        Some(self.first_of(value).map_err(|e| format!("modelRoles.{name} `{value}`: {e}")))
    }

    /// The image model for a chat on `provider`: its `imageModel`, else `modelRoles.image`.
    pub fn resolve_image(&self, provider: &str) -> Option<Result<Resolved, String>> {
        match self.image_model(provider) {
            Some(value) => Some(self.first_of(value).map_err(|e| format!("providers.{provider}.imageModel `{value}`: {e}"))),
            None => self.resolve_role("image"),
        }
    }

    /// Whether a chat on `provider` has an image model to call (its own or the global one).
    pub fn has_image_model(&self, provider: &str) -> bool {
        self.image_model(provider).or_else(|| self.role("image")).is_some_and(|value| !value.trim().is_empty())
    }

    fn image_model(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider)?.image_model.as_deref().filter(|value| !value.trim().is_empty())
    }

    fn first_of(&self, value: &str) -> Result<Resolved, String> {
        let mut last = None;
        for sel in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match self.resolve(sel) {
                Ok(r) => return Ok(r),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| "empty".into()))
    }

    /// Startup model: `modelRoles.default`, else the first model that resolves.
    /// Only when nothing is known yet does this wait for discovery.
    pub async fn initial(&self) -> Result<Resolved, String> {
        match self.resolve_role("default") {
            Some(Ok(r)) => return Ok(r),
            Some(Err(e)) => return Err(format!("{e} (in config.yml)")),
            None => {}
        }
        if self.providers.is_empty() {
            return Err("no model configured".into());
        }
        if self.entries().is_empty() {
            self.refresh(false).await;
        }
        let entries = self.entries();
        entries
            .iter()
            .find_map(|e| self.resolve(&e.selector()).ok())
            .ok_or_else(|| format!("no usable model in {}: add `models` entries or a working `discovery`, or set modelRoles.default in config.yml", self.source.display()))
    }

    /// Configured models, then discovered ones, per provider in file order.
    /// Providers that cannot be used (unsupported api) are left out.
    pub fn entries(&self) -> Vec<Entry> {
        let cache = self.cache.read().expect("cache lock");
        let mut out = Vec::new();
        for (pid, p) in &self.providers {
            if p.api.as_deref().is_some_and(|a| Api::parse(a).is_none()) {
                continue;
            }
            let start = out.len();
            let push = |out: &mut Vec<Entry>, id: &str, name: Option<String>, ctx: Option<u64>, max: Option<u32>, def: Option<&ModelDef>, api: Option<&str>| {
                if out[start..].iter().any(|e: &Entry| e.id == id) {
                    return;
                }
                let ov = p.model_overrides.get(id);
                // models.yml first, then the provider's own listing, then models.dev.
                let cat = catalog::lookup(id);
                let cat = cat.as_ref();
                // Same precedence as `resolve`: override, model, listing, provider.
                let api = ov.and_then(|o| o.api.as_deref()).or(api).or(p.api.as_deref()).and_then(Api::parse);
                out.push(Entry {
                    provider: pid.clone(),
                    id: id.into(),
                    name: ov.and_then(|o| o.name.clone()).or(name).or_else(|| cat.and_then(|c| c.name.clone())),
                    context_window: ov.and_then(|o| o.context_window).or(ctx).or_else(|| cat.and_then(|c| c.context)).unwrap_or(DEFAULT_CONTEXT),
                    max_tokens: ov.and_then(|o| o.max_tokens).or(max).or_else(|| cat.and_then(|c| c.output)),
                    // A card billing cannot use shows no price rather than a wrong one.
                    cost: price_card(ov, def, cat).filter(|price| !price.unsupported).and_then(|price| Some((price.input?, price.output?))),
                    efforts: cat.map(|c| c.efforts.clone()).unwrap_or_default(),
                    fast: api.is_some_and(|api| openai_fast_model(api, id)),
                });
            };
            for m in &p.models {
                push(&mut out, m.id.as_deref().unwrap_or_default(), m.name.clone(), m.context_window, m.max_tokens, Some(m), m.api.as_deref());
            }
            if let Some(c) = cache.0.get(pid).filter(|c| Some(c.source.as_str()) == discovery_url(p).as_deref()) {
                for d in &c.models {
                    push(&mut out, &d.id, None, d.context_window, d.max_tokens, None, d.api.as_deref());
                }
            }
        }
        out
    }

    /// Every configured provider with its model count and discovery state.
    pub fn providers(&self) -> Vec<ProviderInfo> {
        let entries = self.entries();
        let errors = self.errors.read().expect("errors lock");
        let cache = self.cache.read().expect("cache lock");
        self.providers
            .iter()
            .map(|(pid, p)| {
                let models = entries.iter().filter(|e| &e.provider == pid).count();
                let status = match (&p.api, errors.get(pid)) {
                    (Some(api), _) if Api::parse(api).is_none() => ProviderStatus::Unsupported(format!("不支持的 api：{api}")),
                    (_, Some(e)) => ProviderStatus::Failed(e.clone()),
                    _ if discovery_url(p).is_some() && !cache.0.contains_key(pid) => ProviderStatus::Pending,
                    _ => ProviderStatus::Ready,
                };
                ProviderInfo { id: pid.clone(), api: p.api.clone().unwrap_or_default(), models, status }
            })
            .collect()
    }

    /// Discovery state per provider for listings: (provider, api, model count, age).
    pub fn provider_summary(&self) -> Vec<(String, String, Option<u64>)> {
        let cache = self.cache.read().expect("cache lock");
        let now = now();
        self.providers
            .iter()
            .map(|(pid, p)| {
                let age = p.discovery.as_ref().and_then(|_| cache.0.get(pid)).map(|c| now.saturating_sub(c.fetched_at));
                (pid.clone(), p.api.clone().unwrap_or_else(|| "(api not set)".into()), age)
            })
            .collect()
    }

    /// `provider/id[:level]` or a bare id (searched across providers).
    /// The current price card for a selector a request was logged with (`provider/id[:level]`),
    /// without resolving credentials; for costing requests logged before a price was known.
    pub fn price_of(&self, selector: &str) -> Option<catalog::Price> {
        let (sel, _) = split_level(selector.trim());
        let (pid, id) = sel.split_once('/')?;
        let p = self.providers.get(pid)?;
        let def = p.models.iter().find(|m| m.id.as_deref() == Some(id));
        price_card(p.model_overrides.get(id), def, catalog::lookup(id).as_ref())
    }

    pub fn resolve(&self, selector: &str) -> Result<Resolved, String> {
        let (sel, level) = split_level(selector.trim());
        let (pid, id) = match sel.split_once('/') {
            Some((p, id)) if self.providers.contains_key(p) => {
                if id.is_empty() {
                    return Err(format!("`{selector}`: missing model id after `{p}/`"));
                }
                (p.to_owned(), id.to_owned())
            }
            _ => {
                let entries = self.entries();
                match entries.iter().find(|e| e.id.eq_ignore_ascii_case(sel)) {
                    Some(e) => (e.provider.clone(), e.id.clone()),
                    None => {
                        let needle = sel.to_ascii_lowercase();
                        let near: Vec<String> = entries.iter().filter(|e| e.selector().to_ascii_lowercase().contains(&needle)).take(3).map(Entry::selector).collect();
                        let hint = if near.is_empty() { String::new() } else { format!("; did you mean {}", near.join(", ")) };
                        let providers: Vec<&str> = self.providers.keys().map(String::as_str).collect();
                        return Err(format!("model `{selector}` not found (use provider/model-id; providers: {}){hint}", providers.join(", ")));
                    }
                }
            }
        };
        let p = &self.providers[&pid];
        let at = self.source.display();
        let def = p.models.iter().find(|m| m.id.as_deref() == Some(id.as_str()));
        let ov = p.model_overrides.get(&id);
        let cache = self.cache.read().expect("cache lock");
        let found = cache.0.get(&pid).and_then(|c| c.models.iter().find(|d| d.id == id)).cloned();
        drop(cache);
        let pick = |f: &dyn Fn(&ModelDef) -> Option<String>| ov.and_then(f).or_else(|| def.and_then(f));

        let api_name = pick(&|m| m.api.clone()).or_else(|| found.as_ref().and_then(|d| d.api.clone())).or_else(|| p.api.clone());
        let api_name = api_name.ok_or_else(|| format!("{at}: providers.{pid}: `api` is not set (anthropic-messages, openai-completions, openai-responses)"))?;
        let api = Api::parse(&api_name).ok_or_else(|| format!("{at}: providers.{pid}: api `{api_name}` is not supported by ashira"))?;
        let base_url = pick(&|m| m.base_url.clone()).or_else(|| p.base_url.clone()).ok_or_else(|| format!("{at}: providers.{pid}.baseUrl is not set"))?;
        let base_url = base_url.trim_end_matches('/').to_owned();
        let keyless = p.auth.as_deref() == Some("none");
        let key = match &p.api_key {
            Some(k) => Some(api_key(k)),
            None if keyless => None,
            None => return Err(format!("{at}: providers.{pid}.apiKey is not set (use `auth: none` for keyless endpoints)")),
        };
        let mut headers: Vec<(String, String)> = Vec::new();
        for (k, v) in p.headers.iter().chain(def.into_iter().flat_map(|d| &d.headers)).chain(ov.into_iter().flat_map(|o| &o.headers)) {
            let v = header_value(v)?;
            let k = k.to_ascii_lowercase();
            headers.retain(|(h, _)| *h != k);
            headers.push((k, v));
        }
        let mut compat = p.compat.clone();
        if let Some(d) = def {
            compat = compat.merged(&d.compat);
        }
        if let Some(o) = ov {
            compat = compat.merged(&o.compat);
        }
        let url = match api {
            Api::AnthropicMessages => format!("{}/v1/messages", base_url.strip_suffix("/v1").unwrap_or(&base_url)),
            Api::OpenaiCompletions => format!("{base_url}/chat/completions"),
            Api::OpenaiResponses => format!("{base_url}/responses"),
        };
        let found_ctx = found.as_ref().and_then(|d| d.context_window);
        let found_max = found.as_ref().and_then(|d| d.max_tokens);
        let cat = catalog::lookup(&id);
        let configured_max = ov.and_then(|o| o.max_tokens).or(def.and_then(|d| d.max_tokens)).or(found_max);
        let reasoning_capability = ov.and_then(ModelDef::reasoning_capability)
            .or_else(|| def.and_then(ModelDef::reasoning_capability))
            .unwrap_or_else(|| cat.as_ref().and_then(|c| {
                c.efforts.iter().any(|e| e != "none" && e != "off").then_some(true)
            }));
        let display_name = ov.and_then(|o| o.name.clone()).or_else(|| def.and_then(|d| d.name.clone())).or_else(|| cat.as_ref().and_then(|c| c.name.clone()));
        let price = price_card(ov, def, cat.as_ref());
        Ok(Resolved {
            context_window: ov
                .and_then(|o| o.context_window)
                .or(def.and_then(|d| d.context_window))
                .or(found_ctx)
                .or_else(|| cat.as_ref().and_then(|c| c.context))
                .unwrap_or(DEFAULT_CONTEXT),
            // Only Anthropic requires max_tokens; there the catalog's output limit
            // (capped) replaces the fixed default. OpenAI wires omit it unless configured.
            max_tokens: configured_max.or_else(|| match api {
                Api::AnthropicMessages => cat.as_ref().and_then(|c| c.output).map(|n| n.min(ANTHROPIC_MAX_CAP)),
                _ => None,
            }),
            provider: pid,
            id,
            api,
            url,
            base_url,
            headers,
            key,
            auth_header: p.auth_header,
            compat,
            level,
            fast: false,
            display_name,
            price,
            reasoning_capability,
        })
    }

    /// Fetches model lists for providers with discovery whose cache is stale
    /// (or all, with `force`). Returns one message per failed provider.
    pub async fn refresh(&self, force: bool) -> Vec<String> {
        let now = now();
        let jobs: Vec<_> = {
            let cache = self.cache.read().expect("cache lock");
            self.providers
                .iter()
                .filter_map(|(pid, p)| {
                    let url = discovery_url(p)?;
                    let fresh = cache.0.get(pid).is_some_and(|c| c.source == url && now.saturating_sub(c.fetched_at) < CACHE_TTL);
                    (force || !fresh).then(|| (pid.clone(), url, p))
                })
                .collect()
        };
        if jobs.is_empty() {
            return Vec::new();
        }
        let results = join_all(jobs.into_iter().map(|(pid, url, p)| async move {
            let timeout = Duration::from_millis(p.discovery.as_ref().and_then(|d| d.timeout_ms).unwrap_or(10_000));
            let r = fetch_models(&url, p, timeout).await;
            (pid, url, r)
        }))
        .await;
        let mut errors = Vec::new();
        {
            let mut cache = self.cache.write().expect("cache lock");
            let mut failed = self.errors.write().expect("errors lock");
            for (pid, url, r) in results {
                match r {
                    Ok(models) => {
                        failed.remove(&pid);
                        cache.0.insert(pid, CachedProvider { fetched_at: now, source: url, models });
                    }
                    Err(e) => {
                        errors.push(format!("discovery for `{pid}` failed: {e}"));
                        failed.insert(pid, e);
                    }
                }
            }
            if let Ok(bytes) = serde_json::to_vec(&*cache) {
                let _ = fs::write(&self.cache_path, bytes);
            }
        }
        errors
    }
}

fn discovery_url(p: &Provider) -> Option<String> {
    let d = p.discovery.as_ref().filter(|d| matches!(d.kind.as_str(), "openai-models-list" | "proxy"))?;
    let base = p.base_url.as_deref()?.trim_end_matches('/');
    Some(if base.ends_with("/v1") || !d.inject_v1 { format!("{base}/models") } else { format!("{base}/v1/models") })
}

/// Lists the model ids a not-yet-saved provider (a `models.yml` provider
/// mapping) offers through discovery, without touching the cache.
pub async fn probe_models(provider: Value) -> Result<Vec<String>, String> {
    let p: Provider = serde_json::from_value(provider).map_err(|e| e.to_string())?;
    let url = discovery_url(&p).ok_or("需要 Base URL 才能获取模型列表")?;
    let timeout = Duration::from_millis(p.discovery.as_ref().and_then(|d| d.timeout_ms).unwrap_or(10_000));
    let mut ids: Vec<String> = fetch_models(&url, &p, timeout).await?.into_iter().map(|m| m.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn fetch_models(url: &str, p: &Provider, timeout: Duration) -> Result<Vec<Discovered>, String> {
    let http = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
    let mut req = http.get(url);
    for (k, v) in &p.headers {
        req = req.header(k.as_str(), header_value(v)?);
    }
    if let Some(k) = &p.api_key {
        let key = resolve_key(&api_key(k)).await?;
        req = req.bearer_auth(&key).header("x-api-key", &key).header("anthropic-version", "2023-06-01");
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let body: Value = serde_json::from_slice(&bytes).map_err(|e| format!("HTTP {status}: not JSON ({e})"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", body["error"]["message"].as_str().unwrap_or("")));
    }
    let rows = body["data"].as_array().or(body["models"].as_array()).ok_or("response has no `data` list")?;
    let proxy = p.discovery.as_ref().is_some_and(|d| d.kind == "proxy");
    let num = |v: &Value| v.as_u64().filter(|n| *n > 0);
    Ok(rows
        .iter()
        .filter_map(|r| {
            let id = r["id"].as_str()?.to_owned();
            let api = proxy.then(|| {
                let types = r["supported_endpoint_types"].as_array().map(Vec::as_slice).unwrap_or_default();
                let has = |t: &str| types.iter().any(|v| v.as_str() == Some(t));
                if has("anthropic") {
                    Some("anthropic-messages".to_owned())
                } else if has("openai") {
                    Some("openai-completions".to_owned())
                } else {
                    None
                }
            });
            Some(Discovered {
                id,
                context_window: num(&r["max_model_len"]).or(num(&r["context_length"])).or(num(&r["context_window"])),
                max_tokens: [&r["max_output_tokens"], &r["max_completion_tokens"], &r["top_provider"]["max_completion_tokens"]]
                    .into_iter()
                    .find_map(num)
                    .and_then(|n| u32::try_from(n).ok()),
                api: api.flatten(),
            })
        })
        .collect())
}

/// `!cmd` runs a command; otherwise the value names an
/// environment variable, falling back to the literal string.
fn api_key(raw: &str) -> Key {
    if let Some(cmd) = raw.strip_prefix('!') {
        return Key::Command(cmd.trim().to_owned());
    }
    match env::var(raw) {
        Ok(v) if !v.is_empty() => Key::Value(v),
        _ => Key::Value(raw.to_owned()),
    }
}

fn header_value(raw: &str) -> Result<String, String> {
    match api_key(raw) {
        Key::Value(v) => Ok(v),
        Key::Command(c) => Err(format!("header command `!{c}` is not supported; use a literal or env var")),
    }
}

/// Runs `!command` keys once per process (10 s timeout) and caches stdout.
pub async fn resolve_key(key: &Key) -> Result<String, String> {
    use std::{collections::HashMap, sync::Mutex};
    static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    let cmd = match key {
        Key::Value(v) => return Ok(v.clone()),
        Key::Command(c) => c,
    };
    if let Some(v) = CACHE.lock().expect("key cache").as_ref().and_then(|m| m.get(cmd)) {
        return Ok(v.clone());
    }
    let mut c = crate::tools::command(crate::tools::shell());
    c.arg("-c").arg(cmd).stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(10), c.output())
        .await
        .map_err(|_| format!("apiKey command `{cmd}` timed out"))?
        .map_err(|e| format!("apiKey command `{cmd}`: {e}"))?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || value.is_empty() {
        return Err(format!("apiKey command `{cmd}` produced no key"));
    }
    CACHE.lock().expect("key cache").get_or_insert_default().insert(cmd.clone(), value.clone());
    Ok(value)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn memory_auto_settings_lifecycle() {
        let dir = env::temp_dir().join(format!("ashira-settings-test-{}-{}", std::process::id(), SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let path = dir.join("config.yml");
        assert!(!memory_auto_at(&path).unwrap());
        set_memory_auto_at(&path, false).unwrap();
        assert_eq!(read_settings(&path).unwrap()["memory"]["auto"], false);
        set_memory_auto_at(&path, true).unwrap();
        assert!(memory_auto_at(&path).unwrap());

        fs::write(&path, "modelRoles:\n  default: provider/model\nmemory:\n  auto: true\n  other: kept\nunknown:\n  nested:\n    - one\n    - two\n").unwrap();
        set_memory_auto_at(&path, false).unwrap();
        let settings = read_settings(&path).unwrap();
        assert!(!memory_auto_at(&path).unwrap());
        assert_eq!(settings["modelRoles"]["default"], "provider/model");
        assert_eq!(settings["memory"]["other"], "kept");
        assert_eq!(settings["unknown"]["nested"], serde_json::json!(["one", "two"]));

        fs::write(&path, "memory:\n  auto: true\n").unwrap();
        assert!(memory_auto_at(&path).unwrap());
        for invalid in ["memory: [", "[]", "memory: false", "memory:\n  auto: 'true'\n"] {
            fs::write(&path, invalid).unwrap();
            assert!(memory_auto_at(&path).is_err());
            assert!(set_memory_auto_at(&path, false).is_err());
            assert_eq!(fs::read(&path).unwrap(), invalid.as_bytes());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_providers_image_model_wins_over_the_global_role() {
        let yaml = "providers:\n  a:\n    api: openai-completions\n    baseUrl: https://a.example/v1\n    apiKey: k\n    imageModel: b/img-own\n    models: [{id: chat}]\n  b:\n    api: openai-completions\n    baseUrl: https://b.example/v1\n    apiKey: k\n    models: [{id: img-own}, {id: img-global}, {id: chat}]\n";
        let home = env::temp_dir();
        let registry = |roles: Option<&str>| Registry::parse(yaml, home.join("models.yml"), roles, &home.join("config.yml"), home.join("models-cache.json"), Cache::default()).unwrap();

        let global = registry(Some("modelRoles:\n  image: b/img-global\n"));
        assert_eq!(global.resolve_image("a").unwrap().unwrap().id, "img-own");
        assert_eq!(global.resolve_image("b").unwrap().unwrap().id, "img-global", "no own image model: the global role");
        assert!(global.has_image_model("b"));

        let none = registry(None);
        assert_eq!(none.resolve_image("a").unwrap().unwrap().id, "img-own");
        assert!(none.resolve_image("b").is_none() && !none.has_image_model("b"), "nothing set: the tool is only offered on Responses");
        assert!(none.warnings.iter().all(|w| !w.contains("imageModel")), "imageModel is a known provider field");
    }
}
