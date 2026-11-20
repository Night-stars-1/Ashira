use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Component, Path},
};

use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::tools::{Output, LIMIT};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;

fn limit(input: &Value, key: &str) -> Result<usize, String> {
    match input.get(key) {
        None => Ok(DEFAULT_LIMIT),
        Some(value) => value.as_u64().filter(|&n| n > 0)
            .map(|n| n.min(MAX_LIMIT as u64) as usize)
            .ok_or_else(|| format!("{key} must be a positive integer")),
    }
}

fn matcher(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern).literal_separator(true).build()
        .map(|glob| glob.compile_matcher())
        .map_err(|err| format!("invalid glob pattern: {err}"))
}

fn matches_path(matcher: &GlobMatcher, pattern: &str, root: &Path, path: &Path) -> bool {
    let relative = if path == root { path.file_name().map(Path::new).unwrap_or(path) } else { path.strip_prefix(root).unwrap_or(path) };
    matcher.is_match(relative) || (!pattern.contains('/') && !pattern.contains('\\')
        && path.file_name().is_some_and(|name| matcher.is_match(Path::new(name))))
}

fn inside_git(path: &Path) -> bool {
    path.components().any(|component| matches!(component, Component::Normal(name) if name == ".git"))
}

fn root(input: &Value) -> Result<&Path, &'static str> {
    match input.get("path") {
        None => Ok(Path::new(".")),
        Some(value) => value.as_str().map(Path::new).ok_or("path must be a string"),
    }
}

fn excerpt(line: &str, start: usize) -> (&str, bool, bool) {
    const WINDOW: usize = 2000;
    if line.len() <= WINDOW { return (line, false, false); }
    let mut first = start.saturating_sub(WINDOW / 2);
    while !line.is_char_boundary(first) { first += 1; }
    let mut end = (first + WINDOW).min(line.len());
    while !line.is_char_boundary(end) { end -= 1; }
    (&line[first..end], first > 0, end < line.len())
}

fn walk(root: &Path) -> ignore::Walk {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(false).follow_links(false).filter_entry(|entry| entry.file_name() != ".git");
    builder.build()
}

fn record_error(errors: &mut Vec<String>, error: String) {
    if errors.len() < 20 {
        errors.push(error.chars().take(500).collect());
    } else if errors.len() == 20 {
        errors.push("Additional search errors omitted".into());
    }
}

fn finish(lines: Vec<String>, errors: Vec<String>, truncated: bool, cancelled: bool) -> Output {
    let mut text = lines.join("\n");
    if text.is_empty() { text.push_str("(no matches)"); }
    if truncated { text.push_str("\n[Results truncated]"); }
    if cancelled { text.push_str("\n[Search cancelled]"); }
    if !errors.is_empty() {
        text.push_str("\n[Search errors]\n");
        text.push_str(&errors.join("\n"));
    }
    if cancelled || !errors.is_empty() { Output::err(text) } else { Output::ok(text) }
}

pub fn glob(input: &Value, cancel: &CancellationToken) -> Output {
    let Some(pattern) = input.get("pattern").and_then(Value::as_str) else { return Output::err("pattern must be a string"); };
    let matcher = match matcher(pattern) { Ok(m) => m, Err(e) => return Output::err(e) };
    let max = match limit(input, "limit") { Ok(n) => n, Err(e) => return Output::err(e) };
    let root = match root(input) { Ok(root) => root, Err(err) => return Output::err(err) };
    if inside_git(root) { return Output::err("searching inside .git is not allowed"); }
    let mut lines = Vec::new();
    let mut errors = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    for entry in walk(root) {
        if cancel.is_cancelled() { break; }
        match entry {
            Err(err) => record_error(&mut errors, err.to_string()),
            Ok(entry) => {
                if let Some(err) = entry.error() { record_error(&mut errors, err.to_string()); }
                let path = entry.path();
                if (path == root && entry.file_type().is_some_and(|kind| kind.is_dir())) || !matches_path(&matcher, pattern, root, path) { continue; }
                let line = path.display().to_string();
                if lines.len() >= max || bytes + line.len() + 1 > LIMIT { truncated = true; break; }
                bytes += line.len() + 1;
                lines.push(line);
            }
        }
    }
    finish(lines, errors, truncated, cancel.is_cancelled())
}

pub fn grep(input: &Value, cancel: &CancellationToken) -> Output {
    let Some(pattern) = input.get("pattern").and_then(Value::as_str) else { return Output::err("pattern must be a string"); };
    let case_sensitive = match input.get("case_sensitive") {
        None => true,
        Some(v) => match v.as_bool() { Some(b) => b, None => return Output::err("case_sensitive must be a boolean") },
    };
    let regex = match RegexBuilder::new(pattern).case_insensitive(!case_sensitive).build() {
        Ok(regex) => regex, Err(err) => return Output::err(format!("invalid regex: {err}")),
    };
    let glob_pattern = match input.get("glob") {
        None => None,
        Some(v) => match v.as_str() { Some(s) => Some(s), None => return Output::err("glob must be a string") },
    };
    let glob = match glob_pattern.map(matcher).transpose() { Ok(m) => m, Err(e) => return Output::err(e) };
    let max = match limit(input, "max_matches") { Ok(n) => n, Err(e) => return Output::err(e) };
    let root = match root(input) { Ok(root) => root, Err(err) => return Output::err(err) };
    if inside_git(root) { return Output::err("searching inside .git is not allowed"); }
    let mut lines = Vec::new();
    let mut errors = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    'walk: for entry in walk(root) {
        if cancel.is_cancelled() { break; }
        let entry = match entry { Ok(e) => e, Err(e) => { record_error(&mut errors, e.to_string()); continue; } };
        if let Some(err) = entry.error() { record_error(&mut errors, err.to_string()); }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) { continue; }
        let path = entry.path();
        if let (Some(m), Some(p)) = (&glob, glob_pattern) && !matches_path(m, p, root, path) { continue; }
        let file = match File::open(path) { Ok(f) => f, Err(e) => { record_error(&mut errors, format!("{}: {e}", path.display())); continue; } };
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut index = 0;
        loop {
            if cancel.is_cancelled() { break 'walk; }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => index += 1,
                Err(e) => { record_error(&mut errors, format!("{}:{}: {e}", path.display(), index + 1)); break; }
            }
            if line.ends_with('\n') { line.pop(); if line.ends_with('\r') { line.pop(); } }
            if line.contains('\0') { break; }
            let Some(found) = regex.find(&line) else { continue; };
            let (snippet, before, after) = excerpt(&line, found.start());
            let result = format!("{}:{}:{}{}{}", path.display(), index,
                if before { "[... omitted ...]" } else { "" }, snippet,
                if after { "[... omitted ...]" } else { "" });
            if lines.len() >= max || bytes + result.len() + 1 > LIMIT { truncated = true; break 'walk; }
            bytes += result.len() + 1;
            lines.push(result);
        }
    }
    finish(lines, errors, truncated, cancel.is_cancelled())
}
