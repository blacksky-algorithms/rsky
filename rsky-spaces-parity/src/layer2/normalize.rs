//! Response normalization, using the same first-appearance substitution the
//! stored-row comparator uses: each side's distinct revisions map in order to
//! `R1, R2, …`. Fields that cannot match between two independent writers are
//! replaced by a marker rather than dropped, so a field appearing on one side
//! only still shows up as a difference.

use crate::is_tid;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Commit fields derived from fresh random key material on every serve, plus
/// the opaque paging cursor, whose numbering is per-store.
pub const NON_COMPARABLE: [&str; 4] = ["ikm", "sig", "mac", "cursor"];

#[derive(Default)]
pub struct Revs {
    seen: BTreeMap<String, String>,
    order: Vec<String>,
}

impl Revs {
    pub fn placeholder(&mut self, rev: &str) -> String {
        if let Some(existing) = self.seen.get(rev) {
            return existing.clone();
        }
        let placeholder = format!("R{}", self.order.len() + 1);
        self.seen.insert(rev.to_string(), placeholder.clone());
        self.order.push(rev.to_string());
        placeholder
    }

    pub fn count(&self) -> usize {
        self.order.len()
    }

    /// The revisions in first-appearance order, unsubstituted.
    pub fn raw(&self) -> &[String] {
        &self.order
    }
}

/// Object keys are emitted in sorted order. The two decoders build their JSON
/// maps in different orders from the same DAG-CBOR bytes, and record identity is
/// the CID, which is compared unnormalized in the same response.
pub fn normalize(value: &Value, revs: &mut Revs) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| normalize(v, revs)).collect()),
        Value::Object(fields) => {
            let mut sorted: Vec<(&String, &Value)> = fields.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = Map::new();
            for (key, field) in sorted {
                let replacement = if NON_COMPARABLE.contains(&key.as_str()) {
                    Value::String("<non-comparable>".to_string())
                } else if key == "rev" {
                    // Only a revision is substituted. Record keys are TIDs too,
                    // and they must compare literally.
                    match field.as_str() {
                        Some(rev) => Value::String(revs.placeholder(rev)),
                        None => normalize(field, revs),
                    }
                } else {
                    normalize(field, revs)
                };
                out.insert(key.clone(), replacement);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Every revision a response exposed is a syntactically valid TID.
pub fn revs_are_tids(revs: &Revs) -> bool {
    revs.raw().iter().all(|rev| is_tid(rev))
}

pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
