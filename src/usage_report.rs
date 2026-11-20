//! Usage across every saved session for Settings → 用量. Request records are
//! summed per UTC hour and model; the page groups hours into local days.
//! Subagent requests are already logged in their parent session.

use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::UNIX_EPOCH,
};

use serde_json::{Value, json};

use crate::{api::Usage, config, session, stats::RequestStats};

#[derive(Default)]
struct Bucket {
    usage: Usage,
    cost: f64,
    /// Requests whose price was unknown; their cost is missing from `cost`.
    unknown: u64,
    requests: u64,
}

/// `{buckets: [{hour, model, input, output, cacheRead, cacheWrite, cost, unknown, requests}], sessions}`;
/// `hour` is unix seconds / 3600.
pub fn report() -> Value {
    let files = session::all_files();
    let mut buckets = BTreeMap::<(u64, String), Bucket>::new();
    let mut sessions = 0;
    for path in &files {
        if scan(path, &mut buckets) { sessions += 1; }
    }
    let rows: Vec<Value> = buckets.into_iter().map(|((hour, model), b)| json!({
        "hour": hour, "model": model,
        "input": b.usage.input, "output": b.usage.output, "cacheRead": b.usage.cache_read, "cacheWrite": b.usage.cache_write,
        "cost": b.cost, "unknown": b.unknown, "requests": b.requests,
    })).collect();
    json!({"buckets": rows, "sessions": sessions})
}

/// Adds one session file; true when it made at least one request.
fn scan(path: &Path, buckets: &mut BTreeMap<(u64, String), Bucket>) -> bool {
    let Ok(file) = File::open(path) else { return false };
    // Records written before request times existed fall back to the session's start.
    let mut fallback = file.metadata().and_then(|m| m.modified()).ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let mut seen = HashSet::new();
    let mut model = String::new();
    // Logs before the request ledger only have per-turn `usage` records.
    let mut ledger = false;
    let mut any = false;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(mut record) = serde_json::from_str::<Value>(&line) else { continue };
        match record["type"].as_str() {
            Some("meta") => {
                if let Some(created) = record["created"].as_u64() { fallback = created; }
                model = record["model"].as_str().unwrap_or("").to_owned();
            }
            Some("model") => model = record["model"].as_str().unwrap_or("").to_owned(),
            Some("request_stats_boundary") => ledger = true,
            Some("request_stats") => {
                let at = record["at"].as_u64().unwrap_or(fallback);
                let Ok(stats) = serde_json::from_value::<RequestStats>(record["stats"].take()) else { continue };
                // IPC can report the same child request twice; the ledger counts it once.
                if !seen.insert(stats.id.clone()) { continue; }
                let bucket = buckets.entry((at / 3600, base(&stats.model))).or_default();
                add(bucket, stats.usage.unwrap_or_default());
                match stats.cost.filter(|cost| cost.is_finite() && *cost >= 0.0) {
                    Some(cost) if stats.usage.is_some() => bucket.cost += cost,
                    _ => bucket.unknown += 1,
                }
                bucket.requests += 1;
                any = true;
            }
            Some("usage") if !ledger => {
                let get = |key: &str| record[key].as_u64().unwrap_or(0);
                let bucket = buckets.entry((fallback / 3600, base(&model))).or_default();
                add(bucket, Usage { input: get("input"), output: get("output"), cache_read: get("cache_read"), cache_write: get("cache_write") });
                bucket.unknown += 1;
                bucket.requests += 1;
                any = true;
            }
            _ => {}
        }
    }
    any
}

fn add(bucket: &mut Bucket, usage: Usage) {
    bucket.usage.input += usage.input;
    bucket.usage.output += usage.output;
    bucket.usage.cache_read += usage.cache_read;
    bucket.usage.cache_write += usage.cache_write;
}

/// `provider/model:high` and `provider/model` are one model in the report.
fn base(selector: &str) -> String {
    config::split_level(selector).0.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_requests_by_hour_and_model_once_each() {
        let dir = std::env::temp_dir().join(format!("ashira-usage-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let stats = |id: &str, model: &str, input: u64, cost: Option<f64>, at: Option<u64>| {
            let mut record = json!({"type": "request_stats", "stats": {"id": id, "model": model, "kind": "Chat", "usage": {"input": input, "output": 1, "cache_read": 0, "cache_write": 0}, "cost": cost, "duration_ms": 1, "child": false}});
            if let Some(at) = at { record["at"] = json!(at); }
            record.to_string()
        };
        let lines = [
            json!({"type": "meta", "cwd": "x", "model": "p/a", "created": 7200}).to_string(),
            json!({"type": "request_stats_boundary"}).to_string(),
            stats("1", "p/a:high", 10, Some(0.5), Some(3600 * 5 + 10)),
            stats("1", "p/a:high", 10, Some(0.5), Some(3600 * 5 + 10)), // repeated child report
            stats("2", "p/a", 20, None, Some(3600 * 5 + 20)),
            stats("3", "p/b", 30, Some(1.0), None), // older record without a time
            // After the ledger boundary a turn's `usage` record is not a request.
            json!({"type": "usage", "input": 999, "output": 0, "cache_read": 0, "cache_write": 0}).to_string(),
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        let mut buckets = BTreeMap::new();
        assert!(scan(&path, &mut buckets));
        let a = &buckets[&(5, "p/a".to_owned())];
        assert_eq!((a.usage.input, a.requests, a.cost, a.unknown), (30, 2, 0.5, 1));
        let b = &buckets[&(2, "p/b".to_owned())];
        assert_eq!((b.usage.input, b.requests, b.cost), (30, 1, 1.0));
        assert_eq!(buckets.len(), 2);

        // A pre-ledger log counts its per-turn usage with an unknown price.
        let old = dir.join("old.jsonl");
        std::fs::write(&old, [json!({"type": "meta", "cwd": "x", "model": "p/c:max", "created": 3600}).to_string(), json!({"type": "usage", "input": 4, "output": 2, "cache_read": 0, "cache_write": 0}).to_string()].join("\n")).unwrap();
        let mut buckets = BTreeMap::new();
        assert!(scan(&old, &mut buckets));
        let c = &buckets[&(1, "p/c".to_owned())];
        assert_eq!((c.usage.input, c.usage.output, c.unknown), (4, 2, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
