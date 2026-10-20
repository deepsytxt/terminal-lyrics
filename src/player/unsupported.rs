//! Platforms with no backend yet. On a BSD the answer is probably
//! [`super::mpris`] as it stands: MPRIS is freedesktop, so widen its `cfg`.

use std::time::Duration;

use anyhow::{Result, anyhow};

use super::{EventRx, PlayerState, Snapshot, Track};

fn unsupported<T>() -> Result<T> {
    Err(anyhow!(
        "no player backend for this platform yet — terminal-lyrics supports \
         Linux (MPRIS), macOS (Spotify and Music) and Windows (media controls)"
    ))
}

pub struct Session;

impl Session {
    pub async fn open() -> Result<Self> {
        unsupported()
    }

    pub async fn survey(&self) -> Result<Vec<PlayerState>> {
        unsupported()
    }

    pub async fn resolve(&self, _wanted: Option<&str>) -> Result<String> {
        unsupported()
    }

    pub async fn connect(&self, _name: &str) -> Result<PlayerHandle> {
        unsupported()
    }
}

pub struct PlayerHandle;

impl PlayerHandle {
    pub async fn snapshot(&self) -> Option<Snapshot> {
        None
    }

    pub async fn track(&self) -> Option<Track> {
        None
    }

    pub async fn position(&self) -> Option<f64> {
        None
    }

    pub async fn playing(&self) -> bool {
        false
    }

    pub async fn play_pause(&self) -> Result<()> {
        unsupported()
    }

    pub fn spawn(self, _poll_interval: Duration) -> (EventRx, tokio::task::JoinHandle<()>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            let _ = tx.send(super::PlayerEvent::Gone);
        });
        (rx, handle)
    }
}
