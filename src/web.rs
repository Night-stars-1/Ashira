//! `--web <DIR>`: the API port also serves the web client's build, so one address opens
//! the page and its API. The folder is remembered as config.yml `web:` for later starts
//! and for `ashira update`, which keeps it at the core's version (see update.rs).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Written by the installer next to `index.html`: the release the folder holds.
const MARKER: &str = ".ashira-version";

/// The folder to serve: `--web DIR` (absolute, saved to config.yml when it changed),
/// else the saved one.
pub fn folder(requested: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(dir) = requested else { return Ok(crate::config::web_dir()) };
    let dir = std::path::absolute(dir).map_err(|e| format!("--web {dir}: {e}"))?;
    if crate::config::web_dir().as_deref() != Some(dir.as_path()) {
        crate::config::set_web_dir(&dir)?;
    }
    Ok(Some(dir))
}

pub fn installed(dir: &Path) -> bool {
    dir.join("index.html").is_file()
}

/// Empty, or installed by `ashira` at a release other than this core's. A hand-copied
/// build (no version marker) is the owner's choice and is left alone.
pub fn needs_install(dir: &Path) -> bool {
    !installed(dir) || installed_version(dir).is_some_and(|version| version != crate::update::CURRENT)
}

/// The release version (`0.2.4`) the installer put there; None for a hand-copied build.
pub fn installed_version(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(MARKER)).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

pub(crate) fn mark(dir: &Path, version: &str) -> io::Result<()> {
    fs::write(dir.join(MARKER), version.trim_start_matches('v'))
}

/// The file for a request path. Extensionless paths the build lacks get `index.html`, so
/// a reload on a client route still opens the page; `..`, drive prefixes and dotfiles
/// (the version marker) never resolve.
pub fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let mut file = root.to_path_buf();
    let mut last = "";
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if part.starts_with('.') || part.contains(['\\', ':']) { return None; }
        file.push(part);
        last = part;
    }
    if file.is_file() { return Some(file); }
    let index = root.join("index.html");
    (!last.contains('.') && index.is_file()).then_some(index)
}

pub fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_files_and_routes_but_never_escapes() {
        let root = std::env::temp_dir().join(format!("ashira-web-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("index.html"), "<html>").unwrap();
        fs::write(root.join("assets/app.js"), "1").unwrap();
        mark(&root, "v0.2.4").unwrap();
        fs::write(root.parent().unwrap().join("secret.txt"), "no").unwrap();

        assert_eq!(resolve(&root, "/"), Some(root.join("index.html")));
        assert_eq!(resolve(&root, "/assets/app.js"), Some(root.join("assets").join("app.js")));
        // A client route reloads into the page; a missing asset is a 404.
        assert_eq!(resolve(&root, "/sessions/abc"), Some(root.join("index.html")));
        assert_eq!(resolve(&root, "/assets/missing.js"), None);
        for escape in ["/../secret.txt", "/assets/../../secret.txt", "/..\\secret.txt", "/C:/Windows/win.ini", "/.ashira-version"] {
            assert_eq!(resolve(&root, escape), None, "{escape}");
        }
        assert_eq!(installed_version(&root).as_deref(), Some("0.2.4"));
        fs::remove_dir_all(&root).unwrap();
    }
}
