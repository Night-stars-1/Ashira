//! Model metadata from https://models.dev (context window, output limit,
//! price, display name, reasoning levels). The 6 MB `api.json` is reduced to
//! one record per model id and cached in `$ASHIRA_HOME/catalog.json`;
//! startup only reads that file, and the network fetch runs in the background.
//!
//! Values from models.yml always win; the catalog fills what the config and
//! the provider's `/models` listing leave unset.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    sync::{OnceLock, RwLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{config, session};

const URL: &str = "https://models.dev/api.json";
const TTL_SECS: u64 = 24 * 60 * 60;

/// A single source's USD per million token price card.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Price {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub unsupported: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u32>,
    /// USD per million tokens (input, output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<(f64, f64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    fetched_at: u64,
    /// Lowercased model id -> merged info.
    models: BTreeMap<String, Info>,
}

static CATALOG: OnceLock<RwLock<Stored>> = OnceLock::new();

fn path() -> PathBuf {
    session::home().join("catalog.json")
}

fn store() -> &'static RwLock<Stored> {
    CATALOG.get_or_init(|| {
        let stored = fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        RwLock::new(stored)
    })
}

/// Metadata for `id`, trying the id as written and then with gateway
/// decorations removed (`[1m]`, `-high`, date stamps, `vendor/` prefixes).
pub fn lookup(id: &str) -> Option<Info> {
    let s = store().read().expect("catalog lock");
    if s.models.is_empty() {
        return None;
    }
    candidates(id).into_iter().find_map(|c| s.models.get(&c).cloned())
}

/// (model count, seconds since fetch) for listings; `None` before the first fetch.
pub fn status() -> Option<(usize, u64)> {
    let s = store().read().expect("catalog lock");
    (!s.models.is_empty()).then(|| (s.models.len(), config::now().saturating_sub(s.fetched_at)))
}

/// Downloads the catalog when the cached copy is missing or older than a day
/// (or always, with `force`). `Ok(true)` when new data was installed.
pub async fn refresh(force: bool) -> Result<bool, String> {
    let now = config::now();
    if !force && now.saturating_sub(store().read().expect("catalog lock").fetched_at) < TTL_SECS {
        return Ok(false);
    }
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    let resp = http.get(URL).send().await.map_err(|e| format!("models.dev: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("models.dev: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("models.dev: {e}"))?;
    let raw: HashMap<String, RawProvider> = serde_json::from_slice(&bytes).map_err(|e| format!("models.dev: bad JSON: {e}"))?;
    let stored = Stored { fetched_at: now, models: reduce(&raw) };
    if let Ok(b) = serde_json::to_vec(&stored) {
        let _ = fs::write(path(), b);
    }
    *store().write().expect("catalog lock") = stored;
    Ok(true)
}

#[derive(Deserialize)]
struct RawProvider {
    #[serde(default)]
    models: HashMap<String, RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    limit: Option<RawLimit>,
    #[serde(default)]
    cost: Option<RawCost>,
    #[serde(default)]
    reasoning_options: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct RawLimit {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

#[derive(Deserialize)]
struct RawCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

/// Providers that publish the model themselves; their numbers win over resellers'.
const FIRST_PARTY: &[&str] = &["openai", "anthropic", "google", "deepseek", "moonshotai", "xai", "mistral", "zai", "alibaba", "minimax", "meta"];

/// One record per lowercased id: first-party values when available, else the
/// value most providers agree on (ties broken by the larger value).
fn reduce(raw: &HashMap<String, RawProvider>) -> BTreeMap<String, Info> {
    let mut by_id: HashMap<String, Vec<(bool, &RawModel)>> = HashMap::new();
    for (pid, p) in raw {
        let first = FIRST_PARTY.contains(&pid.as_str());
        for (id, m) in &p.models {
            // Router ids like `anthropic/claude-x` are also indexed by the bare id.
            let full = id.to_lowercase();
            let bare = full.rsplit('/').next().unwrap_or(&full).to_owned();
            if bare != full {
                by_id.entry(bare).or_default().push((first, m));
            }
            by_id.entry(full).or_default().push((first, m));
        }
    }
    by_id
        .into_iter()
        .map(|(id, rows)| {
            let info = Info {
                name: vote(&rows, |m| m.name.clone()),
                context: vote(&rows, |m| m.limit.as_ref()?.context.filter(|n| *n > 0)),
                output: vote(&rows, |m| m.limit.as_ref()?.output.filter(|n| *n > 0).and_then(|n| u32::try_from(n).ok())),
                // Vote on the entire card so cache prices never come from another source.
                cost: vote(&rows, |m| {
                    let c = m.cost.as_ref()?;
                    Some(((c.input? * 1e6).round() as i64, (c.output? * 1e6).round() as i64))
                }).map(|(i, o)| (i as f64 / 1e6, o as f64 / 1e6)),
                price: vote(&rows, |m| {
                    let c = m.cost.as_ref()?;
                    let n = |v: Option<f64>| v.filter(|v| v.is_finite() && *v >= 0.0).map(|v| (v * 1e6).round() as i64);
                    Some((n(c.input), n(c.output), n(c.cache_read), n(c.cache_write), !c.extra.is_empty()))
                }).map(|(i, o, r, w, unsupported)| Price {
                    input: i.map(|v| v as f64 / 1e6), output: o.map(|v| v as f64 / 1e6),
                    cache_read: r.map(|v| v as f64 / 1e6), cache_write: w.map(|v| v as f64 / 1e6), unsupported,
                }),
                efforts: vote(&rows, |m| {
                    let effort = m.reasoning_options.as_ref()?.iter().find(|o| o["type"] == "effort")?;
                    Some(effort["values"].as_array()?.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>())
                })
                .unwrap_or_default(),
            };
            (id, info)
        })
        .collect()
}

fn vote<T: Ord>(rows: &[(bool, &RawModel)], f: impl Fn(&RawModel) -> Option<T>) -> Option<T> {
    if let Some(v) = rows.iter().filter(|(first, _)| *first).find_map(|(_, m)| f(m)) {
        return Some(v);
    }
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for (_, m) in rows {
        if let Some(v) = f(m) {
            *counts.entry(v).or_default() += 1;
        }
    }
    // BTreeMap iterates ascending and `max_by_key` keeps the last max, so ties pick the larger value.
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v)
}

/// Ids to try, most specific first. Gateways decorate ids with context tags
/// (`claude-opus-5[1m]`), effort tiers (`gemini-3.8-flash-high`), date stamps
/// (`gpt-5.2-2025-12-11`, `claude-haiku-4-5-20251001`) and vendor prefixes.
fn candidates(id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().to_lowercase();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    let bare = id.rsplit('/').next().unwrap_or(id);
    let no_tag = bare.split('[').next().unwrap_or(bare);
    push(id);
    push(bare);
    push(no_tag);
    let mut cur = no_tag.to_owned();
    while let Some((head, tail)) = cur.rsplit_once('-') {
        let suffix = matches!(
            tail,
            "high" | "low" | "medium" | "minimal" | "max" | "xhigh" | "thinking" | "tiered" | "latest" | "preview" | "fast" | "plus" | "uncensored" | "bypass" | "free"
        );
        // Dates: `20251001`, or the `2025` / `12` / `11` parts of `2025-12-11`.
        let date =
            tail.chars().all(|c| c.is_ascii_digit()) && (tail.len() >= 4 || (tail.len() == 2 && head.rsplit('-').next().is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))));
        if !suffix && !date {
            break;
        }
        cur = head.to_owned();
        push(&cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::candidates;

    #[test]
    fn gateway_decorations_are_stripped() {
        assert!(candidates("claude-opus-5-5[1m]").contains(&"claude-opus-5-5".to_owned()));
        assert!(candidates("gemini-3.8-flash-high").contains(&"gemini-3.8-flash".to_owned()));
        assert!(candidates("gpt-5.2-2025-12-11").contains(&"gpt-5.2".to_owned()));
        assert!(candidates("claude-haiku-4-5-20251001").contains(&"claude-haiku-4-5".to_owned()));
        assert!(candidates("z-ai/GLM-5.3").contains(&"glm-5.3".to_owned()));
        // Version components like `4-8` are not dates and must survive.
        assert_eq!(candidates("claude-opus-4-8").last().map(String::as_str), Some("claude-opus-4-8"));
    }
}
