//! Durable Object storage for the audit log: loading chunks, reading a page,
//! and adding a chunk to a batch of writes. The rules live in
//! `ontime_core::audit`; this only moves bytes.

use serde_json::json;
use worker::*;

use ontime_core::audit::{chunk_key, AuditIndex, Chunk, Page, Retention, Writes};

/// Keys and values written together in one `put_multiple`, so a change and
/// its log line cannot disagree.
pub struct Batch {
    values: js_sys::Object,
    delete: Vec<String>,
}

impl Batch {
    pub fn new() -> Self {
        Self { values: js_sys::Object::new(), delete: Vec::new() }
    }

    /// Stores a value as a JSON string, the way every key here is stored.
    pub fn put<T: serde::Serialize>(&mut self, key: &str, value: &T) -> Result<()> {
        let raw = serde_json::to_string(value).map_err(|e| Error::RustError(format!("Could not serialise {key}: {e}")))?;
        self.put_raw(key, raw)
    }

    pub fn put_raw(&mut self, key: &str, raw: String) -> Result<()> {
        js_sys::Reflect::set(&self.values, &key.into(), &raw.into())?;
        Ok(())
    }

    /// The chunk the log wrote, and the chunks it dropped.
    pub fn audit(&mut self, writes: &Writes) -> Result<()> {
        self.put(&chunk_key(writes.put.no), &writes.put)?;
        self.delete.extend(writes.delete.iter().map(|no| chunk_key(*no)));
        Ok(())
    }

    pub async fn commit(self, storage: &mut Storage) -> Result<()> {
        if js_sys::Object::keys(&self.values).length() > 0 {
            storage.put_multiple_raw(self.values).await?;
        }
        if !self.delete.is_empty() {
            storage.delete_multiple(self.delete).await?;
        }
        Ok(())
    }
}

pub async fn load_chunk(storage: &Storage, no: u64) -> Option<Chunk> {
    storage
        .get::<String>(&chunk_key(no))
        .await
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

/// The newest chunk, reusing `cached` when it is still the head.
pub async fn head(storage: &Storage, index: &AuditIndex, cached: Option<Chunk>) -> Option<Chunk> {
    match (cached, index.head()) {
        (Some(chunk), Some(no)) if chunk.no == no => Some(chunk),
        (_, Some(no)) => load_chunk(storage, no).await,
        (_, None) => None,
    }
}

/// One page, newest first, from `?before=<ms>&limit=<n>`, as the JSON the
/// client reads. `cached` saves loading the head again.
pub async fn page(
    storage: &Storage,
    index: &AuditIndex,
    url: &Url,
    retention: Retention,
    now: i64,
    cached: Option<&Chunk>,
) -> Result<Response> {
    let query = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.to_string());
    let before = query("before").and_then(|v| v.parse::<i64>().ok());
    let limit = query("limit").and_then(|v| v.parse::<usize>().ok());

    let mut page = Page::new(before, limit, now, retention);
    for no in index.newest_first(before) {
        let chunk = match cached {
            Some(head) if head.no == no => Some(head.clone()),
            _ => load_chunk(storage, no).await,
        };
        if chunk.is_some_and(|chunk| page.take(&chunk)) {
            break;
        }
    }

    let mut headers = Headers::new();
    headers.set("content-type", "application/json")?;
    // names of the people who made changes
    headers.set("cache-control", "no-store")?;
    Ok(Response::from_json(&json!({ "lines": page.lines, "before": page.next_before() }))?.with_headers(headers))
}
