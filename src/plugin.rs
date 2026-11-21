//! Process plugins. Startup only reads `plugin.yml`; the process starts on the
//! first tool, command, or hook call and stays up for later calls.
//!
//! User plugins live in `~/.ashira/plugins/<name>/`. A project plugin in
//! `<cwd>/.ashira/plugins/<name>/` with the same name replaces the user one.
//! Plugins are trusted: they run with the user's account.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, atomic::{AtomicU64, Ordering}},
    time::Duration,
};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin},
    sync::{Mutex, oneshot},
    time::timeout,
};

use crate::{session, tools};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PROMPT: usize = 8 * 1024;

#[derive(Clone)]
pub struct Plugins {
    inner: Arc<Vec<Plugin>>,
}

struct Plugin {
    manifest: Manifest,
    dir: PathBuf,
    proc: Mutex<Option<Running>>,
}

struct Manifest {
    name: String,
    description: String,
    command: String,
    args: Vec<String>,
    timeout: Duration,
    prompt: String,
    tools: Vec<PluginTool>,
    commands: Vec<PluginCommand>,
    hooks: HashSet<String>,
}

struct PluginTool {
    name: String,
    description: String,
    schema: Value,
}

#[derive(Clone)]
pub struct PluginCommand {
    pub plugin: String,
    pub name: String,
    pub description: String,
    pub arg: String,
}

struct Running {
    stdin: ChildStdin,
    child: Child,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
    next: AtomicU64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    name: String,
    #[serde(default)]
    description: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    tools: Vec<ToolFile>,
    #[serde(default)]
    commands: Vec<CommandFile>,
    #[serde(default)]
    hooks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolFile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    input_schema: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandFile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    arg: String,
}

pub struct BeforeTool {
    pub input: Option<Value>,
    pub deny: Option<String>,
    pub warning: Option<String>,
}

impl Plugins {
    pub fn empty() -> Plugins { Plugins { inner: Arc::new(Vec::new()) } }

    /// Read manifests only. Broken plugins become warnings and are skipped.
    pub fn load(cwd: &Path) -> (Plugins, Vec<String>) {
        let mut warnings = Vec::new();
        let mut by_name: HashMap<String, Plugin> = HashMap::new();
        for root in [session::home().join("plugins"), cwd.join(".ashira").join("plugins")] {
            let entries = match std::fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warnings.push(format!("{}: {error}", root.display()));
                    continue;
                }
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() { continue; }
                let path = dir.join("plugin.yml");
                if !path.is_file() { continue; }
                match load_manifest(&path, &dir) {
                    Ok(plugin) => { by_name.insert(plugin.manifest.name.clone(), plugin); }
                    Err(error) => warnings.push(format!("{}: {error}", path.display())),
                }
            }
        }
        let mut plugins: Vec<Plugin> = by_name.into_values().collect();
        plugins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        let mut seen_tools = HashMap::<String, String>::new();
        let mut seen_commands = HashMap::<String, String>::new();
        for plugin in &plugins {
            for tool in &plugin.manifest.tools {
                let wire = wire_name(&plugin.manifest.name, &tool.name);
                if let Some(owner) = seen_tools.insert(wire.clone(), plugin.manifest.name.clone()) {
                    warnings.push(format!("plugin tool `{wire}` from `{}` replaces `{owner}`", plugin.manifest.name));
                }
            }
            for command in &plugin.manifest.commands {
                if let Some(owner) = seen_commands.insert(command.name.clone(), plugin.manifest.name.clone()) {
                    warnings.push(format!("plugin command `/{}` from `{}` replaces `{owner}`", command.name, plugin.manifest.name));
                }
            }
        }
        (Plugins { inner: Arc::new(plugins) }, warnings)
    }

    pub fn prompt(&self) -> String {
        let mut out = String::new();
        for plugin in self.inner.iter() {
            if plugin.manifest.prompt.is_empty() { continue; }
            if !out.is_empty() { out.push_str("\n\n"); }
            out.push_str(&format!("# {}\n{}", plugin.manifest.name, plugin.manifest.prompt));
        }
        out
    }

    pub fn tool_definitions(&self) -> Vec<Value> {
        let mut tools = Vec::new();
        for plugin in self.inner.iter() {
            for tool in &plugin.manifest.tools {
                tools.push(json!({
                    "name": wire_name(&plugin.manifest.name, &tool.name),
                    "description": format!("Plugin {}: {}", plugin.manifest.name, tool.description),
                    "input_schema": tool.schema.clone(),
                }));
            }
        }
        tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        tools
    }

    pub fn is_tool(&self, name: &str) -> bool { self.lookup_tool(name).is_some() }

    pub fn commands(&self) -> Vec<PluginCommand> {
        let mut commands = Vec::new();
        for plugin in self.inner.iter() {
            for command in &plugin.manifest.commands {
                commands.push(PluginCommand {
                    plugin: plugin.manifest.name.clone(),
                    name: command.name.clone(),
                    description: command.description.clone(),
                    arg: command.arg.clone(),
                });
            }
        }
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands
    }

    pub fn command(&self, name: &str) -> Option<PluginCommand> {
        self.commands().into_iter().find(|command| command.name == name)
    }

    pub fn list(&self) -> String {
        if self.inner.is_empty() { return "没有已安装的插件。目录：~/.ashira/plugins 和 .ashira/plugins".to_owned(); }
        let mut lines = Vec::new();
        for plugin in self.inner.iter() {
            let tools = plugin.manifest.tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>().join(", ");
            let commands = plugin.manifest.commands.iter().map(|command| format!("/{}", command.name)).collect::<Vec<_>>().join(", ");
            lines.push(format!(
                "{}  {}  工具: {}  命令: {}",
                plugin.manifest.name,
                if plugin.manifest.description.is_empty() { "—" } else { plugin.manifest.description.as_str() },
                if tools.is_empty() { "—" } else { &tools },
                if commands.is_empty() { "—" } else { &commands },
            ));
        }
        lines.join("\n")
    }

    pub async fn call_tool(&self, wire: &str, input: &Value, cancel: &tokio_util::sync::CancellationToken) -> crate::tools::Output {
        let Some((plugin, tool)) = self.lookup_tool(wire) else { return crate::tools::Output::err(format!("unknown plugin tool `{wire}`")); };
        let result = tokio::select! {
            _ = cancel.cancelled() => return crate::tools::Output::err("cancelled"),
            result = plugin.request("tool", json!({"name": tool, "input": input})) => result,
        };
        match result {
            Ok(value) => {
                let content = value.get("content").and_then(Value::as_str).unwrap_or("").to_owned();
                if value.get("is_error").and_then(Value::as_bool).unwrap_or(false) { crate::tools::Output::err(content) } else { crate::tools::Output::ok(content) }
            }
            Err(error) => crate::tools::Output::err(error),
        }
    }

    pub async fn command_text(&self, command: &PluginCommand, arg: &str) -> Result<String, String> {
        let plugin = self.inner.iter().find(|plugin| plugin.manifest.name == command.plugin).ok_or_else(|| format!("plugin `{}` is not loaded", command.plugin))?;
        let value = plugin.request("command", json!({"name": command.name, "arg": arg})).await?;
        Ok(value.get("text").and_then(Value::as_str).unwrap_or("").to_owned())
    }

    pub async fn before_tool(&self, name: &str, input: &Value) -> BeforeTool {
        let mut current = input.clone();
        let mut replaced = false;
        let mut warnings = Vec::new();
        for plugin in self.inner.iter().filter(|plugin| plugin.manifest.hooks.contains("before_tool")) {
            match plugin.request("hook", json!({"hook": "before_tool", "name": name, "input": current})).await {
                Ok(value) => {
                    if value.get("allow").and_then(Value::as_bool) == Some(false) {
                        let reason = value.get("reason").and_then(Value::as_str).unwrap_or("denied by plugin").to_owned();
                        return BeforeTool { input: None, deny: Some(format!("{}: {reason}", plugin.manifest.name)), warning: join_warnings(warnings) };
                    }
                    if let Some(next) = value.get("input").filter(|value| value.is_object()).cloned() {
                        current = next;
                        replaced = true;
                    }
                }
                Err(error) => warnings.push(format!("{} before_tool: {error}", plugin.manifest.name)),
            }
        }
        BeforeTool { input: replaced.then_some(current), deny: None, warning: join_warnings(warnings) }
    }

    pub async fn after_tool(&self, name: &str, input: &Value, output: &crate::tools::Output) -> (Option<crate::tools::Output>, Option<String>) {
        let mut text = output.text.clone();
        let mut is_error = output.is_error;
        let mut changed = false;
        let mut warnings = Vec::new();
        for plugin in self.inner.iter().filter(|plugin| plugin.manifest.hooks.contains("after_tool")) {
            match plugin.request("hook", json!({"hook": "after_tool", "name": name, "input": input, "content": text, "is_error": is_error})).await {
                Ok(value) => {
                    if let Some(content) = value.get("content").and_then(Value::as_str) {
                        text = content.to_owned();
                        changed = true;
                    }
                    if let Some(flag) = value.get("is_error").and_then(Value::as_bool) {
                        is_error = flag;
                        changed = true;
                    }
                }
                Err(error) => warnings.push(format!("{} after_tool: {error}", plugin.manifest.name)),
            }
        }
        let patched = changed.then(|| crate::tools::Output { text, is_error, file: output.file.clone(), image: output.image.clone() });
        (patched, join_warnings(warnings))
    }

    pub fn wants(&self, hook: &str) -> bool {
        self.inner.iter().any(|plugin| plugin.manifest.hooks.contains(hook))
    }

    async fn dispatch(&self, hook: &str, params: Value) -> (Vec<Value>, Option<String>) {
        let mut values = Vec::new();
        let mut warnings = Vec::new();
        for plugin in self.inner.iter().filter(|plugin| plugin.manifest.hooks.contains(hook)) {
            let mut body = params.clone();
            if let Some(object) = body.as_object_mut() { object.insert("hook".into(), json!(hook)); }
            match plugin.request("hook", body).await {
                Ok(value) => values.push(value),
                Err(error) => warnings.push(format!("{} {hook}: {error}", plugin.manifest.name)),
            }
        }
        (values, join_warnings(warnings))
    }

    /// Notification hooks ignore successful return values. Process failures become warnings.
    pub async fn notify(&self, hook: &str, params: Value) -> Option<String> {
        if !self.wants(hook) { return None; }
        self.dispatch(hook, params).await.1
    }

    /// Replaces the outbound message list for one model request. Stored history is untouched.
    pub async fn context_messages(&self, mut messages: Value) -> (Value, Option<String>) {
        if !self.wants("context") { return (messages, None); }
        let (values, warning) = self.dispatch("context", json!({"messages": messages.clone()})).await;
        let mut warnings = warning.into_iter().collect::<Vec<_>>();
        for value in values {
            match value.get("messages") {
                Some(next) if next.as_array().is_some_and(|items| items.iter().all(Value::is_object)) => messages = next.clone(),
                Some(_) => warnings.push("context hook returned invalid messages".into()),
                None => {}
            }
        }
        (messages, join_warnings(warnings))
    }

    pub async fn before_agent_message(&self) -> (Option<String>, Option<String>) {
        if !self.wants("before_agent_start") { return (None, None); }
        let (values, warning) = self.dispatch("before_agent_start", json!({})).await;
        let text = values.iter().filter_map(|value| value.get("message").and_then(Value::as_str)).filter(|text| !text.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
        ((!text.is_empty()).then_some(text), warning)
    }

    pub async fn before_compact(&self) -> (bool, Option<String>, Option<String>) {
        if !self.wants("session_before_compact") { return (false, None, None); }
        let (values, warning) = self.dispatch("session_before_compact", json!({})).await;
        let reason = values.iter().find_map(|value| (value.get("cancel").and_then(Value::as_bool) == Some(true)).then(|| value.get("reason").and_then(Value::as_str).unwrap_or("cancelled by plugin").to_owned()));
        (reason.is_some(), reason, warning)
    }

    pub async fn compact_prompt(&self, prompt: &str) -> (String, Option<String>) {
        if !self.wants("session.compacting") { return (prompt.to_owned(), None); }
        let (values, warning) = self.dispatch("session.compacting", json!({"prompt": prompt})).await;
        let mut prompt = prompt.to_owned();
        let mut extra = Vec::new();
        for value in values {
            if let Some(text) = value.get("prompt").and_then(Value::as_str).filter(|text| !text.trim().is_empty()) { prompt = text.to_owned(); }
            match value.get("context") {
                Some(Value::String(line)) => extra.push(line.clone()),
                Some(Value::Array(lines)) => extra.extend(lines.iter().filter_map(Value::as_str).map(str::to_owned)),
                _ => {}
            }
        }
        if !extra.is_empty() { prompt.push_str("\n\n"); prompt.push_str(&extra.join("\n")); }
        (prompt, warning)
    }

    pub async fn shutdown(&self) {
        for plugin in self.inner.iter() {
            if let Some(mut running) = plugin.proc.lock().await.take() {
                let _ = running.child.start_kill();
                let _ = running.child.wait().await;
            }
        }
    }

    fn lookup_tool(&self, wire: &str) -> Option<(&Plugin, &str)> {
        let (plugin_name, tool_name) = wire.split_once("__")?;
        let plugin = self.inner.iter().find(|plugin| plugin.manifest.name == plugin_name)?;
        let tool = plugin.manifest.tools.iter().find(|tool| tool.name == tool_name)?;
        Some((plugin, tool.name.as_str()))
    }
}

impl Plugin {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut slot = self.proc.lock().await;
        if slot.is_none() {
            *slot = Some(self.spawn().await?);
        }
        let running = slot.as_mut().expect("plugin process");
        let id = running.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        running.pending.lock().expect("plugin replies").insert(id, tx);
        let line = serde_json::to_vec(&json!({"id": id, "method": method, "params": params})).map_err(|error| error.to_string())?;
        if let Err(error) = running.stdin.write_all(&line).await.and(running.stdin.write_all(b"\n").await).and(running.stdin.flush().await) {
            slot.take();
            return Err(format!("{}: {error}", self.manifest.name));
        }
        let wait = timeout(self.manifest.timeout, rx);
        match wait.await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => { slot.take(); Err(error) }
            Ok(Err(_)) => { slot.take(); Err(format!("{} stopped", self.manifest.name)) }
            Err(_) => { slot.take(); Err(format!("{} timed out", self.manifest.name)) }
        }
    }

    async fn spawn(&self) -> Result<Running, String> {
        let mut child = tools::command(&self.manifest.command)
            .args(&self.manifest.args)
            .current_dir(&self.dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", self.manifest.name))?;
        let stdin = child.stdin.take().ok_or_else(|| format!("{} has no stdin", self.manifest.name))?;
        let stdout = child.stdout.take().ok_or_else(|| format!("{} has no stdout", self.manifest.name))?;
        let pending = Arc::new(StdMutex::new(HashMap::<u64, oneshot::Sender<Result<Value, String>>>::new()));
        let incoming = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let value: Value = match serde_json::from_str(&line) { Ok(value) => value, Err(_) => continue };
                let Some(id) = value.get("id").and_then(Value::as_u64) else { continue };
                let reply = if let Some(error) = value.get("error").and_then(Value::as_str) {
                    Err(error.to_owned())
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                };
                if let Some(tx) = incoming.lock().expect("plugin replies").remove(&id) { let _ = tx.send(reply); }
            }
            let mut pending = incoming.lock().expect("plugin replies");
            for (_, tx) in pending.drain() { let _ = tx.send(Err("plugin stopped".into())); }
        });
        let running = Running { stdin, child, pending, next: AtomicU64::new(1) };
        // The initialize call uses the same socket, so it cannot go through `request` while spawn holds it.
        Ok(running)
    }
}

fn load_manifest(path: &Path, dir: &Path) -> Result<Plugin, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: ManifestFile = serde_saphyr::from_str(&text).map_err(|error| error.to_string())?;
    if !valid_name(&file.name) { return Err("name must be 1-32 chars of lowercase letters, digits, or underscores, starting with a letter".into()); }
    if file.command.trim().is_empty() { return Err("command is required".into()); }
    let mut tools = Vec::new();
    for tool in file.tools {
        if !valid_name(&tool.name) { return Err(format!("tool `{}` has an invalid name", tool.name)); }
        let schema = tool.input_schema.filter(|value| value.is_object()).unwrap_or_else(|| json!({"type":"object","properties":{}}));
        tools.push(PluginTool { name: tool.name, description: tool.description, schema });
    }
    let mut commands = Vec::new();
    for command in file.commands {
        if !valid_name(&command.name) { return Err(format!("command `{}` has an invalid name", command.name)); }
        if builtin_command(&command.name) { return Err(format!("command `/{}` is reserved", command.name)); }
        commands.push(PluginCommand { plugin: file.name.clone(), name: command.name, description: command.description, arg: command.arg });
    }
    let mut hooks = HashSet::new();
    for hook in &file.hooks {
        let Some(name) = known_hook(hook) else { return Err(format!("unknown hook `{hook}`")); };
        hooks.insert(name.to_owned());
    }
    let prompt = if file.prompt.chars().count() > MAX_PROMPT { file.prompt.chars().take(MAX_PROMPT).collect() } else { file.prompt };
    let timeout = Duration::from_millis(file.timeout_ms.unwrap_or(DEFAULT_TIMEOUT.as_millis() as u64).clamp(1, 30_000));
    Ok(Plugin {
        manifest: Manifest {
            name: file.name,
            description: file.description,
            command: file.command,
            args: file.args,
            timeout,
            prompt,
            tools,
            commands,
            hooks,
        },
        dir: dir.to_owned(),
        proc: Mutex::new(None),
    })
}
fn known_hook(name: &str) -> Option<&'static str> {
    Some(match name {
        "before_tool" | "tool_call" => "before_tool",
        "after_tool" | "tool_result" => "after_tool",
        "context" => "context",
        "before_agent_start" => "before_agent_start",
        "session_start" => "session_start",
        "session_shutdown" => "session_shutdown",
        "session_before_compact" => "session_before_compact",
        "session.compacting" => "session.compacting",
        "session_compact" => "session_compact",
        "turn_start" => "turn_start",
        "turn_end" => "turn_end",
        "agent_start" => "agent_start",
        "agent_end" => "agent_end",
        "auto_compaction_start" => "auto_compaction_start",
        "auto_compaction_end" => "auto_compaction_end",
        "auto_retry_start" => "auto_retry_start",
        "auto_retry_end" => "auto_retry_end",
        "todo_reminder" => "todo_reminder",
        _ => return None,
    })
}
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.len() <= 32
}

fn builtin_command(name: &str) -> bool {
    matches!(name, "models" | "model" | "thinking" | "think" | "fast" | "compact" | "mcp" | "memory" | "clear" | "sessions" | "usage" | "help" | "exit" | "quit" | "plugins")
}

fn wire_name(plugin: &str, tool: &str) -> String { format!("{plugin}__{tool}") }

fn join_warnings(warnings: Vec<String>) -> Option<String> {
    if warnings.is_empty() { None } else { Some(warnings.join("; ")) }
}

// Silence unused child field while still dropping it to kill the process.
impl Drop for Running {
    fn drop(&mut self) { let _ = self.child.start_kill(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn python() -> (String, Vec<String>) {
        if std::process::Command::new("python").arg("--version").output().is_ok_and(|out| out.status.success()) {
            return ("python".into(), vec!["-u".into(), "plugin.py".into()]);
        }
        if std::process::Command::new("py").args(["-3", "--version"]).output().is_ok_and(|out| out.status.success()) {
            return ("py".into(), vec!["-3".into(), "-u".into(), "plugin.py".into()]);
        }
        panic!("python is required to exercise the plugin process");
    }

    #[tokio::test]
    async fn plugin_process_serves_tool_command_and_hook() {
        let (command, args) = python();
        let root = std::env::temp_dir().join(format!("ashira-plugin-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let dir = root.join(".ashira").join("plugins").join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.py"), r#"
import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    method = msg["method"]
    params = msg.get("params") or {}
    if method == "tool":
        result = {"content": "hello " + str((params.get("input") or {}).get("name", "")), "is_error": False}
    elif method == "command":
        result = {"text": "cmd " + str(params.get("arg", ""))}
    elif method == "hook" and params.get("hook") == "before_tool" and "deny-me" in json.dumps(params.get("input")):
        result = {"allow": False, "reason": "blocked"}
    elif method == "hook" and params.get("hook") == "before_tool":
        result = {"allow": True}
    elif method == "hook" and params.get("hook") == "after_tool":
        result = {"content": str(params.get("content", "")) + " hooked", "is_error": False}
    elif method == "hook" and params.get("hook") == "context":
        result = {"messages": list(params.get("messages") or []) + [{"role": "user", "content": "ctx"}]}
    elif method == "hook" and params.get("hook") == "before_agent_start":
        result = {"message": "remember the plugin"}
    elif method == "hook" and params.get("hook") == "session_before_compact":
        result = {"cancel": True, "reason": "not yet"}
    elif method == "hook" and params.get("hook") == "session.compacting":
        result = {"prompt": str(params.get("prompt", "")) + " EXTRA", "context": ["keep the path"]}
    elif method == "hook":
        with open("seen.txt", "a", encoding="utf-8") as seen:
            seen.write(str(params.get("hook", "")) + "\n")
        result = {}
    else:
        result = {"allow": True}
    sys.stdout.write(json.dumps({"id": msg["id"], "result": result}) + "\n")
    sys.stdout.flush()
"#).unwrap();
        let args_yaml = serde_json::to_string(&args).unwrap();
        std::fs::write(dir.join("plugin.yml"), format!(r#"
name: demo
description: Demo plugin
command: {command}
args: {args_yaml}
timeoutMs: 5000
prompt: Use the demo plugin for greetings.
hooks: [before_tool, tool_call, after_tool, context, before_agent_start, session_before_compact, session.compacting, session_start, session_shutdown, session_compact, turn_start, turn_end, agent_start, agent_end, auto_compaction_start, auto_compaction_end, auto_retry_start, auto_retry_end, todo_reminder]
tools:
  - name: greet
    description: Greet a person
    inputSchema:
      type: object
      properties:
        name: {{type: string}}
      required: [name]
commands:
  - name: greet
    description: Run the greeting command
    arg: "[name]"
"#)).unwrap();
        let bad = root.join(".ashira").join("plugins").join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("plugin.yml"), "name: bad\ncommand: python\ncommands:\n  - name: help\n    description: reserved\n").unwrap();
        let ghost = root.join(".ashira").join("plugins").join("ghost");
        std::fs::create_dir_all(&ghost).unwrap();
        std::fs::write(ghost.join("plugin.yml"), "name: ghost\ncommand: python\nhooks: [session_tree]\n").unwrap();
        let slow = root.join(".ashira").join("plugins").join("slow");
        std::fs::create_dir_all(&slow).unwrap();
        std::fs::write(slow.join("plugin.py"), "import json, sys, time\nfor line in sys.stdin:\n    time.sleep(5)\n    msg = json.loads(line)\n    sys.stdout.write(json.dumps({\"id\": msg.get(\"id\"), \"result\": {}}) + \"\\n\")\n    sys.stdout.flush()\n").unwrap();
        std::fs::write(slow.join("plugin.yml"), format!("name: slow\ncommand: {command}\nargs: {args_yaml}\ntimeoutMs: 100\nhooks: [before_tool]\n")).unwrap();
        let (plugins, warnings) = Plugins::load(&root);
        assert!(warnings.iter().any(|warning| warning.contains("reserved")), "{warnings:?}");
        assert!(warnings.iter().any(|warning| warning.contains("unknown hook `session_tree`")), "{warnings:?}");
        assert!(plugins.list().lines().any(|line| line.starts_with("demo ")));
        assert!(plugins.list().lines().all(|line| !line.starts_with("bad ")));
        assert!(plugins.tool_definitions().iter().any(|tool| tool["name"] == "demo__greet"));
        let missing = plugins.call_tool("demo__missing", &json!({}), &tokio_util::sync::CancellationToken::new()).await;
        assert!(missing.is_error);
        let output = plugins.call_tool("demo__greet", &json!({"name": "Ada"}), &tokio_util::sync::CancellationToken::new()).await;
        assert_eq!(output.text, "hello Ada");
        assert!(!output.is_error);
        let command = plugins.command("greet").unwrap();
        assert_eq!(plugins.command_text(&command, "Ada").await.unwrap(), "cmd Ada");
        let denied = plugins.before_tool("bash", &json!({"command": "echo deny-me"})).await;
        assert_eq!(denied.deny.as_deref(), Some("demo: blocked"));
        let allowed = plugins.before_tool("bash", &json!({"command": "echo ok"})).await;
        assert!(allowed.deny.is_none());
        assert!(allowed.warning.as_deref().unwrap_or("").contains("timed out"), "{:?}", allowed.warning);
        let (patched, _) = plugins.after_tool("bash", &json!({}), &crate::tools::Output::ok("done")).await;
        assert_eq!(patched.unwrap().text, "done hooked");
        let (messages, _) = plugins.context_messages(json!([{"role": "user", "content": "hi"}])).await;
        assert!(messages.as_array().unwrap().iter().any(|message| message["content"] == "ctx"));
        assert_eq!(plugins.before_agent_message().await.0.as_deref(), Some("remember the plugin"));
        let (cancel, reason, _) = plugins.before_compact().await;
        assert!(cancel);
        assert_eq!(reason.as_deref(), Some("not yet"));
        let (prompt, _) = plugins.compact_prompt("summary").await;
        assert!(prompt.contains("EXTRA") && prompt.contains("keep the path"), "{prompt}");
        for hook in ["session_start", "turn_start", "turn_end", "agent_start", "agent_end", "session_compact", "auto_compaction_start", "auto_compaction_end", "auto_retry_start", "auto_retry_end", "todo_reminder", "session_shutdown"] {
            plugins.notify(hook, json!({})).await;
        }
        let seen = std::fs::read_to_string(dir.join("seen.txt")).unwrap();
        for hook in ["session_start", "turn_start", "turn_end", "agent_start", "agent_end", "session_compact", "auto_compaction_start", "auto_compaction_end", "auto_retry_start", "auto_retry_end", "todo_reminder", "session_shutdown"] {
            assert!(seen.contains(hook), "{hook} missing from {seen}");
        }
        plugins.shutdown().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}
