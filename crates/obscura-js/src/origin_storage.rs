//! Fork-only: BrowserContext-scoped localStorage.
//!
//! Upstream has no localStorage. Keeping the whole implementation here means
//! ops.rs carries only a field, a re-export and one registration line, so an
//! upstream rewrite of ops.rs does not conflict with this.

use std::collections::HashMap;

use deno_core::{op2, OpState};

use crate::ops::SharedState;

const LOCAL_STORAGE_ORIGIN_LIMIT: usize = 5 * 1024 * 1024;
const LOCAL_STORAGE_TOTAL_LIMIT: usize = 32 * 1024 * 1024;
const LOCAL_STORAGE_ORIGIN_COUNT_LIMIT: usize = 256;

#[derive(Default)]
struct OriginStorageInner {
    origins: HashMap<String, Vec<(String, String)>>,
    bytes: usize,
}

/// BrowserContext-scoped localStorage data. Each origin keeps insertion order,
/// while one bounded shared store lets pages and navigations in that context
/// observe the same values.
#[derive(Default)]
pub struct OriginStorage {
    inner: std::sync::Mutex<OriginStorageInner>,
}

impl OriginStorage {
    fn snapshot(&self, origin: &str) -> Vec<(String, String)> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .origins
            .get(origin)
            .cloned()
            .unwrap_or_default()
    }

    fn get(&self, origin: &str, key: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .origins
            .get(origin)
            .and_then(|items| items.iter().find(|(name, _)| name == key))
            .map(|(_, value)| value.clone())
    }

    fn set(&self, origin: &str, key: String, value: String) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !inner.origins.contains_key(origin)
            && inner.origins.len() >= LOCAL_STORAGE_ORIGIN_COUNT_LIMIT
        {
            return false;
        }

        let items = inner.origins.get(origin);
        let previous = items
            .and_then(|items| items.iter().find(|(name, _)| name == &key))
            .map(|(name, value)| name.len() + value.len())
            .unwrap_or(0);
        let origin_bytes = items
            .map(|items| {
                items
                    .iter()
                    .map(|(name, value)| name.len() + value.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        let new_bytes = key.len() + value.len();
        let next_origin_bytes = origin_bytes - previous + new_bytes;
        let next_total_bytes = inner.bytes - previous + new_bytes;
        if next_origin_bytes > LOCAL_STORAGE_ORIGIN_LIMIT
            || next_total_bytes > LOCAL_STORAGE_TOTAL_LIMIT
        {
            return false;
        }

        let items = inner.origins.entry(origin.to_string()).or_default();
        if let Some((_, old_value)) = items.iter_mut().find(|(name, _)| name == &key) {
            *old_value = value;
        } else {
            items.push((key, value));
        }
        inner.bytes = next_total_bytes;
        true
    }

    fn remove(&self, origin: &str, key: &str) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let removed = inner.origins.get_mut(origin).and_then(|items| {
            items
                .iter()
                .position(|(name, _)| name == key)
                .map(|index| items.remove(index))
        });
        if let Some((name, value)) = removed {
            inner.bytes -= name.len() + value.len();
        }
        if inner.origins.get(origin).is_some_and(Vec::is_empty) {
            inner.origins.remove(origin);
        }
    }

    fn clear(&self, origin: &str) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(items) = inner.origins.remove(origin) {
            inner.bytes -= items
                .iter()
                .map(|(name, value)| name.len() + value.len())
                .sum::<usize>();
        }
    }
}

fn local_storage_origin(raw_url: &str) -> String {
    url::Url::parse(raw_url)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|_| "null".to_string())
}

#[op2]
#[string]
pub(crate) fn op_local_storage(
    state: &OpState,
    #[string] command: &str,
    #[string] key: &str,
    #[string] value: &str,
) -> String {
    // Stage 4 (frame realms) replaces this with realm_state(scope, state) so a
    // framed document reads its own origin instead of the top document's.
    let gs = state.borrow::<SharedState>().clone();
    let (storage, origin) = {
        let gs = gs.borrow();
        (
            gs.local_storage.clone(),
            local_storage_origin(&gs.url),
        )
    };
    let Some(storage) = storage else {
        return "null".to_string();
    };

    match command {
        "snapshot" => serde_json::to_string(&storage.snapshot(&origin))
            .unwrap_or_else(|_| "[]".to_string()),
        "get" => serde_json::to_string(&storage.get(&origin, key))
            .unwrap_or_else(|_| "null".to_string()),
        "set" => storage.set(&origin, key.to_string(), value.to_string()).to_string(),
        "remove" => {
            storage.remove(&origin, key);
            "true".to_string()
        }
        "clear" => {
            storage.clear(&origin);
            "true".to_string()
        }
        _ => "null".to_string(),
    }
}
