//! Exactly one backend is compiled per target, each exposing the same `Session`
//! and `PlayerHandle`, so there is no trait: never more than one
//! implementation.

pub mod fake;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod mpris;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub mod unsupported;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(target_os = "linux")]
use mpris as backend;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use unsupported as backend;
#[cfg(target_os = "windows")]
use windows as backend;

pub use backend::{PlayerHandle, Session};

use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;

/// The currently loaded track, normalised out of whatever the platform reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// Stable identity for the saved sync nudges: the player's own id, else
    /// artist and title.
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Track length in seconds, when the player reports it.
    pub length: Option<f64>,
    /// Where the player is reading it from (`xesam:url`). A `file://` one is
    /// where lyrics files next to the track are looked for.
    pub url: Option<String>,
}

impl Track {
    pub fn is_usable(&self) -> bool {
        !self.title.trim().is_empty()
    }

    /// What the user sees while lyrics are being looked up.
    pub fn label(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} — {}", self.artist, self.title)
        }
    }
}

/// The last-resort key when the player offers no id, shared so a track keyed
/// this way hits the same cache entry on every platform.
pub fn fallback_id(artist: &str, title: &str) -> String {
    format!("{artist}\u{1}{title}")
}

/// Everything in one round trip: on macOS each field would otherwise cost its
/// own subprocess.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub playing: bool,
    pub position: f64,
    pub track: Option<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A different track was loaded (or the player cleared it).
    Track(Option<Box<Track>>),
    /// Play/pause. Carries the position the player reports at that moment.
    Status { playing: bool, position: f64 },
    /// The player jumped. Spotify does not send this reliably, hence `Tick`.
    Seeked { position: f64 },
    /// Result of the slow background position read, used to correct drift.
    Tick { position: f64, playing: bool },
    /// The player vanished.
    Gone,
}

pub type EventRx = mpsc::UnboundedReceiver<PlayerEvent>;
pub type EventTx = mpsc::UnboundedSender<PlayerEvent>;

/// What a candidate player looks like right now, for ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub name: String,
    pub playing: bool,
    pub has_track: bool,
}

/// Something actually playing wins: a browser's idle MPRIS entry sorts before
/// `spotify` and has no track.
pub fn rank_players(mut candidates: Vec<PlayerState>) -> Option<PlayerState> {
    fn score(p: &PlayerState) -> u8 {
        match (p.playing, p.has_track) {
            (true, true) => 3,
            (false, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }
    }
    // Name as the tiebreak, so the choice is stable between runs.
    candidates.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
    candidates.into_iter().next()
}

/// Exact match first, then prefix, case-insensitively: `--player spot` finds
/// `spotify`.
pub fn match_name(available: &[String], wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    available
        .iter()
        .find(|p| p.eq_ignore_ascii_case(wanted))
        .or_else(|| {
            let lower = wanted.to_lowercase();
            available
                .iter()
                .find(|p| p.to_lowercase().starts_with(&lower))
        })
        .cloned()
}

/// Stricter than [`choose`]: unnamed, only a player with something loaded will
/// do, or the browser's idle entry would be picked the moment the music closed.
pub fn pick_reconnect(states: Vec<PlayerState>, wanted: Option<&str>) -> Option<String> {
    match wanted {
        Some(w) => {
            let names: Vec<String> = states.into_iter().map(|p| p.name).collect();
            match_name(&names, w)
        }
        None => rank_players(states)
            .filter(|p| p.playing || p.has_track)
            .map(|p| p.name),
    }
}

/// The safety net for players that do not emit `Seeked` (Spotify), not the main
/// source of truth, so once a second is plenty.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A followed player: its event stream, and a second handle for play/pause.
/// Dropping it stops the event pump.
pub struct Link {
    pub name: String,
    pub control: PlayerHandle,
    pub events: EventRx,
    pump: tokio::task::JoinHandle<()>,
}

impl Link {
    pub async fn open(session: &Session, name: &str) -> Result<Self> {
        let handle = session.connect(name).await?;
        // The pump needs its own handle; the UI keeps one for play/pause.
        let control = session.connect(name).await?;
        let (events, pump) = handle.spawn(POLL_INTERVAL);
        Ok(Self {
            name: name.to_string(),
            control,
            events,
            pump,
        })
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

/// Shared by every backend so `--player` behaves the same on all of them.
/// `what` names what is missing, e.g. `"MPRIS player on the session bus"`.
pub fn choose(states: Vec<PlayerState>, wanted: Option<&str>, what: &str) -> Result<String> {
    if states.is_empty() {
        return Err(anyhow!(
            "no {what} is running — start a player and try again"
        ));
    }
    match wanted {
        None => Ok(rank_players(states)
            .map(|p| p.name)
            .expect("a non-empty survey always ranks")),
        Some(w) => {
            let names: Vec<String> = states.into_iter().map(|p| p.name).collect();
            match_name(&names, w)
                .ok_or_else(|| anyhow!("no player matching {w:?}; available: {}", names.join(", ")))
        }
    }
}
