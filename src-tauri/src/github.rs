//! Read-only GitHub access: the GraphQL API for repo discovery and conditional GETs (ETags)
//! for everything fetched repeatedly. Mutations go through `gh student`, never through here.
//! The token comes from `gh auth token` per sync and is never stored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use reqwest::header::{self, HeaderMap};
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};

/// The shared HTTP client (connection pool, User-Agent), managed as Tauri state.
pub struct Http(pub reqwest::Client);

const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const CACHE_FILE: &str = "http-cache.json";

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    message: String,
}

/// Runs a GraphQL query. Errors on individual fields (a repo that disappeared mid-sync)
/// come back as `null` in `data` and are only logged; the request fails when there's no
/// data at all.
pub async fn graphql<T: DeserializeOwned>(
    http: &reqwest::Client,
    token: &str,
    query: &str,
    variables: Value,
) -> Result<T> {
    crate::simulate::check()?;
    let request = http
        .post(crate::testenv::github_url(GRAPHQL_URL).as_ref())
        .bearer_auth(token)
        .json(&json!({ "query": query, "variables": variables }));
    let response = request.send().await.map_err(Error::network)?;
    record_rate_limit(response.headers());
    let response = checked(response)?;
    let body: GraphQlResponse<T> = response.json().await?;
    if body
        .errors
        .iter()
        .any(|e| e.kind.as_deref() == Some("RATE_LIMITED"))
    {
        return Err(Error::RateLimited { reset_at: None });
    }
    for err in &body.errors {
        log::warn!("GitHub GraphQL: {}", err.message);
    }
    body.data.ok_or_else(|| {
        Error::GitHub(
            body.errors
                .into_iter()
                .next()
                .map_or_else(|| "empty response".to_owned(), |e| e.message),
        )
    })
}

/// Maps auth and rate-limit statuses to typed errors; other failures stay HTTP errors.
/// The GraphQL budget as of the last response, for the sync details in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub remaining: u32,
    pub limit: u32,
    pub reset_at: Option<DateTime<Utc>>,
}

static RATE_LIMIT: std::sync::Mutex<Option<RateLimit>> = std::sync::Mutex::new(None);

pub fn rate_limit() -> Option<RateLimit> {
    RATE_LIMIT.lock().ok().and_then(|r| *r)
}

fn record_rate_limit(headers: &HeaderMap) {
    let number = |name| header_str(headers, name).and_then(|s| s.parse::<u32>().ok());
    if let (Some(remaining), Some(limit)) =
        (number("x-ratelimit-remaining"), number("x-ratelimit-limit"))
        && let Ok(mut current) = RATE_LIMIT.lock()
    {
        *current = Some(RateLimit {
            remaining,
            limit,
            reset_at: header_str(headers, "x-ratelimit-reset")
                .and_then(|s| s.parse::<i64>().ok())
                .and_then(|epoch| DateTime::from_timestamp(epoch, 0)),
        });
    }
}

fn checked(response: Response) -> Result<Response> {
    match response.status() {
        StatusCode::UNAUTHORIZED => Err(Error::NotSignedIn),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
            if is_rate_limited(response.headers()) =>
        {
            Err(Error::RateLimited {
                reset_at: rate_limit_reset(response.headers()),
            })
        }
        _ => Ok(response.error_for_status()?),
    }
}

fn is_rate_limited(headers: &HeaderMap) -> bool {
    headers.contains_key(header::RETRY_AFTER)
        || header_str(headers, "x-ratelimit-remaining") == Some("0")
}

fn rate_limit_reset(headers: &HeaderMap) -> Option<DateTime<Utc>> {
    if let Some(seconds) =
        header_str(headers, header::RETRY_AFTER.as_str()).and_then(|s| s.parse::<i64>().ok())
    {
        return Some(Utc::now() + chrono::Duration::seconds(seconds));
    }
    header_str(headers, "x-ratelimit-reset")
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|epoch| DateTime::from_timestamp(epoch, 0))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

#[derive(Clone, Serialize, Deserialize)]
struct Cached {
    etag: String,
    body: String,
    /// When the server last confirmed this body.
    #[serde(default)]
    checked_at: Option<DateTime<Utc>>,
}

/// Bodies of earlier GET responses keyed by URL, replayed when the server answers
/// `304 Not Modified`. Conditional requests don't count against GitHub's rate limit.
pub struct HttpCache {
    path: PathBuf,
    entries: Mutex<HashMap<String, Cached>>,
}

impl HttpCache {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(CACHE_FILE);
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    /// GETs `url` as text, revalidating a cached copy with `If-None-Match`.
    /// A body confirmed less than `max_age` ago is returned without any request.
    pub async fn get_text(
        &self,
        http: &reqwest::Client,
        url: &str,
        max_age: chrono::TimeDelta,
    ) -> Result<String> {
        self.get(http, url, max_age, None).await
    }

    /// `get_text` for a GitHub REST URL, sent with the token: authenticated requests get
    /// the 5000/hour budget instead of 60 per IP.
    pub async fn get_api_text_or_cached(
        &self,
        http: &reqwest::Client,
        token: &str,
        url: &str,
        max_age: chrono::TimeDelta,
    ) -> Result<String> {
        match self.get(http, url, max_age, Some(token)).await {
            Err(err) => self.cached_text(url).ok_or(err),
            fetched => fetched,
        }
    }

    async fn get(
        &self,
        http: &reqwest::Client,
        url: &str,
        max_age: chrono::TimeDelta,
        token: Option<&str>,
    ) -> Result<String> {
        let now = Utc::now();
        let cached = self.entries.lock().ok().and_then(|e| e.get(url).cloned());
        if let Some(entry) = &cached
            && entry.checked_at.is_some_and(|at| now - at < max_age)
        {
            return Ok(entry.body.clone());
        }
        crate::simulate::check()?;
        let target = crate::testenv::github_url(url);
        let request = match token {
            Some(token) => http.get(target.as_ref()).bearer_auth(token),
            None => http.get(target.as_ref()),
        };
        let request = match &cached {
            Some(entry) => request.header(header::IF_NONE_MATCH, &entry.etag),
            None => request,
        };
        let response = request.send().await.map_err(Error::network)?;
        if response.status() == StatusCode::NOT_MODIFIED
            && let Some(entry) = cached
        {
            if let Ok(mut entries) = self.entries.lock()
                && let Some(stored) = entries.get_mut(url)
            {
                stored.checked_at = Some(now);
            }
            return Ok(entry.body);
        }
        let response = checked(response)?;
        let etag = header_str(response.headers(), header::ETAG.as_str()).map(str::to_owned);
        let body = response.text().await?;
        if let (Some(etag), Ok(mut entries)) = (etag, self.entries.lock()) {
            entries.insert(
                url.to_owned(),
                Cached {
                    etag,
                    body: body.clone(),
                    checked_at: Some(now),
                },
            );
        }
        Ok(body)
    }

    /// `get_text`, or the last body downloaded from `url` when the request fails (GitHub
    /// unreachable, a server error).
    pub async fn get_text_or_cached(
        &self,
        http: &reqwest::Client,
        url: &str,
        max_age: chrono::TimeDelta,
    ) -> Result<String> {
        match self.get_text(http, url, max_age).await {
            Err(err) => self.cached_text(url).ok_or(err),
            fetched => fetched,
        }
    }

    /// The last body downloaded from `url`, without any request.
    pub fn cached_text(&self, url: &str) -> Option<String> {
        let entries = self.entries.lock().ok()?;
        entries.get(url).map(|e| e.body.clone())
    }

    /// Forgets every cached body, so the next GETs download in full.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
        let _ = std::fs::remove_file(&self.path);
    }

    /// Writes the cache to disk (temp file + rename). Failures only cost a cold cache.
    pub fn save(&self) {
        let Ok(json) = self
            .entries
            .lock()
            .map_err(|_| ())
            .and_then(|e| serde_json::to_vec(&*e).map_err(|_| ()))
        else {
            return;
        };
        if let Err(err) = write_atomic(&self.path, &json) {
            log::warn!("could not save {}: {err}", self.path.display());
        }
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    /// Nothing listens on port 9 (discard), so the request fails at once.
    #[tokio::test]
    async fn a_failed_request_falls_back_to_the_last_body() {
        let dir = std::env::temp_dir().join(format!("c50-http-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = HttpCache::load(&dir);
        let known = "http://127.0.0.1:9/known.json";
        cache.entries.lock().unwrap().insert(
            known.to_owned(),
            Cached {
                etag: "\"1\"".into(),
                body: "last".into(),
                checked_at: None,
            },
        );
        let http = reqwest::Client::new();
        let max_age = chrono::TimeDelta::minutes(10);
        assert_eq!(
            cache
                .get_text_or_cached(&http, known, max_age)
                .await
                .unwrap(),
            "last"
        );
        let unknown = "http://127.0.0.1:9/unknown.json";
        assert!(
            cache
                .get_text_or_cached(&http, unknown, max_age)
                .await
                .is_err()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_rate_limits() {
        let mut headers = HeaderMap::new();
        assert!(!is_rate_limited(&headers));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("1792000000"));
        assert!(is_rate_limited(&headers));
        assert_eq!(
            rate_limit_reset(&headers).map(|t| t.timestamp()),
            Some(1_792_000_000)
        );

        let mut secondary = HeaderMap::new();
        secondary.insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        assert!(is_rate_limited(&secondary));
        let reset = rate_limit_reset(&secondary).unwrap();
        assert!(reset > Utc::now() + chrono::Duration::seconds(50));
    }
}
