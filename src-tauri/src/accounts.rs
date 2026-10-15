//! The GitHub accounts gh is signed in to, switching between them, and their avatars.
//! Avatars come from GitHub's avatar CDN (not the API), once a week per account, and are
//! cached in the app cache dir.

use std::path::Path;
use std::time::{Duration, SystemTime};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::process;

const HOST: &str = "github.com";
const AVATAR_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// The avatar as a `data:` URL, from the cache when it is less than a week old.
pub async fn avatar(http: &reqwest::Client, cache_dir: &Path, login: &str) -> Result<String> {
    let login = valid_login(login)?;
    let path = cache_dir.join("avatars").join(format!("{login}.png"));
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|at| SystemTime::now().duration_since(at).unwrap_or_default() < AVATAR_MAX_AGE);
    let bytes = match (fresh, std::fs::read(&path)) {
        (true, Ok(bytes)) => bytes,
        (_, cached) => match download(http, login, &path).await {
            Ok(bytes) => bytes,
            // Offline: an old picture beats none.
            Err(err) => cached.map_err(|_| err)?,
        },
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}

async fn download(http: &reqwest::Client, login: &str, path: &Path) -> Result<Vec<u8>> {
    crate::simulate::check()?;
    let url = format!("https://github.com/{login}.png?size=64");
    let response = http.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?.to_vec();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::github::write_atomic(path, &bytes)?;
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
}
