//! `ashira update [VERSION]`: replaces this executable with a GitHub Release build of
//! ashira-core, checked against the release's SHA256SUMS.txt. Startup also checks for
//! a newer release in the background, at most once a day.

use std::{
    env, fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
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

/// Retried: the asset CDN behind GitHub's redirect drops connections now and then.
async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 { tokio::time::sleep(Duration::from_secs(2)).await; }
        let result = async {
            let response = http.get(url).send().await?.error_for_status()?;
            response.bytes().await
        }.await;
        match result {
            Ok(bytes) => return Ok(bytes.to_vec()),
            // reqwest's top-level message hides the cause ("error sending request"); the source names it.
            Err(e) => last = std::error::Error::source(&e).map_or_else(|| e.to_string(), |cause| format!("{e}：{cause}")),
        }
    }
    Err(format!("下载失败：{last}"))
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
        return Ok(format!("已是最新版本 v{CURRENT}"));
    }
    let suffix = format!("-{platform}.{ARCHIVE}");
    let asset = release.assets.iter().find(|a| a.name.starts_with("ashira-core-") && a.name.ends_with(&suffix))
        .ok_or_else(|| format!("{} 里没有 {platform} 版 ashira-core", release.tag_name))?;
    eprintln!("下载 {}", asset.name);
    let archive = download(&http, &asset.browser_download_url).await?;
    let sums = release.assets.iter().find(|a| a.name == "SHA256SUMS.txt")
        .ok_or_else(|| format!("{} 没有 SHA256SUMS.txt，无法校验，已取消", release.tag_name))?;
    let sums = String::from_utf8(download(&http, &sums.browser_download_url).await?).map_err(|_| "SHA256SUMS.txt 不是文本")?;
    verify(&archive, &asset.name, &sums)?;
    let binary = extract(&archive)?;
    let target = env::current_exe().map_err(|e| format!("找不到当前程序：{e}"))?;
    replace(&target, &binary)?;
    remember(&release.tag_name);
    Ok(format!("已更新到 {}（{}）", release.tag_name, target.display()))
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
}
