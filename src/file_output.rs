use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use similar::{ChangeTag, DiffTag, TextDiff};

#[derive(Clone, Debug)]
pub struct FileOutput {
    pub path: String,
    pub detail: FileDetail,
}

#[derive(Clone, Debug)]
pub enum FileDetail {
    Summary,
    Read { start: Option<u64>, end: Option<u64>, directory: bool },
    Change(FileChange),
}

#[derive(Clone, Debug)]
pub struct FileChange {
    pub added: Option<usize>,
    pub removed: Option<usize>,
    pub rows: Vec<DiffRow>,
    pub omitted_rows: usize,
    pub full_diff_path: Option<PathBuf>,
    pub notice: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind { Context, Added, Removed, Hunk, NoNewline }

#[derive(Clone, Debug)]
pub struct DiffRow {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub kind: DiffKind,
    pub text: String,
    pub highlights: Vec<Range<usize>>,
}

const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 256 * 1024;
const MAX_LINE_BYTES: usize = 8192;

impl FileChange {
    pub fn unavailable(notice: impl Into<String>) -> Self {
        Self { added: None, removed: None, rows: Vec::new(), omitted_rows: 0, full_diff_path: None, notice: Some(notice.into()) }
    }
}
// Create a complete diff artifact only when its bounded preview is truncated.
fn create_artifact() -> std::io::Result<(PathBuf, BufWriter<File>)> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("ashira");
    fs::create_dir_all(&dir)?;
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    loop {
        let path = dir.join(format!("{millis}-diff-{}-{}.txt", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, BufWriter::new(file))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

fn write_row(writer: &mut BufWriter<File>, kind: DiffKind, text: &str) -> std::io::Result<()> {
    let prefix = match kind { DiffKind::Context => " ", DiffKind::Added => "+", DiffKind::Removed => "-", DiffKind::Hunk | DiffKind::NoNewline => "" };
    writeln!(writer, "{prefix}{text}")
}

fn range(start: usize, end: usize) -> String {
    let len = end - start;
    let line = if len == 0 { start } else { start + 1 };
    if len == 1 { line.to_string() } else { format!("{line},{len}") }
}

fn push_highlight(ranges: &mut Vec<Range<usize>>, start: usize, end: usize) {
    if let Some(last) = ranges.last_mut() && last.end == start {
        last.end = end;
    } else {
        ranges.push(start..end);
    }
}

// Pair lines only within a real replacement operation. Bound both per-pair and
// total work so large changes remain ordinary line diffs.
fn inline_ranges(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let diff = TextDiff::from_chars(old, new);
    let (mut old_pos, mut new_pos) = (0, 0);
    let (mut old_ranges, mut new_ranges) = (Vec::new(), Vec::new());
    for item in diff.iter_all_changes() {
        let len = item.value().len();
        match item.tag() {
            ChangeTag::Equal => { old_pos += len; new_pos += len; }
            ChangeTag::Delete => { push_highlight(&mut old_ranges, old_pos, old_pos + len); old_pos += len; }
            ChangeTag::Insert => { push_highlight(&mut new_ranges, new_pos, new_pos + len); new_pos += len; }
        }
    }
    (old_ranges, new_ranges)
}

pub fn change(path: &str, before: &str, after: &str) -> FileChange {
    let diff = TextDiff::from_lines(before, after);
    let mut added = 0;
    let mut removed = 0;
    for op in diff.ops() {
        match op.tag() {
            DiffTag::Delete => removed += op.old_range().len(),
            DiffTag::Insert => added += op.new_range().len(),
            DiffTag::Replace => { removed += op.old_range().len(); added += op.new_range().len(); }
            DiffTag::Equal => {}
        }
    }
    let mut artifact = None;
    let mut artifact_failed = false;
    let mut rows: Vec<DiffRow> = Vec::new();
    let mut bytes = 0usize;
    let mut omitted_rows = 0usize;
    let mut clipped = false;
    let mut started = false;
    let mut emit = |kind: DiffKind, old: Option<usize>, new: Option<usize>, text: &str, highlights: &[Range<usize>]| {
        let omitted = rows.len() >= MAX_ROWS || bytes >= MAX_BYTES;
        let mut end = if omitted { 0 } else { text.len().min(MAX_LINE_BYTES.min(MAX_BYTES - bytes)) };
        while !text.is_char_boundary(end) { end -= 1; }
        if !started && (omitted || end < text.len()) {
            started = true;
            match create_artifact() {
                Ok((path_buf, mut writer)) => {
                    let result = writeln!(writer, "--- a/{path}\n+++ b/{path}")
                        .and_then(|_| rows.iter().try_for_each(|row| write_row(&mut writer, row.kind, &row.text)));
                    if result.is_err() { artifact_failed = true; }
                    artifact = Some((path_buf, writer));
                }
                Err(_) => artifact_failed = true,
            }
        }
        if !artifact_failed
            && let Some((_, writer)) = &mut artifact
            && write_row(writer, kind, text).is_err()
        {
            artifact_failed = true;
        }
        if omitted {
            omitted_rows += 1;
            return;
        }
        if end < text.len() { clipped = true; }
        bytes += end;
        let highlights = highlights.iter().filter_map(|r| {
            let start = r.start.min(end);
            let finish = r.end.min(end);
            (start < finish).then_some(start..finish)
        }).collect();
        rows.push(DiffRow { old, new, kind, text: text[..end].to_owned(), highlights });
    };
    let mut inline_budget = 2_000_000usize;
    for group in diff.grouped_ops(3) {
        let first = &group[0];
        let last = &group[group.len() - 1];
        let header = format!("@@ -{} +{} @@", range(first.old_range().start, last.old_range().end), range(first.new_range().start, last.new_range().end));
        emit(DiffKind::Hunk, None, None, &header, &[]);
        for op in &group {
            let old_range = op.old_range();
            let new_range = op.new_range();
            let pairs = if op.tag() == DiffTag::Replace && old_range.len() <= 32 && new_range.len() <= 32 {
                old_range.len().min(new_range.len())
            } else { 0 };
            let mut old_highlights = Vec::with_capacity(pairs);
            let mut new_highlights = Vec::with_capacity(pairs);
            for i in 0..pairs {
                let old = diff.old_slices()[old_range.start + i];
                let new = diff.new_slices()[new_range.start + i];
                let work = old.len().saturating_mul(new.len());
                let highlights = if old.len() <= 4096 && new.len() <= 4096 && work <= inline_budget {
                    inline_budget -= work.max(1).min(inline_budget);
                    inline_ranges(old, new)
                } else { (Vec::new(), Vec::new()) };
                old_highlights.push(highlights.0);
                new_highlights.push(highlights.1);
            }
            for item in diff.iter_changes(op) {
                let kind = match item.tag() { ChangeTag::Equal => DiffKind::Context, ChangeTag::Delete => DiffKind::Removed, ChangeTag::Insert => DiffKind::Added };
                let value = item.value();
                let text = value.strip_suffix('\n').unwrap_or(value);
                let highlights = match item.tag() {
                    ChangeTag::Delete => item.old_index().and_then(|index| old_highlights.get(index - old_range.start)),
                    ChangeTag::Insert => item.new_index().and_then(|index| new_highlights.get(index - new_range.start)),
                    ChangeTag::Equal => None,
                }.map(Vec::as_slice).unwrap_or(&[]);
                emit(kind, item.old_index().map(|n| n + 1), item.new_index().map(|n| n + 1), text, highlights);
                if item.missing_newline() { emit(DiffKind::NoNewline, None, None, "\\ No newline at end of file", &[]); }
            }
        }
    }
    drop(emit);
    if let Some((_, writer)) = &mut artifact && writer.flush().is_err() {
        artifact_failed = true;
    }
    let truncated = omitted_rows > 0 || clipped;
    let mut full_diff_path = None;
    if let Some((path, writer)) = artifact {
        drop(writer);
        if truncated && !artifact_failed { full_diff_path = Some(path); } else { let _ = fs::remove_file(path); }
    }
    let notice = if truncated {
        Some(if artifact_failed { "Diff preview truncated; saving the full diff failed." } else { "Diff preview truncated; full diff saved to the artifact." }.to_string())
    } else { None };
    FileChange { added: Some(added), removed: Some(removed), rows, omitted_rows, full_diff_path, notice }
}
