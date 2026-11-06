//! The GitHub accounts gh is signed in to, switching between them, and their avatars.
//! Avatars come from GitHub's avatar CDN (not the API) and are cached in the app cache dir
//! with their ETag, so a picture that hasn't changed is never downloaded again.

use std::path::Path;
use std::time::{Duration, SystemTime};

use base64::Engine;
use reqwest::StatusCode;
use reqwest::header::{self, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::process;

const HOST: &str = "github.com";
/// How long a cached picture is shown without asking GitHub; after that a conditional
/// request tells whether it changed.
const AVATAR_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: String,
    pub active: bool,
}

#[derive(Deserialize)]
struct StatusJson {
    hosts: std::collections::HashMap<String, Vec<HostAccount>>,
}

#[derive(Deserialize)]
struct HostAccount {
    login: String,
    active: bool,
}

/// Every github.com account gh holds a token for, the active one first.
pub async fn list() -> Result<Vec<Account>> {
    let out = process::run("gh", ["auth", "status", "--json", "hosts"], None).await?;
    parse_status(&out.stdout)
}

fn parse_status(json: &str) -> Result<Vec<Account>> {
    let status: StatusJson = serde_json::from_str(json)
        .map_err(|err| Error::GitHub(format!("gh auth status: {err}")))?;
    let mut accounts: Vec<Account> = status
        .hosts
        .get(HOST)
        .into_iter()
        .flatten()
        .map(|a| Account {
            login: a.login.clone(),
            active: a.active,
        })
        .collect();
    accounts.sort_by_key(|a| !a.active);
    Ok(accounts)
}

/// Makes `login` gh's active account (for the terminal too: it is gh's own setting).
pub async fn switch(login: &str) -> Result<()> {
    let login = valid_login(login)?;
    process::run(
        "gh",
        ["auth", "switch", "--hostname", HOST, "--user", login],
        None,
    )
    .await?;
    Ok(())
}

/// Removes `login` from gh (its token leaves gh's store; GitHub keeps the authorization,
/// which the user can revoke in their settings). gh makes another account active if it
/// holds one. Like switching, the terminal follows.
pub async fn sign_out(login: &str) -> Result<()> {
    let login = valid_login(login)?;
    if let Some(var) = crate::tools::env_token_var() {
        return Err(Error::TokenFromEnv(var.into()));
    }
    process::run(
        "gh",
        ["auth", "logout", "--hostname", HOST, "--user", login],
        None,
    )
    .await?;
    Ok(())
}

/// The avatar as a `data:` URL, from the cache when it is less than a day old.
pub async fn avatar(http: &reqwest::Client, cache_dir: &Path, login: &str) -> Result<String> {
    let login = valid_login(login)?;
    let url = format!("https://github.com/{login}.png?size=64");
    let url = crate::testenv::github_url(&url);
    let path = cache_dir.join("avatars").join(format!("{login}.png"));
    let bytes = load(http, url.as_ref(), &path).await?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}

/// The picture at `path` while it is fresh, else what GitHub answers to its ETag (kept
/// next to it as `<login>.etag`): a 304 keeps the cached one another `AVATAR_MAX_AGE`.
async fn load(http: &reqwest::Client, url: &str, path: &Path) -> Result<Vec<u8>> {
    let fresh = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|at| SystemTime::now().duration_since(at).unwrap_or_default() < AVATAR_MAX_AGE);
    match std::fs::read(path).ok() {
        Some(bytes) if fresh => Ok(bytes),
        cached => match download(http, url, path, cached.as_deref()).await {
            Ok(bytes) => Ok(bytes),
            // Offline: an old picture beats none.
            Err(err) => cached.ok_or(err),
        },
    }
}

async fn download(
    http: &reqwest::Client,
    url: &str,
    path: &Path,
    cached: Option<&[u8]>,
) -> Result<Vec<u8>> {
    crate::simulate::check()?;
    let etag_path = path.with_extension("etag");
    // Only a picture still on disk is worth revalidating.
    let etag = cached
        .and_then(|_| std::fs::read_to_string(&etag_path).ok())
        .and_then(|etag| HeaderValue::from_str(etag.trim()).ok());
    let mut request = http.get(url);
    if let Some(etag) = etag {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await?.error_for_status()?;
    if response.status() == StatusCode::NOT_MODIFIED {
        let cached = cached.ok_or_else(|| Error::GitHub(format!("unexpected 304 for {url}")))?;
        let touched = std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(SystemTime::now()));
        if let Err(err) = touched {
            log::warn!("could not refresh {}: {err}", path.display());
        }
        return Ok(cached.to_vec());
    }
    let etag = response.headers().get(header::ETAG).cloned();
    let bytes = response.bytes().await?.to_vec();
    crate::github::write_atomic(path, &bytes)?;
    match etag {
        Some(etag) => crate::github::write_atomic(&etag_path, etag.as_bytes())?,
        None => {
            let _ = std::fs::remove_file(&etag_path);
        }
    }
    Ok(bytes)
}

/// GitHub logins are letters, digits and single hyphens; anything else never reaches a
/// path, a URL or gh.
fn valid_login(login: &str) -> Result<&str> {
    let ok = !login.is_empty()
        && login.len() <= 39
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !login.starts_with('-');
    if ok {
        Ok(login)
    } else {
        Err(Error::GitHub(format!("not a GitHub login: {login:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_accounts_active_first() {
        let json = r#"{"hosts":{"github.com":[
            {"state":"success","active":false,"host":"github.com","login":"alt"},
            {"state":"success","active":true,"host":"github.com","login":"main"}
        ],"ghe.example.com":[{"active":true,"login":"work"}]}}"#;
        assert_eq!(
            parse_status(json).unwrap(),
            vec![
                Account {
                    login: "main".into(),
                    active: true
                },
                Account {
                    login: "alt".into(),
                    active: false
                },
            ]
        );
    }

    #[test]
    fn rejects_odd_logins() {
        assert!(valid_login("Romain-Antunes1").is_ok());
        assert!(valid_login("../etc").is_err());
        assert!(valid_login("-x").is_err());
        assert!(valid_login("").is_err());
    }

    /// Answers one request with `response` and hands back the request it read.
    fn serve(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/ada.png", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap().to_lowercase()
        });
        (url, server)
    }

    fn age(path: &Path, by: Duration) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - by).unwrap();
    }

    #[tokio::test]
    async fn an_avatar_is_downloaded_again_only_when_its_etag_changed() {
        let dir = std::env::temp_dir().join(format!("c50-avatar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("avatars").join("ada.png");
        let etag = dir.join("avatars").join("ada.etag");
        let http = reqwest::Client::new();
        let stale = AVATAR_MAX_AGE + Duration::from_secs(60);

        let (url, server) = serve(
            "HTTP/1.1 200 OK\r\nETag: \"one\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\nold",
        );
        assert_eq!(load(&http, &url, &path).await.unwrap(), b"old");
        assert!(!server.join().unwrap().contains("if-none-match"));
        assert_eq!(std::fs::read_to_string(&etag).unwrap(), "\"one\"");

        // Fresh: GitHub isn't asked (nothing listens on port 9).
        let offline = "http://127.0.0.1:9/ada.png";
        assert_eq!(load(&http, offline, &path).await.unwrap(), b"old");

        // Stale and unchanged: a 304 keeps the picture and makes it fresh again.
        age(&path, stale);
        let (url, server) = serve("HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n");
        assert_eq!(load(&http, &url, &path).await.unwrap(), b"old");
        assert!(server.join().unwrap().contains("if-none-match: \"one\""));
        assert_eq!(load(&http, offline, &path).await.unwrap(), b"old");

        // Stale and changed: the new picture and its ETag replace the old ones.
        age(&path, stale);
        let (url, server) = serve(
            "HTTP/1.1 200 OK\r\nETag: \"two\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\nnew",
        );
        assert_eq!(load(&http, &url, &path).await.unwrap(), b"new");
        assert!(server.join().unwrap().contains("if-none-match: \"one\""));
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_to_string(&etag).unwrap(), "\"two\"");

        // Stale and offline: the old picture beats none.
        age(&path, stale);
        assert_eq!(load(&http, offline, &path).await.unwrap(), b"new");
        // With nothing cached, offline is an error.
        std::fs::remove_file(&path).unwrap();
        assert!(load(&http, offline, &path).await.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_suites_fake_gh_lists_one_account() {
        let json = crate::testenv::fake_gh(&["auth", "status", "--json", "hosts"]);
        assert_eq!(
            parse_status(&json).unwrap(),
            vec![Account {
                login: "ada-e2e".into(),
                active: true
            }]
        );
    }
}
