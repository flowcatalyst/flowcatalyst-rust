//! A short-lived cache of the filter-options answers (Go
//! `dispatchjob/api/filter_options_cache.go`).
//!
//! The facets are DISTINCT reads over the `created_at` window. They are
//! bounded by the window (24 hours by default, 31 days at most), but they are
//! the most expensive reads the console issues and it issues them on every
//! page open, so the answer is kept for a minute.
//!
//! Keyed by the caller's tenant scope (a client user's values are those of
//! platform jobs and its own tenants, never another's) and by the window cut to
//! five-minute buckets, so two opens a few seconds apart, whose "last 24 hours"
//! differ by those seconds, share an entry. The window echoed with a cached
//! answer is the one the first request computed.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};

use super::api::DispatchJobFilterOptionsResponse;

/// How long a computed answer is reused.
pub const TTL: Duration = Duration::from_secs(60);
/// The granularity the window's ends are cut to for the key.
const BUCKET_SECS: i64 = 5 * 60;
/// Bounds the cache; past it the oldest entries go.
const MAX_ENTRIES: usize = 256;

#[derive(Default)]
pub struct FilterOptionsCache {
    entries: Mutex<HashMap<String, (Instant, DispatchJobFilterOptionsResponse)>>,
}

fn bucket(t: DateTime<Utc>) -> String {
    let secs = t.timestamp().div_euclid(BUCKET_SECS) * BUCKET_SECS;
    DateTime::<Utc>::from_timestamp(secs, 0)
        .unwrap_or(t)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The cache key: tenant scope (`None`: an anchor) and the bucketed window.
pub fn key(scope: Option<&[String]>, from: DateTime<Utc>, to: DateTime<Utc>) -> String {
    let who = match scope {
        None => "*".to_string(),
        Some(ids) => {
            let mut ids = ids.to_vec();
            ids.sort();
            format!("c:{}", ids.join(","))
        }
    };
    format!("{who}|{}|{}", bucket(from), bucket(to))
}

impl FilterOptionsCache {
    pub fn get(&self, key: &str, now: Instant) -> Option<DispatchJobFilterOptionsResponse> {
        let entries = self.entries.lock().ok()?;
        let (at, resp) = entries.get(key)?;
        (now.duration_since(*at) < TTL).then(|| resp.clone())
    }

    pub fn put(&self, key: String, now: Instant, resp: DispatchJobFilterOptionsResponse) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        // Drop what has expired so the map stays small.
        entries.retain(|_, (at, _)| now.duration_since(*at) < TTL);
        while entries.len() >= MAX_ENTRIES {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
        entries.insert(key, (now, resp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch_job::window::WindowResponse;
    use chrono::TimeZone;

    fn resp(tag: &str) -> DispatchJobFilterOptionsResponse {
        DispatchJobFilterOptionsResponse {
            statuses: vec![tag.into()],
            codes: vec![],
            event_types: vec![],
            client_ids: vec![],
            dispatch_pool_ids: vec![],
            subscription_ids: vec![],
            kinds: vec![],
            window: WindowResponse {
                from: String::new(),
                to: String::new(),
                defaulted: true,
            },
        }
    }

    #[test]
    fn the_key_is_per_scope_and_cut_to_five_minute_buckets() {
        let t = |m, s| Utc.with_ymd_and_hms(2026, 10, 19, 12, m, s).unwrap();
        assert_eq!(key(None, t(1, 5), t(2, 0)), key(None, t(3, 59), t(4, 1)));
        assert_ne!(key(None, t(1, 5), t(2, 0)), key(None, t(6, 0), t(7, 0)));
        let (a, b) = (vec!["clt_a".to_string()], vec!["clt_b".to_string()]);
        assert_ne!(
            key(Some(&a), t(1, 0), t(2, 0)),
            key(Some(&b), t(1, 0), t(2, 0))
        );
        assert_ne!(key(Some(&a), t(1, 0), t(2, 0)), key(None, t(1, 0), t(2, 0)));
        let ab = vec!["clt_b".to_string(), "clt_a".to_string()];
        let ba = vec!["clt_a".to_string(), "clt_b".to_string()];
        assert_eq!(
            key(Some(&ab), t(1, 0), t(2, 0)),
            key(Some(&ba), t(1, 0), t(2, 0))
        );
    }

    #[test]
    fn an_entry_lives_for_a_minute_and_the_cache_is_bounded() {
        let c = FilterOptionsCache::default();
        let t0 = Instant::now();
        c.put("k".into(), t0, resp("x"));
        assert!(c.get("k", t0 + Duration::from_secs(59)).is_some());
        assert!(c.get("k", t0 + Duration::from_secs(60)).is_none());
        for i in 0..(MAX_ENTRIES + 10) {
            c.put(format!("k{i}"), t0, resp("y"));
        }
        assert!(c.entries.lock().unwrap().len() <= MAX_ENTRIES);
    }
}
