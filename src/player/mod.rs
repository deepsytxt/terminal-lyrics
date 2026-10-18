//! What the rest of the program knows about "something is playing".
//!
//! The TUI never talks to a media API directly: it consumes [`PlayerEvent`]s
//! from a channel. That seam is what lets `fake.rs` drive the whole sync engine
//! in a test with no player and no sleeping, and it is also where the supported
//! platforms diverge.
//!
//! # Backends
//!
//! Exactly one backend is compiled, chosen by target, and each exposes the same
//! `Session` and `PlayerHandle` items. There is no trait and no dynamic
//! dispatch, because there is never more than one implementation in the binary:
//!
//! | target | module | how it listens |
//! |---|---|---|
//! | Linux | [`mpris`] | D-Bus signals, plus a slow `Position` poll |
//! | macOS | [`macos`] | AppleScript, polled |
//! | Windows | [`windows`] | System Media Transport Controls, polled |
//! | other | [`unsupported`] | fails with a clear message |
//!
//! Everything above this line — [`Track`], [`PlayerEvent`], the ranking in
//! [`rank_players`] and the name matching in [`match_name`] — is
//! platform-neutral and stays testable everywhere.

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
    /// Stable identity for the saved sync nudges. Spotify gives a permalink
    /// or a `spotify:track:` URI either way; other players fall back to their
    /// own id, then to artist+title.
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

/// The last-resort cache key, when the player offers no id of its own.
///
/// Shared so that a track keyed this way on one platform hits the same cache
/// entry on another.
pub fn fallback_id(artist: &str, title: &str) -> String {
    format!("{artist}\u{1}{title}")
}

/// Everything a backend can say about the player in one round trip.
///
/// The point is the round trip: on macOS each field would otherwise cost its own
/// subprocess, so the poll loop asks once and gets all three.
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

/// Choose between players when the user has not named one.
///
/// Taking the first name alphabetically is wrong in practice: a browser
/// registers an idle `org.mpris.MediaPlayer2.chromium.instance…` that sorts
/// before `spotify` and reports no track at all, so the visualiser would follow
/// silence while music played next to it. Something actually playing wins.
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

/// Resolve `--player` against the names that are actually available.
///
/// Exact match first, then prefix, both case-insensitively, so `--player spot`
/// finds `spotify` and `--player chromium` finds `chromium.instance26065`.
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

/// Which player to pick up after the one being followed has closed, or `None`
/// to keep waiting.
///
/// Stricter than [`choose`]: a named player is taken as soon as it is back, but
/// unnamed, only one with something loaded will do. Otherwise the browser's
/// idle MPRIS entry would be picked up the moment the music player closed, and
/// the display would sit on it while the real player came back next to it.
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

/// How often to read `Position` from the player. This is the safety net for
/// players that do not emit `Seeked` — Spotify among them — not the main
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

/// Pick the player to follow, given a survey of what is running.
///
/// Shared by every backend so that `--player` behaves identically on all of
/// them. `what` names the thing that is missing, e.g. `"MPRIS player on the
/// session bus"`, because that part is the only bit that differs.
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
