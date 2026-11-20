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

use crate::{api::Usage, config, session};

const URL: &str = "https://models.dev/api.json";
const TTL_SECS: u64 = 24 * 60 * 60;
/// Bumped when `Info` changes meaning, so an older cached catalog is fetched again.
const VERSION: u32 = 3;

/// A single source's USD per million token price card.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Price {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// Context-length tiers, ascending by `above`; the base rates apply below the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<Tier>,
    /// The card prices tokens the usage cannot separate (reasoning at its own rate, unknown kinds).
    #[serde(default)]
    pub unsupported: bool,
}

/// Rates once the request's context (input + cache read + cache write) exceeds `above` tokens.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Tier {
    pub above: u64,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

/// (input, output, cache read, cache write) USD per million tokens.
pub type Rates = (Option<f64>, Option<f64>, Option<f64>, Option<f64>);

impl Price {
    /// A models.dev `cost` object or a models.yml `cost` card. Rates are rounded to
    /// micro-dollars so equal cards from different sources compare equal.
    pub fn parse(card: &Value) -> Option<Price> {
        let card = card.as_object()?;
        let rate = |obj: &serde_json::Map<String, Value>, snake: &str, camel: &str| {
            obj.get(snake).or_else(|| obj.get(camel)).and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0).map(|v| (v * 1e6).round() / 1e6)
        };
        let rates = |obj| (rate(obj, "input", "input"), rate(obj, "output", "output"), rate(obj, "cache_read", "cacheRead"), rate(obj, "cache_write", "cacheWrite"));
        let known = |key: &str| matches!(key, "input" | "output" | "cache_read" | "cacheRead" | "cache_write" | "cacheWrite");
        // Rates for tokens the usage never has or already bills correctly. Audio: Ashira
        // sends and receives none. Reasoning: providers report it inside output, so only
        // a rate equal to output's is billed right; a different one makes the card unusable.
        let harmless = |obj: &serde_json::Map<String, Value>, key: &str| match key {
            "input_audio" | "output_audio" | "inputAudio" | "outputAudio" => true,
            "reasoning" => rate(obj, "reasoning", "reasoning").is_some_and(|r| Some(r) == rate(obj, "output", "output")),
            _ => false,
        };
        let mut unsupported = false;
        let mut tiers = Vec::new();
        for (key, value) in card {
            match key.as_str() {
                key if known(key) || harmless(card, key) => {}
                // models.dev's older single tier; `tiers` carries it too.
                "context_over_200k" => {}
                "tiers" => for tier in value.as_array().into_iter().flatten() {
                    let parsed = tier.as_object().and_then(|obj| {
                        let limit = obj.get("tier")?;
                        if limit["type"] != "context" || obj.keys().any(|key| key != "tier" && !known(key) && !harmless(obj, key)) { return None; }
                        let (input, output, cache_read, cache_write) = rates(obj);
                        Some(Tier { above: limit["size"].as_u64()?, input, output, cache_read, cache_write })
                    });
                    match parsed {
                        Some(tier) => tiers.push(tier),
                        None => unsupported = true,
                    }
                },
                _ => unsupported = true,
            }
        }
        tiers.sort_by_key(|tier| tier.above);
        let (input, output, cache_read, cache_write) = rates(card);
        Some(Price { input, output, cache_read, cache_write, tiers, unsupported })
    }

    /// The rates for a request whose context is `context` tokens.
    pub fn rates(&self, context: u64) -> Rates {
        match self.tiers.iter().rev().find(|tier| context > tier.above) {
            Some(tier) => (tier.input, tier.output, tier.cache_read, tier.cache_write),
            None => (self.input, self.output, self.cache_read, self.cache_write),
        }
    }

    /// USD for one request, or `None` when a token kind it used has no rate. Tiered
    /// cards bill the whole request at the tier its context (input + cache) falls in.
    pub fn cost(&self, usage: Usage) -> Option<f64> {
        if self.unsupported && (usage.input != 0 || usage.output != 0 || usage.cache_read != 0 || usage.cache_write != 0) {
            return None;
        }
        let (input, output, cache_read, cache_write) = self.rates(usage.input + usage.cache_read + usage.cache_write);
        let component = |tokens: u64, rate: Option<f64>| -> Option<f64> {
            if tokens == 0 { return Some(0.0); }
            Some(tokens as f64 * rate.filter(|r| r.is_finite() && *r >= 0.0)?)
        };
        let cost = (component(usage.input, input)? + component(usage.output, output)?
            + component(usage.cache_read, cache_read)? + component(usage.cache_write, cache_write)?) / 1_000_000.0;
        cost.is_finite().then_some(cost)
    }

    /// Total order for voting between sources: equal cards share a key.
    fn key(&self) -> PriceKey {
        let micros = |v: Option<f64>| v.map(|v| (v * 1e6).round() as i64);
        let rates = |(i, o, r, w): Rates| [micros(i), micros(o), micros(r), micros(w)];
        let tiers = self.tiers.iter().map(|t| (t.above, rates((t.input, t.output, t.cache_read, t.cache_write)))).collect();
        (rates((self.input, self.output, self.cache_read, self.cache_write)), tiers, self.unsupported)
    }
}

type PriceKey = ([Option<i64>; 4], Vec<(u64, [Option<i64>; 4])>, bool);

/// `Price` ordered by its voting key.
struct Voted(Price);

impl PartialEq for Voted {
    fn eq(&self, other: &Self) -> bool { self.0.key() == other.0.key() }
}
impl Eq for Voted {}
impl PartialOrd for Voted {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for Voted {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.key().cmp(&other.0.key()) }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u32>,
    /// The voted price card; billing and model listings both read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    version: u32,
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
        let mut stored: Stored = fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // An older layout still answers lookups but counts as stale, so startup refetches it.
        if stored.version != VERSION { stored.fetched_at = 0; }
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
    let stored = Stored { version: VERSION, fetched_at: now, models: reduce(&raw) };
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
    cost: Option<Value>,
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
                // Vote on the entire card so cache and tier prices never come from another source.
                price: vote(&rows, |m| Price::parse(m.cost.as_ref()?).map(Voted)).map(|voted| voted.0),
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
    use super::{candidates, Price};

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

    #[test]
    fn context_tiers_pick_rates_by_request_context() {
        // models.dev's grok-4.7 card: one tier above 200k, plus the legacy duplicate key.
        let card = serde_json::json!({
            "input": 2, "output": 6, "cache_read": 0.5,
            "tiers": [{"input": 4, "output": 12, "cache_read": 1, "tier": {"type": "context", "size": 200000}}],
            "context_over_200k": {"input": 4, "output": 12, "cache_read": 1},
        });
        let price = Price::parse(&card).unwrap();
        assert!(!price.unsupported);
        assert_eq!(price.rates(200_000), (Some(2.0), Some(6.0), Some(0.5), None));
        assert_eq!(price.rates(200_001), (Some(4.0), Some(12.0), Some(1.0), None));

        // Several tiers given out of order still pick the highest one exceeded.
        let card = serde_json::json!({"input": 1, "output": 2, "tiers": [
            {"input": 3, "output": 6, "tier": {"type": "context", "size": 128000}},
            {"input": 2, "output": 4, "tier": {"type": "context", "size": 32000}},
        ]});
        let price = Price::parse(&card).unwrap();
        assert_eq!(price.rates(10_000).0, Some(1.0));
        assert_eq!(price.rates(50_000).0, Some(2.0));
        assert_eq!(price.rates(500_000).0, Some(3.0));

        // Audio rates never apply (no audio is sent), and a reasoning rate equal to output's
        // bills right; models.dev's google card for gemini-4-hydrogen has both shapes.
        let card = serde_json::json!({"input": 1.5, "output": 7.5, "cache_read": 0.075, "input_audio": 1.5, "reasoning": 7.5});
        let price = Price::parse(&card).unwrap();
        assert!(!price.unsupported);
        assert_eq!(price.rates(0), (Some(1.5), Some(7.5), Some(0.075), None));

        // Rates usage cannot be split into stay unsupported.
        for card in [serde_json::json!({"input": 1, "output": 2, "reasoning": 3}),
                     serde_json::json!({"input": 1, "output": 2, "tiers": [{"input": 2, "output": 2, "reasoning": 3, "tier": {"type": "context", "size": 1}}]}),
                     serde_json::json!({"input": 1, "output": 2, "tiers": [{"input": 2, "tier": {"type": "time", "size": 1}}]})] {
            assert!(Price::parse(&card).unwrap().unsupported);
        }
    }
}
