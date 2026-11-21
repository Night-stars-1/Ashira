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

use crate::{api::Usage, config::{self, Registry}, session, stats::RequestStats};

#[derive(Default)]
struct Bucket {
    usage: Usage,
    cost: f64,
    /// Requests whose price was unknown; their cost is missing from `cost`.
    unknown: u64,
    requests: u64,
    /// Output tokens and generation time of requests that logged a duration, for TPS;
    /// legacy per-turn records have no time and stay out of both.
    timed_output: u64,
    duration_ms: u64,
}

/// `{buckets: [{hour, model, input, output, cacheRead, cacheWrite, cost, unknown, requests, timedOutput, durationMs}], sessions}`;
/// `hour` is unix seconds / 3600.
pub fn report(registry: &Registry) -> Value {
    let files = session::all_files();
    let mut buckets = BTreeMap::<(u64, String), Bucket>::new();
    let mut sessions = 0;
    for path in &files {
        if scan(path, registry, &mut buckets) { sessions += 1; }
    }
    let rows: Vec<Value> = buckets.into_iter().map(|((hour, model), b)| json!({
        "hour": hour, "model": model,
        "input": b.usage.input, "output": b.usage.output, "cacheRead": b.usage.cache_read, "cacheWrite": b.usage.cache_write,
        "cost": b.cost, "unknown": b.unknown, "requests": b.requests,
        "timedOutput": b.timed_output, "durationMs": b.duration_ms,
    })).collect();
    json!({"buckets": rows, "sessions": sessions})
}

/// Adds one session file; true when it made at least one request.
fn scan(path: &Path, registry: &Registry, buckets: &mut BTreeMap<(u64, String), Bucket>) -> bool {
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
                let Ok(mut stats) = serde_json::from_value::<RequestStats>(record["stats"].take()) else { continue };
                stats.price_missing(registry);
                // IPC can report the same child request twice; the ledger counts it once.
                if !seen.insert(stats.id.clone()) { continue; }
                let bucket = buckets.entry((at / 3600, base(&stats.model))).or_default();
                add(bucket, stats.usage.unwrap_or_default());
                match stats.cost.filter(|cost| cost.is_finite() && *cost >= 0.0) {
                    Some(cost) if stats.usage.is_some() => bucket.cost += cost,
                    _ => bucket.unknown += 1,
                }
                bucket.requests += 1;
                if let Some(usage) = stats.usage.filter(|usage| usage.output > 0 && stats.duration_ms > 0) {
                    bucket.timed_output += usage.output;
                    bucket.duration_ms += stats.duration_ms;
                }
                any = true;
            }
            Some("usage") if !ledger => {
                let get = |key: &str| record[key].as_u64().unwrap_or(0);
                let usage = Usage { input: get("input"), output: get("output"), cache_read: get("cache_read"), cache_write: get("cache_write") };
                let bucket = buckets.entry((fallback / 3600, base(&model))).or_default();
                add(bucket, usage);
                match registry.price_of(&model).and_then(|price| price.cost(usage)) {
                    Some(cost) => bucket.cost += cost,
                    None => bucket.unknown += 1,
                }
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
        assert!(scan(&path, &Registry::empty(), &mut buckets));
        let a = &buckets[&(5, "p/a".to_owned())];
        assert_eq!((a.usage.input, a.requests, a.cost, a.unknown), (30, 2, 0.5, 1));
        assert_eq!((a.timed_output, a.duration_ms), (2, 2));
        let b = &buckets[&(2, "p/b".to_owned())];
        assert_eq!((b.usage.input, b.requests, b.cost), (30, 1, 1.0));
        assert_eq!(buckets.len(), 2);

        // A pre-ledger log counts its per-turn usage with an unknown price.
        let old = dir.join("old.jsonl");
        std::fs::write(&old, [json!({"type": "meta", "cwd": "x", "model": "p/c:max", "created": 3600}).to_string(), json!({"type": "usage", "input": 4, "output": 2, "cache_read": 0, "cache_write": 0}).to_string()].join("\n")).unwrap();
        let mut buckets = BTreeMap::new();
        assert!(scan(&old, &Registry::empty(), &mut buckets));
        let c = &buckets[&(1, "p/c".to_owned())];
        assert_eq!((c.usage.input, c.usage.output, c.unknown), (4, 2, 1));
        // No request times before the ledger, so no TPS either.
        assert_eq!((c.timed_output, c.duration_ms), (0, 0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn requests_logged_without_a_price_use_todays_price() {
        let dir = std::env::temp_dir().join(format!("ashira-backfill-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        // Priced in models.yml now, with a context tier above 100 tokens.
        let registry = Registry::from_yaml("providers:\n  p:\n    api: openai-responses\n    baseUrl: https://x.example/v1\n    apiKey: k\n    modelOverrides:\n      a:\n        cost: {input: 1, output: 10, tiers: [{input: 2, output: 20, tier: {type: context, size: 100}}]}\n");
        let stats = |id: &str, input: u64, cost: Option<f64>| json!({"type": "request_stats", "at": 3600, "stats": {"id": id, "model": "p/a:high", "kind": "Chat", "usage": {"input": input, "output": 1_000_000, "cache_read": 0, "cache_write": 0}, "cost": cost, "duration_ms": 1, "child": false}}).to_string();
        let path = dir.join("s.jsonl");
        std::fs::write(&path, [
            json!({"type": "meta", "cwd": "x", "model": "p/a", "created": 3600}).to_string(),
            json!({"type": "request_stats_boundary"}).to_string(),
            stats("billed", 50, Some(7.0)), // kept as billed
            stats("short", 50, None),       // base tier: 50 × $1/M + 1M × $10/M
            stats("long", 200, None),       // above 100: 200 × $2/M + 1M × $20/M
        ].join("\n")).unwrap();
        let mut buckets = BTreeMap::new();
        assert!(scan(&path, &registry, &mut buckets));
        let a = &buckets[&(1, "p/a".to_owned())];
        assert_eq!(a.unknown, 0);
        assert!((a.cost - (7.0 + 10.00005 + 20.0004)).abs() < 1e-9, "{}", a.cost);
        let loaded = crate::session::load(&path, &registry).unwrap().ledger.snapshot();
        assert!(!loaded.main_unknown);
        assert!((loaded.main_cost - a.cost).abs() < 1e-9);

        // A log from before request records: each `usage` is one response of the session's model.
        let old = dir.join("old.jsonl");
        std::fs::write(&old, [json!({"type": "meta", "cwd": "x", "model": "p/a:max", "created": 3600}).to_string(), json!({"type": "usage", "input": 50, "output": 0, "cache_read": 0, "cache_write": 0}).to_string()].join("\n")).unwrap();
        let mut buckets = BTreeMap::new();
        assert!(scan(&old, &registry, &mut buckets));
        assert_eq!(buckets[&(1, "p/a".to_owned())].unknown, 0);
        assert!(!crate::session::load(&old, &registry).unwrap().ledger.snapshot().main_unknown);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
