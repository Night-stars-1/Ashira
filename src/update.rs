//! `ashira update [VERSION]`: replaces this executable with a GitHub Release build of
//! ashira-core, checked against the release's SHA256SUMS.txt, and moves the `--web`
//! folder (config.yml `web:`) to the same release. Startup also checks for a newer
//! release in the background, at most once a day.

use std::{
    env, fs,
    io::{self, Cursor, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "Night-stars-1/Ashira";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Startup checks run at most this often; the result is cached under `~/.ashira`.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const PLATFORM: Option<&str> = Some("windows-x64");
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PLATFORM: Option<&str> = Some("linux-x64");
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const PLATFORM: Option<&str> = Some("macos-arm64");
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
)))]
const PLATFORM: Option<&str> = None;

const BINARY: &str = if cfg!(windows) { "ashira.exe" } else { "ashira" };
const ARCHIVE: &str = if cfg!(windows) { "zip" } else { "tar.gz" };

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("ashira/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())
}

async fn release(http: &reqwest::Client, tag: Option<&str>) -> Result<Release, String> {
    let url = match tag {
        Some(tag) => format!("https://api.github.com/repos/{REPO}/releases/tags/{tag}"),
        None => format!("https://api.github.com/repos/{REPO}/releases/latest"),
    };
    let response = http.get(&url).header("accept", "application/vnd.github+json").send().await.map_err(|e| format!("无法访问 GitHub：{e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(match tag { Some(tag) => format!("没有 {tag} 这个版本"), None => "还没有发布任何版本".into() });
    }
    if !response.status().is_success() {
        return Err(format!("GitHub 返回 {}", response.status()));
    }
    let body = response.bytes().await.map_err(|e| format!("无法读取 Release 信息：{e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("无法解析 Release 信息：{e}"))
}

/// The download in progress: bytes received and size if known. `GET /v1/update` reads it.
static DOWNLOADING: std::sync::Mutex<Option<(u64, Option<u64>)>> = std::sync::Mutex::new(None);

/// `(received, total)` of the download in progress, else `None`.
pub fn downloading() -> Option<(u64, Option<u64>)> {
    *DOWNLOADING.lock().ok()?
}

/// Publishes one download's progress; cleared when dropped, even if the download is abandoned.
struct Published;

impl Published {
    fn start(total: Option<u64>) -> Self {
        if let Ok(mut slot) = DOWNLOADING.lock() { *slot = Some((0, total)); }
        Self
    }

    fn received(&self, bytes: u64) {
        if let Ok(mut slot) = DOWNLOADING.lock() && let Some(state) = slot.as_mut() { state.0 = bytes; }
    }
}

impl Drop for Published {
    fn drop(&mut self) {
        if let Ok(mut slot) = DOWNLOADING.lock() { *slot = None; }
    }
}

/// Retried: the asset CDN behind GitHub's redirect drops connections now and then.
/// `progress` draws a bar on stderr while the body arrives, when stderr is a terminal.
async fn download(http: &reqwest::Client, url: &str, progress: bool) -> Result<Vec<u8>, String> {
    let progress = progress && io::stderr().is_terminal();
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 { tokio::time::sleep(Duration::from_secs(2)).await; }
        let mut bar = progress.then(Progress::new);
        let result = async {
            let mut response = http.get(url).send().await?.error_for_status()?;
            let total = response.content_length();
            let mut body = Vec::with_capacity(total.unwrap_or(0) as usize);
            let published = Published::start(total);
            while let Some(chunk) = response.chunk().await? {
                body.extend_from_slice(&chunk);
                published.received(body.len() as u64);
                if let Some(bar) = &mut bar { bar.update(body.len() as u64, total, false); }
            }
            if let Some(bar) = &mut bar { bar.update(body.len() as u64, total, true); }
            Ok::<_, reqwest::Error>(body)
        }.await;
        match result {
            Ok(body) => return Ok(body),
            Err(e) => {
                // End a half-drawn bar so the next message starts on its own line.
                if bar.is_some_and(|bar| bar.width > 0) { eprintln!(); }
                // reqwest's top-level message hides the cause ("error sending request"); the source names it.
                last = std::error::Error::source(&e).map_or_else(|| e.to_string(), |cause| format!("{e}：{cause}"));
                if attempt < 2 { eprintln!("下载中断，重试（{}/2）：{last}", attempt + 1); }
            }
        }
    }
    Err(format!("下载失败：{last}"))
}

/// One stderr line redrawn in place: `[#####-----]  45%  3.2/7.1 MB  1.2 MB/s`.
/// Overwrites with `\r` and space padding rather than VT escapes, which Windows
/// consoles do not process unless asked to.
struct Progress {
    start: Instant,
    drawn: Option<Instant>,
    /// Characters of the last line drawn; 0 before the first draw.
    width: usize,
}

impl Progress {
    const BAR: usize = 30;

    fn new() -> Self {
        Self { start: Instant::now(), drawn: None, width: 0 }
    }

    /// Redraws at most every 100 ms; `done` always draws and ends the line.
    fn update(&mut self, received: u64, total: Option<u64>, done: bool) {
        let now = Instant::now();
        if !done && self.drawn.is_some_and(|at| now - at < Duration::from_millis(100)) { return; }
        self.drawn = Some(now);
        let elapsed = (now - self.start).as_secs_f64();
        let speed = if elapsed > 0.0 { (received as f64 / elapsed) as u64 } else { 0 };
        let line = match total.filter(|&total| total > 0) {
            Some(total) => {
                let ratio = (received as f64 / total as f64).min(1.0);
                let filled = (ratio * Self::BAR as f64).round() as usize;
                format!("[{}{}] {:>3.0}%  {:.1}/{}  {}/s", "#".repeat(filled), "-".repeat(Self::BAR - filled), ratio * 100.0, received as f64 / 1_048_576.0, mb(total), mb(speed))
            }
            None => format!("{}  {}/s", mb(received), mb(speed)),
        };
        let width = line.chars().count();
        let mut err = io::stderr().lock();
        let _ = write!(err, "\r{line}{:pad$}", "", pad = self.width.saturating_sub(width));
        if done { let _ = writeln!(err); }
        let _ = err.flush();
        self.width = width;
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

/// `1.2.3` from `v1.2.3`; pre-release suffixes are ignored for ordering.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

fn normalize_tag(tag: &str) -> String {
    if tag.starts_with('v') { tag.to_owned() } else { format!("v{tag}") }
}

/// Runs `ashira update [VERSION]`; returns the message to print.
pub async fn run(requested: Option<&str>) -> Result<String, String> {
    let platform = PLATFORM.ok_or("这个平台没有预编译的 ashira-core，请从源码编译")?;
    let http = http()?;
    let tag = requested.map(normalize_tag);
    let release = release(&http, tag.as_deref()).await?;
    if tag.is_none() && !newer(&release.tag_name, CURRENT) {
        remember(&release.tag_name);
        let mut message = format!("已是最新版本 v{CURRENT}");
        // The core is current but its page may lag (copied by hand, or a failed earlier update).
        if let Some(dir) = crate::config::web_dir().filter(|dir| crate::web::installed_version(dir).as_deref() != Some(CURRENT)) {
            match install_web(&dir).await {
                Ok(tag) => message.push_str(&format!("\n网页已更新到 {tag}（{}）", dir.display())),
                Err(error) => message.push_str(&format!("\n网页没有更新：{error}")),
            }
        }
        return Ok(message);
    }
    let suffix = format!("-{platform}.{ARCHIVE}");
    let asset = release.assets.iter().find(|a| a.name.starts_with("ashira-core-") && a.name.ends_with(&suffix))
        .ok_or_else(|| format!("{} 里没有 {platform} 版 ashira-core", release.tag_name))?;
    eprintln!("下载 {}", asset.name);
    let archive = download(&http, &asset.browser_download_url, true).await?;
    let sums = checksums(&http, &release).await?;
    verify(&archive, &asset.name, &sums)?;
    let binary = extract(&archive)?;
    let target = env::current_exe().map_err(|e| format!("找不到当前程序：{e}"))?;
    replace(&target, &binary)?;
    remember(&release.tag_name);
    let mut message = format!("已更新到 {}（{}）", release.tag_name, target.display());
    // The page follows the core: a newer page may call routes an older core lacks.
    if let Some(dir) = crate::config::web_dir() {
        match install_web_from(&http, &release, &sums, &dir).await {
            Ok(()) => message.push_str(&format!("\n网页已更新到 {}（{}）", release.tag_name, dir.display())),
            Err(error) => message.push_str(&format!("\n网页没有更新：{error}")),
        }
    }
    Ok(message)
}

async fn checksums(http: &reqwest::Client, release: &Release) -> Result<String, String> {
    let sums = release.assets.iter().find(|a| a.name == "SHA256SUMS.txt")
        .ok_or_else(|| format!("{} 没有 SHA256SUMS.txt，无法校验，已取消", release.tag_name))?;
    String::from_utf8(download(http, &sums.browser_download_url, false).await?).map_err(|_| "SHA256SUMS.txt 不是文本".into())
}

/// Puts the web build of this core's release into `dir`, for the page's 更新网页 button.
pub async fn install_web(dir: &Path) -> Result<String, String> {
    let http = http()?;
    let release = release(&http, Some(&normalize_tag(CURRENT))).await?;
    let sums = checksums(&http, &release).await?;
    install_web_from(&http, &release, &sums, dir).await?;
    Ok(release.tag_name)
}

async fn install_web_from(http: &reqwest::Client, release: &Release, sums: &str, dir: &Path) -> Result<(), String> {
    let asset = release.assets.iter().find(|a| a.name.starts_with("ashira-web-") && a.name.ends_with(".zip"))
        .ok_or_else(|| format!("{} 里没有网页包", release.tag_name))?;
    let archive = download(http, &asset.browser_download_url, false).await?;
    verify(&archive, &asset.name, sums)?;
    let (dir, tag) = (dir.to_path_buf(), release.tag_name.clone());
    tokio::task::spawn_blocking(move || replace_web(&archive, &dir, &tag)).await.map_err(|e| e.to_string())?
}

/// Unpacks next to `dir`, then swaps the folders, so a failed unpack leaves the old page
/// in place and a browser never loads half of each.
fn replace_web(archive: &[u8], dir: &Path, tag: &str) -> Result<(), String> {
    let parent = dir.parent().ok_or("网页目录无效")?;
    let name = dir.file_name().ok_or("网页目录无效")?.to_string_lossy().into_owned();
    let staged = parent.join(format!(".{name}.new"));
    let old = parent.join(format!(".{name}.old"));
    let _ = fs::remove_dir_all(&staged);
    let _ = fs::remove_dir_all(&old);
    fs::create_dir_all(&staged).map_err(|e| format!("无法写入 {}：{e}", parent.display()))?;
    let result = unpack_web(archive, &staged).and_then(|()| {
        crate::web::mark(&staged, tag).map_err(|e| e.to_string())?;
        if dir.exists() { fs::rename(dir, &old).map_err(|e| format!("无法替换 {}：{e}", dir.display()))?; }
        fs::rename(&staged, dir).map_err(|e| {
            let _ = fs::rename(&old, dir);
            format!("无法替换 {}：{e}", dir.display())
        })
    });
    let _ = fs::remove_dir_all(&staged);
    let _ = fs::remove_dir_all(&old);
    result
}

/// The zip holds one folder (`ashira-web-<tag>/`); its contents go straight into `into`.
fn unpack_web(archive: &[u8], into: &Path) -> Result<(), String> {
    let invalid = |e: &dyn std::fmt::Display| format!("网页包损坏：{e}");
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| invalid(&e))?;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).map_err(|e| invalid(&e))?;
        // `enclosed_name` rejects absolute paths and `..`; the first component is the wrapper folder.
        let Some(path) = file.enclosed_name() else { return Err(invalid(&"非法路径")) };
        let relative: PathBuf = path.components().skip(1).collect();
        if relative.as_os_str().is_empty() { continue; }
        let target = into.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let mut out = fs::File::create(&target).map_err(|e| e.to_string())?;
        io::copy(&mut file, &mut out).map_err(|e| invalid(&e))?;
    }
    if !crate::web::installed(into) { return Err("网页包里没有 index.html".into()); }
    Ok(())
}

fn verify(data: &[u8], name: &str, sums: &str) -> Result<(), String> {
    let expected = sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.trim().to_ascii_lowercase())
    }).ok_or_else(|| format!("SHA256SUMS.txt 里没有 {name}"))?;
    let actual: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected { return Err(format!("{name} 的 SHA256 校验失败，已取消")); }
    Ok(())
}

/// The executable from the release archive (`ashira-core-<tag>-<platform>/ashira[.exe]`).
fn extract(archive: &[u8]) -> Result<Vec<u8>, String> {
    let invalid = |e: &dyn std::fmt::Display| format!("压缩包损坏：{e}");
    if cfg!(windows) {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| invalid(&e))?;
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).map_err(|e| invalid(&e))?;
            let found = file.is_file() && file.enclosed_name().is_some_and(|p| p.file_name().is_some_and(|n| n == BINARY));
            if found {
                let mut data = Vec::new();
                file.read_to_end(&mut data).map_err(|e| invalid(&e))?;
                return Ok(data);
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in tar.entries().map_err(|e| invalid(&e))? {
            let mut entry = entry.map_err(|e| invalid(&e))?;
            let found = entry.header().entry_type().is_file() && entry.path().is_ok_and(|p| p.file_name().is_some_and(|n| n == BINARY));
            if found {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| invalid(&e))?;
                return Ok(data);
            }
        }
    }
    Err(format!("压缩包里没有 {BINARY}"))
}

/// Writes the new build next to `target` and swaps it in. A running executable cannot be
/// overwritten on Windows but can be renamed, so the old one moves aside first.
fn replace(target: &Path, binary: &[u8]) -> Result<(), String> {
    let dir = target.parent().ok_or("程序路径无效")?;
    let denied = |e: io::Error| if e.kind() == io::ErrorKind::PermissionDenied {
        format!("没有权限写入 {}，请用有权限的账户运行", dir.display())
    } else { e.to_string() };
    let staged = dir.join(format!(".{BINARY}.new"));
    fs::write(&staged, binary).map_err(denied)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).map_err(denied)?;
    }
    #[cfg(windows)]
    {
        let old = dir.join(format!(".{BINARY}.old"));
        // Left by the previous update, which could not delete itself while running.
        let _ = fs::remove_file(&old);
        if let Err(e) = fs::rename(target, &old) { let _ = fs::remove_file(&staged); return Err(denied(e)); }
        if let Err(e) = fs::rename(&staged, target) {
            let _ = fs::rename(&old, target);
            let _ = fs::remove_file(&staged);
            return Err(denied(e));
        }
        let _ = fs::remove_file(&old);
    }
    #[cfg(not(windows))]
    fs::rename(&staged, target).map_err(|e| { let _ = fs::remove_file(&staged); denied(e) })?;
    Ok(())
}

/// The running program's file as it was at startup: `ashira update` replacing it changes this.
static STARTED_AS: std::sync::OnceLock<Option<(SystemTime, u64)>> = std::sync::OnceLock::new();

/// This program's path. Once `ashira update` replaced it, Linux reports the running (now
/// unlinked) file as `<path> (deleted)`; the path itself holds the new build.
pub fn program() -> io::Result<PathBuf> {
    let exe = env::current_exe()?;
    Ok(match exe.to_str().and_then(|path| path.strip_suffix(" (deleted)")) {
        Some(path) => PathBuf::from(path),
        None => exe,
    })
}

fn executable_stamp() -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(program().ok()?).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Called once at startup, before anything can replace the file.
pub fn remember_started() {
    let _ = STARTED_AS.set(executable_stamp());
}

/// Whether the program on disk is no longer the one running (an update is waiting for a restart).
pub fn replaced_since_start() -> bool {
    matches!((STARTED_AS.get(), executable_stamp()), (Some(Some(started)), Some(now)) if *started != now)
}

/// Removes the previous executable that a Windows update had to leave behind while it ran.
pub fn clean_leftover() {
    #[cfg(windows)]
    if let Some(dir) = env::current_exe().ok().as_deref().and_then(Path::parent) {
        let _ = fs::remove_file(dir.join(format!(".{BINARY}.old")));
    }
}

fn cache() -> PathBuf {
    crate::session::home().join("update.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(serde::Serialize, Deserialize)]
struct Cached {
    checked: u64,
    latest: String,
}

fn remember(latest: &str) {
    let _ = fs::create_dir_all(crate::session::home());
    let _ = fs::write(cache(), serde_json::json!({"checked": now(), "latest": latest}).to_string());
}

fn read_cache() -> Option<Cached> {
    fs::read(cache()).ok().and_then(|data| serde_json::from_slice(&data).ok())
}

/// Refreshes the cached newest release tag from GitHub unless it is under a day old.
async fn refresh() -> Option<String> {
    if let Some(cached) = read_cache() && now().saturating_sub(cached.checked) < CHECK_EVERY.as_secs() {
        return Some(cached.latest);
    }
    let http = http().ok()?;
    let tag = tokio::time::timeout(Duration::from_secs(10), release(&http, None)).await.ok()?.ok()?.tag_name;
    remember(&tag);
    Some(tag)
}

/// Startup check (TUI and `--serve`): a one-line notice when a newer release exists.
pub async fn notice() -> Option<String> {
    PLATFORM?;
    let latest = refresh().await?;
    newer(&latest, CURRENT).then(|| format!("有新版本 {latest}（当前 v{CURRENT}），运行 `ashira update` 更新"))
}

/// The newer release tag from the last check, without touching the network; for `GET /v1`.
pub fn cached() -> Option<String> {
    PLATFORM?;
    read_cache().map(|c| c.latest).filter(|latest| newer(latest, CURRENT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_versions_numerically() {
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("v1.0.0", "0.99.99"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("nightly", "0.1.0"));
    }

    #[test]
    fn checksum_must_match_the_named_file() {
        let data = b"ashira";
        let hash: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
        let sums = format!("{hash}  ashira-core-v1.0.0-linux-x64.tar.gz\n0000  other.zip\n");
        assert!(verify(data, "ashira-core-v1.0.0-linux-x64.tar.gz", &sums).is_ok());
        assert!(verify(b"tampered", "ashira-core-v1.0.0-linux-x64.tar.gz", &sums).is_err());
        assert!(verify(data, "missing.zip", &sums).is_err());
    }

    #[test]
    fn web_update_swaps_the_folder_and_keeps_it_on_a_bad_archive() {
        use std::io::Write as _;
        let root = std::env::temp_dir().join(format!("ashira-webup-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let dir = root.join("web");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), "old").unwrap();
        fs::write(dir.join("stale.js"), "gone after update").unwrap();

        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ashira-web-v0.3.0/index.html", options).unwrap();
        zip.write_all(b"new").unwrap();
        zip.start_file("ashira-web-v0.3.0/assets/app.js", options).unwrap();
        zip.write_all(b"js").unwrap();
        let archive = zip.finish().unwrap().into_inner();

        replace_web(&archive, &dir, "v0.3.0").unwrap();
        assert_eq!(fs::read_to_string(dir.join("index.html")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dir.join("assets/app.js")).unwrap(), "js");
        assert!(!dir.join("stale.js").exists());
        assert_eq!(crate::web::installed_version(&dir).as_deref(), Some("0.3.0"));

        // A broken archive leaves the installed page untouched.
        assert!(replace_web(b"not a zip", &dir, "v0.4.0").is_err());
        assert_eq!(fs::read_to_string(dir.join("index.html")).unwrap(), "new");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1, "no staging folders left behind");
        fs::remove_dir_all(&root).unwrap();
    }
}
