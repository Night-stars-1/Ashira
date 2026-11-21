mod agent;
mod attachment;
mod api;
mod catalog;
mod clipboard;
mod composer;
mod config;
mod danger;
mod file_output;
mod file_report;
mod graphics;
mod hub;
mod input;
mod image_gen;
mod ipc;
mod mcp;
mod plugin;
mod mcp_report;
mod memory;
mod memory_auto;
mod memory_report;
mod openai;
mod process_guard;
mod model_config;
mod serve;
mod report;
mod question;
mod render;
mod responses;
mod search;
mod session;
mod stats;
mod usage_report;
mod subagents;
mod subagent_view;
mod todo;
mod todo_render;
mod tools;
mod title;
mod transcript;
mod tui;
mod update;
mod web;
mod web_terminal;

use std::{
    env,
    io::{self, IsTerminal, Read, Write},
    process::ExitCode,
    sync::Arc,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{Agent, Event, Shared},
    api::Client,
    config::Registry,
    mcp::Mcp,
    session::Session,
};

const HELP: &str = "ashira — a fast terminal coding agent

USAGE:
    ashira [OPTIONS] [PROMPT]
    ashira update [VERSION]  Replace this program with the latest (or given) GitHub release,
                             and the --web folder with the same release's web build

OPTIONS:
    -p, --print <PROMPT>   Run one prompt non-interactively and print the result
                           (reads the prompt from stdin when PROMPT is `-`)
    -r, --resume [ID]      Resume the newest session here, or the one whose id starts with ID
    -l, --list             List sessions for the current directory
    -m, --model <SELECTOR> provider/model-id[:level], or a bare model id
                           (default: the resumed session's model, then modelRoles.default)
        --models [FILTER]  List models (configured + discovered), grouped by provider
        --refresh-models   Re-run model discovery and the models.dev download, then list
    -y, --yes              Run dangerous commands without asking
        --serve            Serve the HTTP API instead of the TUI (default 127.0.0.1:8787)
        --listen <ADDR>    API listen address, implies --serve
        --web <DIR>        Also serve the web client from DIR on the API port (implies --serve);
                           remembered as config.yml `web:`, an empty DIR offers to install it
    -h, --help             Show this help
    -V, --version          Show version

MODELS:
    ~/.ashira/models.yml   providers.<id>: api, baseUrl, apiKey, headers, compat,
                           models, modelOverrides, discovery
    ~/.ashira/config.yml   modelRoles: { default: provider/model-id[:level], task: ... }
    Levels: off|minimal|low|medium|high|xhigh|max. In the TUI, /models picks a model.

ENVIRONMENT:
    ASHIRA_SHELL       shell for the bash tool; overrides config.yml `shell:` (default: Git Bash / bash)
    ASHIRA_HOME        data dir (default ~/.ashira)
    ASHIRA_API_TOKEN   required with --serve/--listen; clients send it as a Bearer header or ?token=
    ASHIRA_WEB_TERMINAL=1  enable the web/App terminal: anyone with the token can run commands as you

GLOBAL MEMORY:
    ~/.ashira/memory.json   saved preferences and facts, shared across projects and sessions
    /memory                list entries; set <key> <text> saves, delete <key> forgets
    /memory auto on|off|status  control automatic consolidation (off by default)
    ~/.ashira/config.yml       memory: { auto: true } enables automatic consolidation

MCP: servers from ./.ashira/mcp.json, ~/.ashira/mcp.json, Claude Code, Cursor, VS Code and ./mcp.json / ./.mcp.json;
     first definition of a name wins.
     {\"mcpServers\": {name: {type: stdio|http|sse, command, args, env, cwd, url, headers, timeout, enabled,
                             approval: ask|allow, autoApprove: [tool…]}}, \"toolSearch\": 40}
    ASHIRA_MCP_TIMEOUT_MS  per-request MCP timeout for every server (default 30000, 0 = none)";

struct Args {
    print: Option<String>,
    resume: Option<String>,
    list: bool,
    model: Option<String>,
    models: Option<(String, bool)>,
    yes: bool,
    serve: Option<String>,
    web: Option<String>,
    prompt: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { print: None, resume: None, list: false, model: None, models: None, yes: false, serve: None, web: None, prompt: None };
    let mut it = env::args().skip(1).peekable();
    let mut rest = Vec::new();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("ashira {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-p" | "--print" => a.print = Some(it.next().ok_or("--print needs a prompt (or `-` for stdin)")?),
            "-r" | "--resume" => a.resume = Some(it.next_if(|n| !n.starts_with('-')).unwrap_or_default()),
            "-l" | "--list" => a.list = true,
            "-m" | "--model" => a.model = Some(it.next().ok_or("--model needs a value")?),
            "--models" => a.models = Some((it.next_if(|n| !n.starts_with('-')).unwrap_or_default(), false)),
            "--refresh-models" => a.models = Some((String::new(), true)),
            "-y" | "--yes" => a.yes = true,
            "--serve" => a.serve = Some("127.0.0.1:8787".into()),
            "--listen" => a.serve = Some(it.next().ok_or("--listen needs host:port")?),
            "--web" => a.web = Some(it.next().ok_or("--web needs a folder")?),
            "--" => rest.extend(it.by_ref()),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s} (see --help)")),
            _ => rest.push(arg),
        }
    }
    if a.web.is_some() && a.serve.is_none() {
        a.serve = Some("127.0.0.1:8787".into());
    }
    if !rest.is_empty() {
        a.prompt = Some(rest.join(" "));
    }
    Ok(a)
}

fn main() -> ExitCode {
    match env::args_os().nth(1).as_deref() {
        Some(flag) if flag == std::ffi::OsStr::new("--tool-bootstrap") => {
            if env::args_os().len() != 2 || io::stdin().is_terminal() { return ExitCode::from(2); }
            match process_guard::tool_bootstrap_main() {
                Ok(code) => std::process::exit(code),
                Err(error) => { eprintln!("ashira tool bootstrap: {error}"); return ExitCode::FAILURE; }
            }
        }
        Some(flag) if flag == std::ffi::OsStr::new("--worker-stdio") => {
            if env::args_os().len() != 2 || io::stdin().is_terminal() || io::stdout().is_terminal() { return ExitCode::from(2); }
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("worker runtime");
            let result = runtime.block_on(ipc::worker_main());
            runtime.shutdown_background();
            return match result { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("ashira worker: {error}"); ExitCode::FAILURE } };
        }
        Some(word) if word == std::ffi::OsStr::new("update") => {
            let version = env::args().nth(2);
            if env::args_os().len() > 3 || version.as_deref().is_some_and(|v| v.starts_with('-')) {
                eprintln!("ashira: usage: ashira update [VERSION]");
                return ExitCode::from(2);
            }
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("update runtime");
            let result = runtime.block_on(update::run(version.as_deref()));
            return match result { Ok(message) => { println!("{message}"); ExitCode::SUCCESS } Err(error) => { eprintln!("ashira update: {error}"); ExitCode::FAILURE } };
        }
        _ => {}
    }
    update::clean_leftover();
    update::remember_started();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ashira: {e}");
            return ExitCode::from(2);
        }
    };
    let cwd = env::current_dir().unwrap_or_else(|_| ".".into());
    if args.list {
        for (id, prompt) in session::list(&cwd) {
            println!("{id}  {prompt}");
        }
        return ExitCode::SUCCESS;
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    if let Some((filter, refresh)) = &args.models {
        let code = rt.block_on(list_models(&Registry::load(), filter, *refresh));
        rt.shutdown_background();
        return code;
    }
    let code = {
        let _guard = rt.enter();
        match run(args, cwd, &rt) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("ashira: {e}");
                ExitCode::FAILURE
            }
        }
    };
    // Don't wait on blocking-pool reads of pipes that orphaned processes still hold.
    rt.shutdown_background();
    code
}

fn run(args: Args, cwd: std::path::PathBuf, rt: &tokio::runtime::Runtime) -> Result<ExitCode, String> {
    // Checked first so a missing token fails before any startup work.
    let api = match args.serve.clone() {
        Some(listen) => Some((listen, env::var("ASHIRA_API_TOKEN").ok().map(|token| token.trim().to_owned()).filter(|token| !token.is_empty())
            .ok_or("--serve 需要设置 ASHIRA_API_TOKEN（网页和 App 连接时填写同一个值），例如：ASHIRA_API_TOKEN=<随机字符串> ashira --serve")?,
            // `--web DIR`, else the folder a previous `--web` saved in config.yml.
            web::folder(args.web.as_deref())?)),
        None => None,
    };
    let registry = Registry::load();
    let (mut notices, warnings) = (Vec::new(), registry.warnings.clone());
    let loaded = match &args.resume {
        Some(id) => Some(session::load(&session::find(&cwd, id)?, &registry)?),
        None => None,
    };
    // Discovered lists refresh in the background; startup uses the cache.
    let resolved = match (args.model, loaded.as_ref().and_then(|l| l.model.clone())) {
        (Some(sel), _) => registry.resolve(&sel).or_else(|_| {
            // Not in config or cache yet: discover once, then retry.
            rt.block_on(registry.refresh(false));
            registry.resolve(&sel)
        }),
        // A resumed session keeps its model while it still resolves.
        (None, Some(sel)) if registry.resolve(&sel).is_ok() => registry.resolve(&sel),
        _ => rt.block_on(registry.initial()),
    };
    let resolved = resolved.map(|mut model| {
        model.fast = loaded.as_ref().is_some_and(|session| session.fast);
        model
    });
    // Without a model the TUI still starts; /models picks one once configured.
    let client = match resolved.and_then(Client::new) {
        Ok(c) => {
            c.warm();
            Some(c)
        }
        Err(e) => {
            notices.push(format!("模型：{e}"));
            None
        }
    };
    let model = client.as_ref().map(|c| c.name().to_owned()).unwrap_or_default();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    let mcp = Mcp::new(cwd.clone(), ev_tx.clone());
    notices.extend(warnings.into_iter().map(|w| format!("模型配置：{w}")));
    let print = match args.print {
        Some(p) if p == "-" => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            Some(s)
        }
        Some(p) => Some(p),
        None if args.serve.is_none() && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) => {
            let mut s = args.prompt.clone().unwrap_or_default();
            if !io::stdin().is_terminal() {
                io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            }
            if s.trim().is_empty() {
                return Err("no TTY: pass a prompt with -p or on stdin".into());
            }
            Some(s)
        }
        None => None,
    };
    let replay = if print.is_none() {
        loaded.as_ref().map(|l| session::Replay::open(&l.session.path)
            .map_err(|e| format!("{}: {e}", l.session.path.display()))).transpose()?
    } else { None };

    let (plugins, plugin_warnings) = plugin::Plugins::load(&cwd);
    notices.extend(plugin_warnings);
    // Chosen now, so a bad ASHIRA_SHELL or `shell:` is reported at start, not at the first command.
    if let Some(problem) = &tools::shell_choice().problem {
        notices.push(format!("Shell：{problem}"));
        if api.is_some() { eprintln!("ashira: warning: Shell：{problem}"); }
    }
    let shared = Arc::new(Shared::new(registry, client, mcp.clone(), plugins, cwd.clone(), ev_tx, print.is_none(), args.yes));
    let (agent, resumed) = match loaded {
        Some(l) => {
            let n = l.history.len();
            (Agent::resumed(shared.clone(), l), Some(n))
        }
        None => (Agent::new(shared.clone(), Session::new(&cwd, &model)), None),
    };
    let session_id = agent.session.as_ref().map(Session::id).unwrap_or_default();

    if let Some(prompt) = print {
        if shared.model().is_none() {
            return Err(notices.join("\n"));
        }
        for n in &notices {
            eprintln!("ashira: warning: {n}");
        }
        let shared = shared.clone();
        // Print mode: every server gets to load its tools (or fail) before the turn.
        let code = rt.block_on(async {
            if let Some(warning) = shared.plugins.notify("session_start", serde_json::json!({"resumed": resumed.is_some(), "sessionId": &session_id})).await {
                let _ = shared.events.send(Event::Notice(warning));
            }
            mcp.start();
            mcp.wait_settled(mcp_wait(), true).await;
            let code = print_mode(agent, prompt, ev_rx).await;
            shared.shutdown().await;
            mcp.shutdown().await;
            code
        });
        return Ok(code);
    }

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let agent_task = rt.spawn(agent.serve(cmd_rx));
    let initial_prompt = args.prompt;
    {
        // One quiet line instead of a notice per unreachable provider; --models shows details.
        let shared = shared.clone();
        rt.spawn(async move {
            let failed: Vec<String> = shared.registry().refresh(false).await.iter().filter_map(|e| e.split('`').nth(1).map(str::to_owned)).collect();
            if !failed.is_empty() {
                let _ = shared.events.send(Event::Notice(format!("以下服务商获取模型列表失败：{}（详情见 /models 或 ashira --models）", failed.join("、"))));
            }
            let _ = shared.events.send(Event::ModelsChanged);
        });
    }
    {
        // models.dev metadata (context, output limit, price). Checked hourly so a long-running
        // TUI or API picks up a new model's price; `refresh` only downloads once a day.
        let shared = shared.clone();
        rt.spawn(async move {
            let mut first = true;
            loop {
                match catalog::refresh(false).await {
                    Ok(true) => shared.catalog_updated(),
                    Ok(false) => {}
                    // Reported once; later attempts retry quietly with the cache in use.
                    Err(e) if first => {
                        let _ = shared.events.send(Event::Notice(format!("更新模型资料失败（{e}），继续使用缓存")));
                    }
                    Err(_) => {}
                }
                first = false;
                tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
            }
        });
    }
    if print.is_none() {
        // Newer release: a notice in the TUI and the web/App (cached for a day, see update.rs).
        let shared = shared.clone();
        rt.spawn(async move {
            if let Some(notice) = update::notice().await { let _ = shared.events.send(Event::Notice(notice)); }
        });
    }
    if shared.model().is_none() && !shared.registry().source().is_file() {
        notices.push(format!("models.yml 示例：\n{}", config::EXAMPLE));
    }
    // After the first frame: discovery and connections run in the background.
    rt.spawn({
        let mcp = mcp.clone();
        async move { mcp.start() }
    });
    rt.block_on(async {
        if let Some(warning) = shared.plugins.notify("session_start", serde_json::json!({"resumed": resumed.is_some(), "sessionId": &session_id})).await {
            let _ = shared.events.send(Event::Notice(warning));
        }
    });
    if let Some((listen, api_token, web_dir)) = api {
        let addr: std::net::SocketAddr = listen.parse().map_err(|error| format!("--listen: {error}"))?;
        // A restart's new process can start before the old one has let go of the port.
        let listener = rt.block_on(async {
            let restarted = env::var_os(RESTARTED).is_some();
            let mut tries = 0;
            loop {
                match tokio::net::TcpListener::bind(addr).await {
                    Err(error) if restarted && error.kind() == std::io::ErrorKind::AddrInUse && tries < 50 => {
                        tries += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    result => break result,
                }
            }
        }).map_err(|error| error.to_string())?;
        // Each session gets its own runtime; this agent only stays idle as the template's
        // owner. Its events (process-wide notices) have no session page to go to.
        drop(cmd_tx);
        let mut ev_rx = ev_rx;
        rt.spawn(async move { while ev_rx.recv().await.is_some() {} });
        // An empty folder, or one at another release (the core was just updated), gets this
        // core's web build in the background; the API serves meanwhile, and if it fails the
        // install page in its place still offers it.
        if let Some(dir) = web_dir.clone().filter(|dir| web::needs_install(dir)) {
            eprintln!("ashira web: 正在下载 v{} 的网页到 {} …", update::CURRENT, dir.display());
            rt.spawn(async move {
                match update::install_web(&dir).await {
                    Ok(tag) => eprintln!("ashira web: 已安装 {tag}"),
                    Err(error) => eprintln!("ashira web: 网页下载失败：{error}（打开网页可重试，或运行 `ashira update`）"),
                }
            });
        }
        let serve_result = rt.block_on(serve::start(shared.clone(), listener, api_token, web_dir));
        rt.block_on(async {
            agent_task.await.ok();
            mcp.shutdown().await;
        });
        if serve_result? {
            // Docker (`restart: unless-stopped`) starts the container again; a child started
            // here would die with this process, the container's main one.
            if update::in_container() { return Ok(ExitCode::from(RESTART_EXIT)); }
            relaunch()?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    let result = tui::run(shared.clone(), cmd_tx, ev_rx, tui::Startup { session_id, resumed, notices, replay, initial_prompt });
    let final_session = rt.block_on(async {
        shared.interrupt();
        shared.shutdown().await;
        let session = agent_task.await.ok().flatten();
        mcp.shutdown().await;
        session
    });
    if result.map_err(|e| e.to_string())?
        && let Some(session) = final_session
        && let Some(command) = resume_command(&session, &cwd)
    {
        println!("\n使用以下命令恢复此会话：\n{command}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Marks the process `relaunch` starts, so it waits for the port its parent is releasing.
const RESTARTED: &str = "ASHIRA_RESTARTED";
/// Exit status of a container's requested restart; non-zero so `restart: on-failure` restarts too.
const RESTART_EXIT: u8 = 75;

/// `POST /v1/restart`: starts the program now on disk (the updated build) with the same
/// arguments, environment and directory. Its output goes where this process's went when that
/// was a file or terminal; a pipe that closes as this process exits (a supervisor capturing
/// it) would end the new one at its first log line, so then it writes nowhere.
fn relaunch() -> Result<(), String> {
    let exe = update::program().map_err(|error| format!("找不到当前程序：{error}"))?;
    eprintln!("ashira: 正在重启 {}", exe.display());
    let mut command = std::process::Command::new(&exe);
    command.args(env::args_os().skip(1)).env(RESTARTED, "1").stdin(std::process::Stdio::null());
    if !io::stderr().is_terminal() {
        command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Outlives this process's console and job instead of closing with it.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        if command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB).spawn().is_ok() { return Ok(()); }
        // A job that forbids breakaway: start inside it instead.
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map_err(|error| format!("重启失败：{error}"))?;
    Ok(())
}

fn resume_command(session: &Session, cwd: &std::path::Path) -> Option<String> {
    if !session.path.is_file() { return None; }
    let target = std::fs::canonicalize(&session.path).ok()?;
    let id = session.id();
    let by_id = !id.is_empty() && session::find(cwd, &id).ok()
        .and_then(|path| std::fs::canonicalize(path).ok()).is_some_and(|path| path == target);
    let argument = if by_id { id } else {
        let path = if session.path.is_absolute() { session.path.clone() } else { cwd.join(&session.path) };
        path.to_string_lossy().into_owned()
    };
    Some(format!("ashira --resume {}", shell_literal(&argument)))
}

/// Quote resume arguments for PowerShell on Windows and POSIX shells elsewhere.
fn shell_literal(value: &str) -> String {
    if !value.is_empty() && value.chars().all(|ch| ch.is_ascii_alphanumeric()
        || matches!(ch, '_' | '-' | '.' | '/' | ':') || (cfg!(windows) && ch == '\\'))
    { return value.to_owned(); }
    let escaped = if cfg!(windows) { value.replace('\'', "''") } else { value.replace('\'', "'\\''") };
    format!("'{escaped}'")
}

/// Print-mode readiness wait: `ASHIRA_MCP_TIMEOUT_MS` (0 = unbounded), else 30 s.
fn mcp_wait() -> std::time::Duration {
    match env::var("ASHIRA_MCP_TIMEOUT_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
        Some(0) => std::time::Duration::from_secs(86_400),
        Some(ms) => std::time::Duration::from_millis(ms),
        None => std::time::Duration::from_secs(30),
    }
}

/// `--models`: provider-grouped table.
async fn list_models(registry: &Registry, filter: &str, refresh: bool) -> ExitCode {
    for w in &registry.warnings {
        eprintln!("warning: {w}");
    }
    for e in registry.refresh(refresh).await {
        eprintln!("warning: {e}");
    }
    if let Err(e) = catalog::refresh(refresh).await {
        eprintln!("warning: {e}");
    }
    match catalog::status() {
        Some((n, age)) => eprintln!("models.dev: {n} models, updated {}h ago", age / 3600),
        None => eprintln!("models.dev: not downloaded yet"),
    }
    let default = registry.role("default").map(|s| config::split_level(s).0.to_owned());
    let needle = filter.to_ascii_lowercase();
    let entries: Vec<_> = registry.entries().into_iter().filter(|e| needle.is_empty() || e.selector().to_ascii_lowercase().contains(&needle)).collect();
    println!("# {}", registry.source().display());
    for (pid, api, age) in registry.provider_summary() {
        let rows: Vec<_> = entries.iter().filter(|e| e.provider == pid).collect();
        if rows.is_empty() && !needle.is_empty() {
            continue;
        }
        let fetched = age.map(|s| format!(" · discovered {}m ago", s / 60)).unwrap_or_default();
        println!("\n{pid}  ({api}{fetched})");
        for e in rows {
            let mark = if default.as_deref() == Some(e.selector().as_str()) { "*" } else { " " };
            let name = e.name.as_deref().map(|n| format!("  {n}")).unwrap_or_default();
            let max = e.max_tokens.map(|m| format!(" out {}", agent::tokens(u64::from(m)))).unwrap_or_default();
            println!("{mark} {:<40} {:>8}{max}{name}", e.selector(), agent::tokens(e.context_window));
        }
    }
    ExitCode::SUCCESS
}

/// Streams the answer to stdout; tool activity goes to stderr.
async fn print_mode(mut agent: Agent, prompt: String, mut events: mpsc::UnboundedReceiver<Event>) -> ExitCode {
    let cancel = agent.shared.turn_token();
    {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel.cancel();
            }
        });
    }
    let done = CancellationToken::new();
    let printer = tokio::spawn({
        let done = done.clone();
        let shared = agent.shared.clone();
        async move {
            let mut out = io::stdout();
            let mut at_line_start = true;
            loop {
                // After the turn, drain what is queued, then stop (MCP keeps a sender alive).
                let ev = tokio::select! {
                    biased;
                    ev = events.recv() => ev,
                    _ = done.cancelled() => events.try_recv().ok(),
                };
                let Some(ev) = ev else { break };
                match ev {
                    Event::Text(t) => {
                        let _ = out.write_all(t.as_bytes());
                        let _ = out.flush();
                        at_line_start = t.ends_with('\n');
                    }
                    Event::TextEnd if !at_line_start => {
                        let _ = out.write_all(b"\n");
                        at_line_start = true;
                    }
                    Event::ToolStart { call, summary } if !call.is_coordination() => {
                        let who = call.agent.as_ref().map(|a| format!("[{}] ", a.name)).unwrap_or_default();
                        eprintln!("● {who}{} {}", call.name, summary.lines().next().unwrap_or(""));
                    }
                    Event::ToolEnd { is_error: true, output, .. } => eprintln!("  ✗ {}", output.lines().next().unwrap_or("")),
                    Event::Notice(n) => eprintln!("{n}"),
                    Event::SubagentFinished(completion) if shared.subagents.accepts_completion(&completion) => {
                        let marker = match completion.status { subagents::Status::Completed => "✓", subagents::Status::Failed => "✗", _ => "−" };
                        let name = render::sanitize(&completion.identity.name).replace('\n', " ");
                        eprintln!("{marker} Background job {} [{}] {name} ({:.1}s)", completion.status.as_str(), completion.identity.role.as_str(), completion.elapsed_ms as f64 / 1000.0);
                        if let Some(error) = &completion.error {
                            eprintln!("  {}", render::sanitize(error).lines().next().unwrap_or(""));
                        }
                    }
                    // Print mode reserves stdout for the answer, never reasoning or internal receipts.
                    Event::Reasoning(_) | Event::ReasoningEnd => {}
                    Event::PeerMessage(message) if shared.subagents.accepts_message_notice(&message) => {
                        let sender = render::sanitize(&message.from_name).replace('\n', " ");
                        eprintln!("Incoming IRC message [{}] {sender}", message.from_role);
                        let preview: String = message.body.chars().take(2000).collect();
                        eprintln!("{}", render::sanitize(&preview));
                    }
                    _ => {}
                }
            }
        }
    });
    let result = agent.print_turn(api::user_text(&prompt), &cancel).await;
    drop(agent);
    done.cancel();
    let _ = printer.await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ashira: {e}");
            ExitCode::FAILURE
        }
    }
}
