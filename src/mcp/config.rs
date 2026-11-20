//! MCP server discovery: Ashira's own files first, then other tools' (Claude
//! Code, Cursor, VS Code), then root `mcp.json` / `.mcp.json`. The first
//! definition of a name wins; later ones are shadowed, never merged. Runs off
//! the UI thread, after the first frame.

use std::{
    env,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Map, Value, json};

use crate::{
    config::{Key, resolve_key},
    session,
};

use super::transport::Spec;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone)]
pub enum Endpoint {
    Stdio { command: String, args: Vec<String>, env: Vec<(String, String)>, cwd: Option<String> },
    Http { url: String, headers: Vec<(String, String)> },
    Sse { url: String, headers: Vec<(String, String)> },
}

#[derive(Clone)]
pub struct ServerConfig {
    pub endpoint: Endpoint,
    /// Milliseconds; `Some(0)` disables client-side deadlines.
    timeout_ms: Option<u64>,
    /// Include the server's `initialize` instructions in the system prompt.
    pub instructions: bool,
    /// `approval: "allow"`: calls run without asking. Default asks.
    pub trusted: bool,
    /// Tools (raw MCP names) that run without asking even when the server asks.
    pub auto_approve: Vec<String>,
    /// Hash of the raw entry: cached tool lists are reused only while it matches.
    pub fingerprint: String,
}

impl ServerConfig {
    pub fn kind(&self) -> &'static str {
        match self.endpoint {
            Endpoint::Stdio { .. } => "stdio",
            Endpoint::Http { .. } => "http",
            Endpoint::Sse { .. } => "sse",
        }
    }

    /// What the server runs or connects to, for display. Secrets live in
    /// headers and env, which are left out.
    pub fn target(&self) -> String {
        match &self.endpoint {
            Endpoint::Stdio { command, args, .. } => std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" "),
            Endpoint::Http { url, .. } | Endpoint::Sse { url, .. } => url.clone(),
        }
    }

    /// `ASHIRA_MCP_TIMEOUT_MS`, then the server's `timeout`, then 30 s; 0 means none.
    pub fn timeout(&self) -> Option<Duration> {
        let env = env::var("ASHIRA_MCP_TIMEOUT_MS").ok().and_then(|v| v.trim().parse::<u64>().ok());
        let ms = env.or(self.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS);
        (ms > 0).then(|| Duration::from_millis(ms))
    }

    /// Resolves `!command`, environment-variable names and literals in `env` /
    /// `headers` right before connecting, so secrets are never read at startup.
    pub async fn spec(&self, cwd: &Path) -> Spec {
        match &self.endpoint {
            Endpoint::Stdio { command, args, env, cwd: dir } => Spec::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: resolve_pairs(env).await,
                cwd: dir.as_ref().map(|d| cwd.join(d)).unwrap_or_else(|| cwd.to_owned()),
            },
            Endpoint::Http { url, headers } => Spec::Http { url: url.clone(), headers: resolve_pairs(headers).await },
            Endpoint::Sse { url, headers } => Spec::Sse { url: url.clone(), headers: resolve_pairs(headers).await },
        }
    }
}

/// `!cmd` runs a command (entry dropped when it fails or prints
/// nothing); a value naming a set environment variable uses that variable;
/// anything else is literal. Empty results are dropped.
async fn resolve_pairs(pairs: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(pairs.len());
    for (k, raw) in pairs {
        let value = match raw.strip_prefix('!') {
            Some(cmd) => resolve_key(&Key::Command(cmd.trim().to_owned())).await.ok(),
            None => Some(env::var(raw).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| raw.clone())),
        };
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            out.push((k.clone(), v));
        }
    }
    out
}

pub struct Def {
    pub name: String,
    pub config: ServerConfig,
    /// File the definition came from.
    pub source: String,
}

#[derive(Default)]
pub struct Discovery {
    pub servers: Vec<Def>,
    /// Definitions turned off by `enabled: false` or `disabledServers`: (name, source).
    pub disabled: Vec<(String, String)>,
    pub warnings: Vec<String>,
    /// Top-level `toolSearch` in the user file: with more MCP tools than this
    /// the model gets `mcp_search` / `mcp_call` instead of every schema;
    /// `false` always lists every tool.
    pub tool_search: Option<usize>,
}

/// One config file and the key holding its server map.
struct File {
    path: PathBuf,
    key: &'static str,
    /// `~/.claude.json` keeps per-project servers under `projects.<cwd>.mcpServers`.
    claude_project: bool,
}

fn user_home() -> PathBuf {
    env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| ".".into())
}

/// User-level Ashira file: holds `disabledServers` / `enabledServers` and is the
/// one `/mcp enable|disable` writes.
pub fn user_file() -> PathBuf {
    session::home().join("mcp.json")
}

fn files(cwd: &Path) -> Vec<File> {
    let home = user_home();
    let f = |path: PathBuf| File { path, key: "mcpServers", claude_project: false };
    vec![
        f(cwd.join(".ashira").join("mcp.json")),
        f(user_file()),
        File { path: home.join(".claude.json"), key: "mcpServers", claude_project: true },
        f(cwd.join(".claude").join("mcp.json")),
        f(home.join(".claude").join("mcp.json")),
        f(cwd.join(".cursor").join("mcp.json")),
        f(home.join(".cursor").join("mcp.json")),
        File { path: cwd.join(".vscode").join("mcp.json"), key: "servers", claude_project: false },
        f(cwd.join("mcp.json")),
        f(cwd.join(".mcp.json")),
    ]
}

pub fn discover(cwd: &Path) -> Discovery {
    let mut d = Discovery::default();
    let user = read_json(&user_file(), &mut d.warnings);
    let list =
        |key: &str| -> Vec<String> { user.as_ref().and_then(|u| u[key].as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default() };
    let (deny, allow) = (list("disabledServers"), list("enabledServers"));
    d.tool_search = user.as_ref().and_then(|u| match &u["toolSearch"] {
        Value::Bool(false) => Some(usize::MAX),
        v => v.as_u64().map(|n| n as usize),
    });
    let mut seen = std::collections::HashSet::new();
    for file in files(cwd) {
        let root = if file.path == user_file() { user.clone() } else { read_json(&file.path, &mut d.warnings) };
        let Some(root) = root else { continue };
        let source = file.path.display().to_string();
        let mut maps: Vec<&Map<String, Value>> = Vec::new();
        if file.claude_project
            && let Some(projects) = root["projects"].as_object()
        {
            // Claude Code keys projects by path; compare loosely across slash styles.
            let want = norm_path(cwd);
            maps.extend(projects.iter().filter(|(p, _)| norm_path(Path::new(p)) == want).filter_map(|(_, v)| v[file.key].as_object()));
        }
        maps.extend(root[file.key].as_object());
        for map in maps {
            for (name, raw) in map {
                // A shadowed or disabled name still claims it: no fallback to a lower source.
                if !seen.insert(name.clone()) {
                    continue;
                }
                let enabled = match raw.get("enabled") {
                    Some(Value::Bool(b)) => *b,
                    Some(Value::String(s)) => !matches!(s.as_str(), "false" | "0"),
                    Some(Value::Number(n)) => n.as_u64() != Some(0),
                    _ => true,
                };
                if deny.contains(name) || (!enabled && !allow.contains(name)) {
                    d.disabled.push((name.clone(), source.clone()));
                    continue;
                }
                match parse(raw) {
                    Ok(config) => d.servers.push(Def { name: name.clone(), config, source: source.clone() }),
                    Err(e) => d.warnings.push(format!("{source}: `{name}`: {e}")),
                }
            }
        }
    }
    d.servers.sort_by(|a, b| a.name.cmp(&b.name));
    d
}

fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn read_json(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => Some(v),
        Ok(_) => {
            warnings.push(format!("{}: top level must be an object", path.display()));
            None
        }
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

fn parse(raw: &Value) -> Result<ServerConfig, String> {
    if !raw.is_object() {
        return Err("server entry must be an object".into());
    }
    let s = |k: &str| raw[k].as_str().map(expand);
    let pairs =
        |k: &str| -> Vec<(String, String)> { raw[k].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), expand(v.as_str()?)))).collect()).unwrap_or_default() };
    let (command, url) = (s("command"), s("url").or_else(|| s("serverUrl")));
    let kind = raw["type"].as_str().or(raw["transport"].as_str());
    let endpoint = match (kind, command, url) {
        (_, Some(_), Some(_)) => return Err("set either `command` or `url`, not both".into()),
        (None | Some("stdio"), Some(command), None) => Endpoint::Stdio {
            command,
            args: raw["args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(expand)).collect()).unwrap_or_default(),
            env: pairs("env"),
            cwd: s("cwd"),
        },
        (Some("stdio"), None, _) => return Err("stdio server needs `command`".into()),
        (None | Some("http" | "streamable-http" | "streamableHttp"), None, Some(url)) => Endpoint::Http { url, headers: pairs("headers") },
        (Some("sse"), None, Some(url)) => Endpoint::Sse { url, headers: pairs("headers") },
        (Some("http" | "sse" | "streamable-http" | "streamableHttp"), _, None) => return Err("remote server needs `url`".into()),
        (Some(other), _, _) => return Err(format!("unknown type `{other}`")),
        (None, None, None) => return Err("needs `command` (stdio) or `url` (http/sse)".into()),
    };
    let timeout_ms = match &raw["timeout"] {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    };
    let mut h = DefaultHasher::new();
    raw.to_string().hash(&mut h);
    let trusted = match raw["approval"].as_str() {
        None | Some("ask") => false,
        Some("allow") => true,
        Some(other) => return Err(format!("`approval` must be ask or allow, got `{other}`")),
    };
    let auto_approve = raw["autoApprove"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    Ok(ServerConfig {
        endpoint,
        timeout_ms,
        instructions: raw["instructions"].as_bool().unwrap_or(true),
        trusted,
        auto_approve,
        fingerprint: format!("{:016x}", h.finish()),
    })
}

/// `${VAR}` and `${VAR:-default}`; unresolved placeholders stay literal.
fn expand(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let Some(end) = tail.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &tail[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        match env::var(name).ok().filter(|v| !v.is_empty()).or(default.map(str::to_owned)) {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + 3 + end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `/mcp enable|disable`: flips `enabled` when the server is defined in
/// Ashira's user file, otherwise maintains `disabledServers` / `enabledServers`
/// there (other tools' files are never modified). Returns the file written.
pub fn set_enabled(name: &str, enabled: bool) -> Result<PathBuf, String> {
    let path = user_file();
    let mut root = read_owned(&path)?;
    let Some(obj) = root.as_object_mut() else { return Err(format!("{}: top level must be an object", path.display())) };
    let mut edit_list = |key: &str, add: bool| {
        let list = obj.entry(key).or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            a.retain(|v| v.as_str() != Some(name));
            if add {
                a.push(name.into());
            }
        }
    };
    edit_list("disabledServers", !enabled);
    edit_list("enabledServers", false);
    if let Some(entry) = obj.get_mut("mcpServers").and_then(|m| m.get_mut(name)).and_then(Value::as_object_mut) {
        if enabled {
            entry.remove("enabled");
        } else {
            entry.insert("enabled".into(), false.into());
        }
    } else if enabled {
        // Defined elsewhere with `enabled: false`: force it on from here.
        let list = obj.entry("enabledServers").or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            a.push(name.into());
        }
    }
    for key in ["disabledServers", "enabledServers"] {
        if obj.get(key).and_then(Value::as_array).is_some_and(Vec::is_empty) {
            obj.remove(key);
        }
    }
    write_owned(&path, &root)?;
    Ok(path)
}

/// Ashira-owned files `/mcp add` and `/mcp remove` may modify: project first.
fn owned_files(cwd: &Path) -> [PathBuf; 2] {
    [cwd.join(".ashira").join("mcp.json"), user_file()]
}

fn read_owned(path: &Path) -> Result<Value, String> {
    let root = match std::fs::read_to_string(path) {
        Ok(t) if !t.trim().is_empty() => serde_json::from_str::<Value>(&t).map_err(|e| format!("{}: {e}", path.display()))?,
        _ => json!({}),
    };
    if root.is_object() { Ok(root) } else { Err(format!("{}: top level must be an object", path.display())) }
}

/// Atomic: a temp file renamed over the target.
fn write_owned(path: &Path, root: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n").and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Letters, digits, `_ . : -`, single inner spaces; at most 100 characters.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name.split(' ').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c)))
}

/// `/mcp add`: writes `entry` under `mcpServers.<name>` in the project or user file.
pub fn add(cwd: &Path, name: &str, entry: Value, project: bool) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!("服务器名 `{name}` 无效：只能用字母、数字、_ . : -，最长 100 个字符"));
    }
    parse(&entry)?;
    let [proj, user] = owned_files(cwd);
    let path = if project { proj } else { user };
    let mut root = read_owned(&path)?;
    let servers = root.as_object_mut().expect("object").entry("mcpServers").or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else { return Err(format!("{}: `mcpServers` must be an object", path.display())) };
    if servers.contains_key(name) {
        return Err(format!("{} 里已有 `{name}`", path.display()));
    }
    servers.insert(name.to_owned(), entry);
    write_owned(&path, &root)?;
    Ok(path)
}

/// `/mcp remove`: deletes the first Ashira-owned definition of `name`.
pub fn remove(cwd: &Path, name: &str) -> Result<PathBuf, String> {
    for path in owned_files(cwd) {
        let mut root = read_owned(&path)?;
        if root["mcpServers"].as_object_mut().and_then(|m| m.remove(name)).is_some() {
            write_owned(&path, &root)?;
            return Ok(path);
        }
    }
    Err(not_owned(cwd, name))
}

fn not_owned(cwd: &Path, name: &str) -> String {
    format!("`{name}` 不在 Ashira 自己的配置文件里（{} 或 {}）；其他工具的配置不会被修改，可用 /mcp disable {name} 隐藏", owned_files(cwd)[0].display(), user_file().display())
}

/// The raw definition of `name` from an Ashira-owned file, for editing.
/// `{"type", "command", "args", "url", "headers", "source"}` in the shape the
/// add form uses: args space-joined, headers as `Name: value` lines.
pub fn entry(cwd: &Path, name: &str) -> Result<Value, String> {
    for path in owned_files(cwd) {
        let root = read_owned(&path)?;
        let Some(raw) = root["mcpServers"].get(name) else { continue };
        let config = parse(raw)?;
        let text = |key: &str| raw.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let args = raw.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let headers = raw
            .get("headers")
            .and_then(Value::as_object)
            .map(|h| h.iter().map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        return Ok(json!({
            "name": name,
            "type": config.kind(),
            "command": text("command"),
            "args": args,
            "url": text("url"),
            "headers": headers,
            "source": path.display().to_string(),
        }));
    }
    Err(not_owned(cwd, name))
}

/// Replaces `previous` with `name` → `entry` in the file that defines it,
/// keeping keys the form does not edit (timeout, approval, env, …).
/// Renaming onto an existing name is rejected.
pub fn update(cwd: &Path, previous: &str, name: &str, entry: Value) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!("服务器名 `{name}` 无效：只能用字母、数字、_ . : -，最长 100 个字符"));
    }
    parse(&entry)?;
    for path in owned_files(cwd) {
        let mut root = read_owned(&path)?;
        let Some(servers) = root["mcpServers"].as_object_mut() else { continue };
        if !servers.contains_key(previous) {
            continue;
        }
        if name != previous && servers.contains_key(name) {
            return Err(format!("{} 里已有 `{name}`", path.display()));
        }
        let mut merged = servers.remove(previous).and_then(|old| old.as_object().cloned()).unwrap_or_default();
        // The form owns the transport fields; drop the old ones so switching
        // stdio ↔ http does not leave a stale `command` or `url` behind.
        for key in ["type", "transport", "command", "args", "url", "headers"] {
            merged.remove(key);
        }
        merged.extend(entry.as_object().cloned().unwrap_or_default());
        servers.insert(name.to_owned(), Value::Object(merged));
        write_owned(&path, &root)?;
        return Ok(path);
    }
    Err(not_owned(cwd, previous))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_placeholders() {
        // SAFETY: single-threaded test setting a variable no other test reads.
        unsafe { env::set_var("ASHIRA_MCP_TEST_VAR", "tok") };
        assert_eq!(expand("Bearer ${ASHIRA_MCP_TEST_VAR}"), "Bearer tok");
        assert_eq!(expand("${ASHIRA_MCP_NOPE:-dflt}/x"), "dflt/x");
        assert_eq!(expand("keep ${ASHIRA_MCP_NOPE} as is"), "keep ${ASHIRA_MCP_NOPE} as is");
        assert_eq!(expand("${unterminated"), "${unterminated");
    }

    #[test]
    fn infers_transport() {
        assert!(matches!(parse(&json!({"command": "x"})).unwrap().endpoint, Endpoint::Stdio { .. }));
        assert!(matches!(parse(&json!({"url": "https://a/mcp"})).unwrap().endpoint, Endpoint::Http { .. }));
        assert!(matches!(parse(&json!({"type": "sse", "url": "https://a/sse"})).unwrap().endpoint, Endpoint::Sse { .. }));
        assert!(parse(&json!({"command": "x", "url": "y"})).is_err());
        assert!(parse(&json!({"type": "http"})).is_err());
        assert!(parse(&json!({"type": "ws", "url": "y"})).is_err());
    }

    #[test]
    fn update_renames_and_keeps_other_keys() {
        let cwd = env::temp_dir().join(format!("ashira-mcp-update-{}", std::process::id()));
        let file = cwd.join(".ashira").join("mcp.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, json!({"mcpServers": {
            "a": {"command": "old", "args": ["-x"], "timeout": 5000, "env": {"K": "v"}},
            "b": {"url": "https://b/mcp"},
        }}).to_string()).unwrap();

        update(&cwd, "a", "renamed", json!({"type": "http", "url": "https://new/mcp"})).unwrap();
        let root: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let servers = &root["mcpServers"];
        assert!(servers.get("a").is_none());
        assert_eq!(servers["renamed"], json!({"type": "http", "url": "https://new/mcp", "timeout": 5000, "env": {"K": "v"}}));
        assert_eq!(entry(&cwd, "renamed").unwrap()["url"], "https://new/mcp");
        assert!(update(&cwd, "renamed", "b", json!({"command": "x"})).unwrap_err().contains("已有"));

        std::fs::remove_dir_all(&cwd).unwrap();
    }
}
