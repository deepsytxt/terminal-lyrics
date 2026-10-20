//! Writes enhanced LRC and parses it, rather than building the display model a
//! second way: [`crate::lrc`] already knows voices, duets and word ends.

use super::model::{Line, Lyrics, Word};
use crate::lrc;

/// `mm:ss.SSS`.
fn stamp(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// Collapses whitespace runs: pretty-printed markup leaves indentation between
/// words that would open a hole in the line.
struct Body {
    out: String,
    space: bool,
}

impl Body {
    fn new() -> Self {
        // True at the start, so a line never opens with a space.
        Body {
            out: String::new(),
            space: true,
        }
    }

    fn text(&mut self, text: &str) {
        for c in text.chars() {
            if c.is_whitespace() {
                if !self.space {
                    self.out.push(' ');
                }
                self.space = true;
            } else {
                self.out.push(c);
                self.space = false;
            }
        }
    }

    fn tag(&mut self, at: f64) {
        self.out.push_str(&format!("<{}>", stamp(at)));
    }

    fn word(&mut self, word: &Word) {
        let core = word.text.trim();
        if core.is_empty() {
            self.text(&word.text);
            return;
        }
        let lead = &word.text[..word.text.len() - word.text.trim_start().len()];
        let trail = &word.text[word.text.trim_end().len()..];
        self.text(lead);
        self.tag(word.start);
        self.text(core);
        if word.end > 0.0 {
            self.tag(word.end);
        }
        self.text(trail);
    }
}

/// The lead a backing vocal sings over: the main line of its group.
fn parent(lines: &[Line], bg: &Line) -> Option<f64> {
    if bg.group == 0 {
        return None;
    }
    lines
        .iter()
        .find(|l| l.group == bg.group && !l.background)
        .map(|l| l.start)
}

/// Enhanced LRC for `lyrics`; empty when nothing is timed.
pub fn to_lrc(lyrics: &Lyrics) -> String {
    let mut out = String::new();
    for line in &lyrics.synced {
        out.push_str(&format!("[{}]", stamp(line.start)));
        if line.background {
            match parent(&lyrics.synced, line) {
                Some(p) => out.push_str(&format!("[bg:{}]", stamp(p))),
                None => out.push_str("[bg]"),
            }
        }
        if line.end > 0.0 {
            out.push_str(&format!("[end:{}]", stamp(line.end)));
        }
        let mut body = Body::new();
        if line.words.is_empty() {
            body.text(&line.text);
        } else {
            for w in &line.words {
                body.word(w);
            }
        }
        out.push_str(body.out.trim_end());
        out.push('\n');
    }
    out
}

pub fn to_display(lyrics: &Lyrics) -> lrc::Lyrics {
    lrc::parse(&to_lrc(lyrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start: f64, end: f64, text: &str) -> Word {
        Word {
            start,
            end,
            text: text.into(),
        }
    }

    #[test]
    fn words_keep_their_gaps_and_spaces() {
        let lyrics = Lyrics {
            synced: vec![Line {
                start: 1.0,
                end: 3.0,
                text: "Hello you".into(),
                words: vec![w(1.0, 1.5, "Hel"), w(1.5, 2.0, "lo "), w(2.5, 3.0, "you")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let shown = to_display(&lyrics);
        let line = &shown.lines[0];
        assert_eq!(line.text, "Hello you");
        assert_eq!(line.words.len(), 3);
        assert!(line.continues_word(1));
        assert!(!line.continues_word(2));
        assert_eq!(line.words[1].end, 2.0);
    }

    #[test]
    fn backing_vocal_hangs_on_its_lead() {
        let lyrics = Lyrics {
            synced: vec![
                Line {
                    start: 1.0,
                    end: 4.0,
                    text: "Lead".into(),
                    words: vec![w(1.0, 4.0, "Lead")],
                    group: 1,
                    ..Default::default()
                },
                Line {
                    start: 2.0,
                    end: 3.0,
                    text: "(ooh)".into(),
                    words: vec![w(2.0, 3.0, "(ooh)")],
                    background: true,
                    group: 1,
                    ..Default::default()
                },
                Line {
                    start: 5.0,
                    text: "Next".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let shown = to_display(&lyrics);
        assert_eq!(shown.lines.len(), 2);
        assert_eq!(shown.lines[0].secondary[0].text, "ooh");
        assert!(shown.lines[0].secondary[0].background);
    }

    #[test]
    fn plain_only_shows_nothing() {
        let lyrics = Lyrics {
            plain: "a\nb".into(),
            ..Default::default()
        };
        assert!(to_display(&lyrics).is_empty());
    }
}
