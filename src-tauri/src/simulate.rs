//! Development builds can pretend GitHub is down or the device has no network, to check
//! what the app shows without unplugging anything. Every request to GitHub (API, Pages,
//! release assets, avatars) and the reachability probe ask `check` first. Release builds
//! never simulate anything.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Network {
    #[default]
    Normal,
    /// GitHub answers every request with a server error; the network itself works.
    GithubDown,
    /// Nothing answers: requests fail to connect, the probe too.
    NoInternet,
}

static NETWORK: AtomicU8 = AtomicU8::new(0);

pub fn network() -> Network {
    match NETWORK.load(Ordering::Relaxed) {
        1 => Network::GithubDown,
        2 => Network::NoInternet,
        _ => Network::Normal,
    }
}

pub fn set_network(network: Network) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Err(Error::Unsupported(
            "network simulation is for development builds".to_owned(),
        ));
    }
    let value = match network {
        Network::Normal => 0,
        Network::GithubDown => 1,
        Network::NoInternet => 2,
    };
    NETWORK.store(value, Ordering::Relaxed);
    log::info!("simulated network: {network:?}");
    Ok(())
}

/// The error a request to GitHub would get right now, if any.
pub fn check() -> Result<()> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    match network() {
        Network::Normal => Ok(()),
        Network::GithubDown => Err(Error::GitHub(
            "503 Service Unavailable (simulated)".to_owned(),
        )),
        Network::NoInternet => Err(Error::Offline),
    }
}

/// Whether a TCP connection to GitHub would open: only "no internet" stops it.
pub fn reachable() -> bool {
    !cfg!(debug_assertions) || network() != Network::NoInternet
}
