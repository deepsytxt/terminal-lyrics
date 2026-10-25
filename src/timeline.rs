//! The sweep is interpolated when the source has no word tags; that guess is
//! display-only and never written to disk.

use std::ops::Range;

use crate::lrc::{Lyrics, Word};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    /// Playback is before the first timestamp; the intro is still running.
    Intro { until: f64 },
    /// `index` into [`Lyrics::lines`].
    Line { index: usize },
    /// Past the end of the last line.
    Outro,
}

/// Laid out whole and revealed a syllable at a time: laying out only the sung
/// syllables would make the word crawl as it grew.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveWord {
    /// Char range of the whole word in [`crate::lrc::Line::text`].
    pub range: Range<usize>,
    /// End of the syllable reached so far, as a char index into the line.
    pub sung_to: usize,
}

impl ActiveWord {
    /// How many of the word's characters have been reached.
    pub fn revealed(&self) -> usize {
        self.sung_to.saturating_sub(self.range.start)
    }
}

/// A few whole words of a line, on screen on their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Char range of the chunk in [`crate::lrc::Line::text`].
    pub range: Range<usize>,
    /// Chars of the chunk reached so far, counted from its start. The rest is
    /// laid out but held blank, so the chunk does not shift as words arrive.
    pub reveal: usize,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    lyrics: Lyrics,
}

impl Timeline {
    pub fn new(lyrics: Lyrics) -> Self {
        Self { lyrics }
    }

    pub fn lyrics(&self) -> &Lyrics {
        &self.lyrics
    }

    pub fn len(&self) -> usize {
        self.lyrics.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lyrics.lines.is_empty()
    }

    pub fn line(&self, index: usize) -> Option<&crate::lrc::Line> {
        self.lyrics.lines.get(index)
    }

    /// Which line covers `pos`. O(log n) — the display asks this ~30 times a
    /// second, and a long song is a few hundred lines.
    pub fn locate(&self, pos: f64) -> Position {
        let lines = &self.lyrics.lines;
        let Some(first) = lines.first() else {
            return Position::Outro;
        };
        if pos < first.start {
            return Position::Intro { until: first.start };
        }
        // Index of the last line whose start is <= pos.
        let idx = lines.partition_point(|l| l.start <= pos) - 1;
        let line = &lines[idx];
        if idx + 1 == lines.len() && pos >= line.end {
            return Position::Outro;
        }
        Position::Line { index: idx }
    }

    /// Windowed rather than tied to the line, since a second voice rarely lines
    /// up with it; where two overlap the most recent wins, as in
    /// [`Timeline::locate`].
    pub fn secondary(&self, index: usize, pos: f64) -> Option<&crate::lrc::Secondary> {
        self.lyrics
            .lines
            .get(index)?
            .secondary
            .iter()
            .rev()
            .find(|s| pos >= s.start && pos < s.display_end())
    }

    /// `None` before the first word. In a gap between words the previous word
    /// is held: flashing through real gaps would be worse than holding.
    pub fn active_word(&self, index: usize, pos: f64) -> Option<ActiveWord> {
        let line = self.lyrics.lines.get(index)?;
        active_word_in(&line.text, &line.words, pos)
    }

    /// [`Timeline::active_word`] for the second voice; `None` for a voice
    /// without word timings, which is drawn whole.
    pub fn second_active_word(&self, index: usize, pos: f64) -> Option<ActiveWord> {
        let s = self.secondary(index, pos)?;
        active_word_in(&s.text, &s.words, pos)
    }

    /// Only real word timings move it: the partner's window starts where it was
    /// cut off, so there is nothing honest to interpolate. Always 0 for a
    /// backing vocal.
    pub fn second_highlight_chars(&self, index: usize, pos: f64) -> usize {
        let Some(s) = self.secondary(index, pos).filter(|s| !s.background) else {
            return 0;
        };
        sweep_words(&s.words, s.text.chars().count(), pos).unwrap_or(0)
    }

    /// The few words of line `index` that belong on screen at `pos`: at most
    /// `max_words` of them, split where the line breathes (see
    /// [`chunk_ranges`]). `None` before the line's first word is sung.
    ///
    /// Real word timings pick the chunk when the line has them. Otherwise the
    /// time is guessed from character counts, over the part of the line's
    /// window that is plausibly singing (see [`sung_span`]); the guess is
    /// display-only, like the interpolated sweep.
    pub fn chunk_at(&self, index: usize, pos: f64, max_words: usize) -> Option<Chunk> {
        let line = self.lyrics.lines.get(index)?;
        let chars: Vec<char> = line.text.chars().collect();
        let words = word_ranges(&chars);
        let chunks = group_words(&chars, &words, max_words);
        let last = chunks.len().checked_sub(1)?;

        if !line.words.is_empty() {
            let active = active_word_in(&line.text, &line.words, pos)?;
            let at = chunks
                .iter()
                .position(|c| active.range.start < c.end)
                .unwrap_or(last);
            let range = chunks[at].clone();
            let reveal = active.sung_to.saturating_sub(range.start).min(range.len());
            return Some(Chunk { range, reveal });
        }

        let total = chars.len();
        let sung = sung_span(line.end - line.start, total);
        let frac = ((pos - line.start) / sung).clamp(0.0, 1.0);
        let reached = (total as f64 * frac) as usize;
        let at = chunks.iter().position(|c| reached < c.end).unwrap_or(last);
        let range = chunks[at].clone();
        // Each word of the chunk appears as the one before it is finished.
        let reveal = words
            .iter()
            .find(|w| w.start >= range.start && w.end <= range.end && w.end > reached)
            .map_or(range.len(), |w| w.end - range.start);
        Some(Chunk { range, reveal })
    }

    /// Follows word tags when there are any; otherwise an estimate, linear
    /// across the line by character count.
    pub fn highlight_chars(&self, index: usize, pos: f64) -> usize {
        let Some(line) = self.lyrics.lines.get(index) else {
            return 0;
        };
        let total = line.char_len();
        if total == 0 {
            return 0;
        }

        if let Some(chars) = sweep_words(&line.words, total, pos) {
            return chars;
        }

        let span = line.end - line.start;
        if span <= f64::EPSILON {
            return total;
        }
        let frac = ((pos - line.start) / span).clamp(0.0, 1.0);
        ((total as f64) * frac).round() as usize
    }
}

/// How a line splits into chunks of at most `max_words` words, as char ranges
/// of `text`. A sentence end (`.` `!` `?` `…`) always closes a chunk, and the
/// words of each sentence are shared out evenly, so seven words at three a
/// time come out as 3 + 2 + 2 rather than 3 + 3 + 1: a lone last word reads
/// as a mistake.
pub fn chunk_ranges(text: &str, max_words: usize) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    group_words(&chars, &word_ranges(&chars), max_words)
}

/// Char ranges of the whitespace-separated words of `chars`.
fn word_ranges(chars: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in chars.iter().enumerate() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(s..chars.len());
    }
    out
}

/// Whether `word` ends a sentence, ignoring a closing quote or bracket.
fn ends_phrase(chars: &[char], word: &Range<usize>) -> bool {
    chars[word.clone()]
        .iter()
        .rev()
        .find(|&&c| !matches!(c, '"' | '\'' | '»' | '”' | '’' | ')'))
        .is_some_and(|&c| matches!(c, '.' | '!' | '?' | '…'))
}

fn group_words(chars: &[char], words: &[Range<usize>], max_words: usize) -> Vec<Range<usize>> {
    let max_words = max_words.max(1);
    let mut out = Vec::new();
    let mut from = 0;
    for (i, word) in words.iter().enumerate() {
        if i + 1 < words.len() && !ends_phrase(chars, word) {
            continue;
        }
        let phrase = &words[from..=i];
        let groups = phrase.len().div_ceil(max_words);
        let mut at = 0;
        for g in 0..groups {
            let take = (phrase.len() - at).div_ceil(groups - g);
            out.push(phrase[at].start..phrase[at + take - 1].end);
            at += take;
        }
        from = i + 1;
    }
    out
}

/// Seconds of a line's window (`window`, up to the next line) that are
/// actually singing. A line is stamped where it starts and runs to the next
/// one, so the window includes any pause after it; counting all of it would
/// show the last words late. Capped at a plausible pace for `chars`
/// characters and at most 92% of the window.
fn sung_span(window: f64, chars: usize) -> f64 {
    let pace = 0.6 + chars as f64 * 0.12;
    (window.max(0.0) * 0.92).min(pace).max(0.5)
}

/// The word being sung in a voice with these word timings. See
/// [`Timeline::active_word`].
fn active_word_in(text: &str, words: &[Word], pos: f64) -> Option<ActiveWord> {
    let started = words.partition_point(|w| w.start <= pos);
    if started == 0 {
        return None;
    }
    let cur = started - 1;
    Some(ActiveWord {
        range: crate::lrc::word_bounds(text, words, cur)?,
        sung_to: words[cur].range.end,
    })
}

/// How far real word timings have swept across a voice `total` characters long.
/// `None` when there are no word timings to follow.
fn sweep_words(words: &[Word], total: usize, pos: f64) -> Option<usize> {
    if words.is_empty() {
        return None;
    }
    // Last word that has already started.
    let started = words.partition_point(|w| w.start <= pos);
    if started == 0 {
        return Some(0);
    }
    let w = &words[started - 1];
    let span = w.end - w.start;
    let frac = if span > f64::EPSILON {
        ((pos - w.start) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let width = w.range.end.saturating_sub(w.range.start) as f64;
    Some((w.range.start + (width * frac).round() as usize).min(total))
}
