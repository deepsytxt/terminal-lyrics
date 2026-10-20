//! The AMLL TTML database: hand-timed word-by-word lyrics under CC0. There is
//! no search: the Spotify track id the player hands over is the key.

use std::time::Duration;

use super::Http;
use crate::lyrics::error::{Error, Result};
use crate::lyrics::format::ttml;
use crate::lyrics::model::{Lyrics, Request};

const BASE: &str = "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/spotify-lyrics";

pub async fn fetch(http: &Http, req: &Request) -> Result<Lyrics> {
    // Not cached as a miss: the same track played from Spotify has an id.
    let Some(id) = spotify_track_id(&req.spotify_id) else {
        return Err(Error::Uncached);
    };
    let request = http
        .client()
        .get(format!("{BASE}/{id}.ttml"))
        .timeout(Duration::from_secs(6));
    ttml::parse(&http.send(request).await?)
}

/// The track id out of whatever the player called the track: an
/// `open.spotify.com/track/` link, a `spotify:track:` URI, MPRIS's
/// `/com/spotify/track/` path, or a bare id.
pub fn spotify_track_id(key: &str) -> Option<&str> {
    let key = key.trim();
    let candidate = [
        "open.spotify.com/track/",
        "spotify:track:",
        "/com/spotify/track/",
    ]
    .iter()
    .find_map(|marker| key.split_once(marker).map(|(_, rest)| rest))
    .unwrap_or(key);
    let candidate = candidate.split(['?', '#', '/']).next().unwrap_or_default();
    // Spotify ids are 22 base62 characters; nothing else can be in the
    // database.
    (candidate.len() == 22 && candidate.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_comes_out_of_every_shape_spotify_gives_it_in() {
        let id = "5TRPicyLGbAF2LGBFbHGvO";
        for key in [
            "https://open.spotify.com/track/5TRPicyLGbAF2LGBFbHGvO",
            "https://open.spotify.com/track/5TRPicyLGbAF2LGBFbHGvO?si=abc",
            "spotify:track:5TRPicyLGbAF2LGBFbHGvO",
            "/com/spotify/track/5TRPicyLGbAF2LGBFbHGvO",
            "5TRPicyLGbAF2LGBFbHGvO",
        ] {
            assert_eq!(spotify_track_id(key), Some(id), "{key}");
        }
    }

    #[test]
    fn anything_else_is_not_an_id() {
        for key in [
            "",
            "A1B2C3D4E5F60718",
            "file:///music/song.flac",
            "/org/mpris/MediaPlayer2/Track/3",
            "spotify:episode:5TRPicyLGbAF2LGBFbHGvO",
        ] {
            assert_eq!(spotify_track_id(key), None, "{key}");
        }
    }
}
