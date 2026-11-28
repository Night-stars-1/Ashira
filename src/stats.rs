//! Request-level accounting, independent of the current context usage snapshot.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::api::Usage;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Kind {
    Chat,
    Compact,
    Memory,
    /// The background request that names a session.
    Title,
    /// A `generate_image` call to the image model.
    Image,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RequestStats {
    pub id: String,
    pub model: String,
    pub kind: Kind,
    pub usage: Option<Usage>,
    pub cost: Option<f64>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_output_ms: Option<u64>,
    pub child: bool,
}

pub(crate) fn output_rate(usage: Usage, duration_ms: u64) -> Option<f64> {
    (usage.output > 0).then(|| usage.output as f64 * 1000.0 / duration_ms.max(100) as f64)
}

impl RequestStats {
    /// Costs a request logged without a price (the model was unpriced then) at today's
    /// price; a logged cost is kept as it was billed.
    pub fn price_missing(&mut self, registry: &crate::config::Registry) {
        if self.cost.is_none() && let Some(usage) = self.usage {
            self.cost = registry.price_of(&self.model).and_then(|price| price.cost(usage));
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub usage: Usage,
    pub main_cost: f64,
    pub child_cost: f64,
    pub main_unknown: bool,
    pub child_unknown: bool,
    pub main_requests: u64,
    pub child_requests: u64,
    pub last_rate: Option<f64>,
}

#[derive(Default)]
pub struct Ledger {
    ids: HashSet<String>,
    snapshot: Snapshot,
}

impl Ledger {
    pub fn record(&mut self, request: &RequestStats) -> bool {
        if !self.ids.insert(request.id.clone()) {
            return false;
        }
        if let Some(usage) = request.usage {
            self.add_usage(usage);
        }
        let snapshot = &mut self.snapshot;
        let (cost, unknown, requests) = if request.child {
            (&mut snapshot.child_cost, &mut snapshot.child_unknown, &mut snapshot.child_requests)
        } else {
            (&mut snapshot.main_cost, &mut snapshot.main_unknown, &mut snapshot.main_requests)
        };
        *requests += 1;
        match request.cost.filter(|cost| cost.is_finite() && *cost >= 0.0) {
            Some(value) if request.usage.is_some() => *cost += value,
            _ => *unknown = true,
        }
        if !request.child && matches!(request.kind, Kind::Chat) {
            snapshot.last_rate = request.usage.and_then(|usage| output_rate(usage, request.duration_ms));
        }
        true
    }

    /// A turn's usage from a log written before request records; `cost` is today's
    /// price for the session's model at the time, when there is one.
    pub fn legacy(&mut self, usage: Usage, cost: Option<f64>) {
        self.add_usage(usage);
        match cost {
            Some(cost) => self.snapshot.main_cost += cost,
            None => self.snapshot.main_unknown = true,
        }
        self.snapshot.main_requests += 1;
    }

    pub fn snapshot(&self) -> Snapshot { self.snapshot }

    fn add_usage(&mut self, usage: Usage) {
        self.snapshot.usage.input += usage.input;
        self.snapshot.usage.output += usage.output;
        self.snapshot.usage.cache_read += usage.cache_read;
        self.snapshot.usage.cache_write += usage.cache_write;
    }
}
