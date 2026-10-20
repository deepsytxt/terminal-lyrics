//! The player is right whenever it speaks and the position is interpolated in
//! between; a `Tick` re-anchors only on real divergence, so it does not jump.

use std::time::{Duration, Instant};

use crate::clock::SyncClock;
use crate::lyrics::lookup_key;
use crate::player::{PlayerEvent, Track};

/// What the caller needs to react to, above just redrawing.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A different track is loaded, or the same one with different details
    /// to look it up by; the caller should fetch lyrics.
    Track(Option<Box<Track>>),
    /// The position moved somewhere unpredicted.
    Resynced,
    Gone,
    None,
}

pub struct SyncEngine {
    clock: SyncClock,
    track: Option<Track>,
    threshold: Duration,
}

impl SyncEngine {
    pub fn new(offset_ms: i64, threshold: Duration, now: Instant) -> Self {
        Self {
            clock: SyncClock::new(offset_ms, now),
            track: None,
            threshold,
        }
    }

    pub fn clock(&self) -> &SyncClock {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut SyncClock {
        &mut self.clock
    }

    pub fn track(&self) -> Option<&Track> {
        self.track.as_ref()
    }

    /// Position to look up in the lyrics, including the user's offset nudge.
    pub fn lyric_position(&self, now: Instant) -> f64 {
        self.clock.lyric_position(now)
    }

    pub fn is_playing(&self) -> bool {
        self.clock.is_playing()
    }

    pub fn apply(&mut self, event: PlayerEvent, now: Instant) -> Change {
        match event {
            PlayerEvent::Track(next) => {
                let next = next.map(|b| *b);
                // Players re-emit metadata when art loads or a rating changes.
                // Only a new id, or a new lookup key (Spotify's real length
                // arrives late), drops the lyrics.
                let same = match (&self.track, &next) {
                    (Some(a), Some(b)) => a.id == b.id && lookup_key(a) == lookup_key(b),
                    (None, None) => true,
                    _ => false,
                };
                self.track = next.clone();
                if same {
                    Change::None
                } else {
                    Change::Track(next.map(Box::new))
                }
            }
            PlayerEvent::Status { playing, position } => {
                self.clock.anchor(position, 1.0, playing, now);
                Change::Resynced
            }
            PlayerEvent::Seeked { position } => {
                self.clock
                    .anchor(position, 1.0, self.clock.is_playing(), now);
                Change::Resynced
            }
            PlayerEvent::Tick { position, playing } => {
                if playing != self.clock.is_playing() {
                    self.clock.anchor(position, 1.0, playing, now);
                    return Change::Resynced;
                }
                if self.clock.reconcile(position, self.threshold, now) {
                    Change::Resynced
                } else {
                    Change::None
                }
            }
            PlayerEvent::Gone => {
                self.track = None;
                Change::Gone
            }
        }
    }
}
