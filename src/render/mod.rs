//! Composing the screen.
//!
//! Everything here is pure: state in, a ratatui `Text` out. The terminal is
//! only touched by `tui.rs`, so the layout can be unit-tested.

pub mod case;
pub mod font;
pub mod layout;

pub use self::case::Case;

use ratatui::style::{Color, Modifier, Style};
// `Theme`'s fields are `Color`, so callers outside the crate need the type to
// say anything about them.
pub use ratatui::style::Color as ThemeColor;
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

use self::font::Font;
use self::layout::Layout;

/// Blank rows between wrapped lines of block art.
pub const LINE_GAP: usize = 1;

/// Blank rows between the two voices. Twice the gap between wrapped lines, so
/// the two read as two people singing rather than one phrase that happened to
/// wrap.
pub const VOICE_GAP: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub sung: Color,
    pub unsung: Color,
    /// Background vocals and the status lines. A whole style rather than a
    /// colour, because the default is not a colour of its own but the lyric
    /// colour drawn faint.
    pub dim: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Deliberately terminal palette colours, not fixed RGB: the display
            // then matches whatever scheme the user's terminal is set to.
            sung: Color::Cyan,
            unsung: Color::White,
            dim: faint(Color::White),
        }
    }
}

impl Theme {
    /// The default theme, with the lyric text repainted in `accent`.
    ///
    /// `unsung` is the slot that moves, and it has to be that one: `sung` is
    /// only reached where the sweep has passed, and the sweep is off by default
    /// (`Sweep::Never`), so an accent applied to `sung` would be a setting that
    /// parses, prints back, and never changes a pixel. `unsung` is what a line
    /// is drawn in almost all of the time.
    ///
    /// `dim` follows it, still faint, so a background vocal reads as a quieter
    /// version of the line. `sung` stays a palette entry, so the highlight
    /// still follows the terminal's own scheme — which is the property the
    /// default was chosen for, and the one a list of hardcoded themes would
    /// have cost.
    pub fn with_accent(accent: Option<[u8; 3]>) -> Self {
        match accent {
            None => Self::default(),
            Some([r, g, b]) => Self {
                unsung: Color::Rgb(r, g, b),
                dim: faint(Color::Rgb(r, g, b)),
                ..Self::default()
            },
        }
    }

    /// Draw the dimmed text in exactly `colour` instead, for a terminal whose
    /// faint attribute is too weak, too strong, or missing.
    pub fn with_dim(self, colour: Option<[u8; 3]>) -> Self {
        match colour {
            None => self,
            Some([r, g, b]) => Self {
                dim: Style::new().fg(Color::Rgb(r, g, b)),
                ..self
            },
        }
    }
}

/// The lyric colour with the terminal's faint attribute, rather than a grey of
/// our own. Palette 8 ("bright black", what `DarkGray` names) is often tuned to
/// sit barely above the background, and on a translucent or blurred one it all
/// but vanishes; faint is worked out by the terminal from the text and what is
/// behind it, so it stays a readable step down from the line. A terminal that
/// ignores faint draws it at full strength, and the smaller font still says it
/// is the background voice.
fn faint(colour: Color) -> Style {
    Style::new().fg(colour).add_modifier(Modifier::DIM)
}

/// A second voice on screen alongside the line being read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondVoice<'a> {
    pub text: &'a str,
    /// A background vocal: dimmed, and drawn a size smaller than the line it
    /// sits over. The other half of a duet is neither — it is a co-equal voice.
    pub background: bool,
    /// As for [`Screen::Lyric`]: how far the sweep has got, and how much is
    /// inked. A duet partner goes word by word and sweeps just like the line;
    /// both are ignored for a background vocal, which is always drawn whole.
    pub highlight: usize,
    pub reveal: usize,
}

/// What the screen should show right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen<'a> {
    /// A lyric line.
    ///
    /// `highlight` is how many characters the sweep has passed. `reveal` is how
    /// many are drawn at all: a word timed in syllables is laid out whole, so it
    /// sits where it will finally sit, but only the syllables reached so far are
    /// inked. The rest holds its space blank instead of shifting the word.
    Lyric {
        text: &'a str,
        highlight: usize,
        reveal: usize,
        /// Drawn above `text`.
        second: Option<SecondVoice<'a>>,
    },
    /// Track known, lyrics being looked up.
    Searching { label: &'a str },
    /// Looked up, nothing found.
    NoLyrics { label: &'a str },
    /// Nothing is playing.
    Idle { message: &'a str },
}

/// Render a screen into styled text sized for `width` x `height`.
pub fn render(
    screen: &Screen<'_>,
    font: &Font,
    width: u16,
    height: u16,
    theme: Theme,
) -> Text<'static> {
    let width = width as usize;
    let height = height as usize;

    match screen {
        Screen::Lyric {
            text,
            highlight,
            reveal,
            second,
        } => {
            // A line that is nothing but a background vocal has no main text,
            // and must still draw the voice that is actually singing.
            if text.trim().is_empty() && second.is_none_or(|s| s.text.trim().is_empty()) {
                return Text::default();
            }
            let (main, second) = fit_voices(text, *second, font, width, height);
            let mut rows: Vec<Line<'static>> = Vec::new();
            if let Some(s) = &second {
                // Dimmed and flat for a background vocal. A duet partner is drawn
                // exactly as the line is, so the two read as equals rather than
                // as a line and its shadow.
                let style = match s.background {
                    true => RowStyle::Flat(theme.dim),
                    false => RowStyle::Sweep {
                        highlight: s.highlight,
                        reveal: s.reveal,
                    },
                };
                rows.extend(block_rows(&s.fitted.layout, style, width, theme));
                if !main.layout.is_empty() {
                    rows.extend(std::iter::repeat_n(Line::default(), VOICE_GAP));
                }
            }
            rows.extend(block_rows(
                &main.layout,
                RowStyle::Sweep {
                    highlight: *highlight,
                    reveal: *reveal,
                },
                width,
                theme,
            ));
            // The rows an accent or a descender added, at the very top and the
            // very bottom: centring ignores them, so the capitals stay put as
            // one word with a descender follows one without.
            let first = second
                .as_ref()
                .and_then(|s| s.fitted.layout.lines.first())
                .or(main.layout.lines.first());
            let last = main
                .layout
                .lines
                .last()
                .or(second.as_ref().and_then(|s| s.fitted.layout.lines.last()));
            let overhang = (first.map_or(0, |l| l.above), last.map_or(0, |l| l.below));
            vertically_centre(rows, height, overhang)
        }
        Screen::Searching { label } => {
            status_text(label, "searching for lyrics", width, height, theme)
        }
        Screen::NoLyrics { label } => status_text(label, "no lyrics found", width, height, theme),
        Screen::Idle { message } => status_text("", message, width, height, theme),
    }
}

/// How a run of glyph art is coloured.
enum RowStyle {
    /// A lead voice, the line or a duet partner: swept characters bold in
    /// `sung`, the rest in `unsung`, and only the first `reveal` inked at all.
    Sweep { highlight: usize, reveal: usize },
    /// A background vocal: one style, all of it inked, never bold.
    Flat(Style),
}

/// One voice laid out at the size it will be drawn.
struct Fitted {
    layout: Layout,
    font: Font,
}

/// A fitted second voice, and how to colour it.
struct FittedSecond {
    fitted: Fitted,
    background: bool,
    highlight: usize,
    reveal: usize,
}

/// Fit the line, and the voice over it, into the space there is.
///
/// The order matters. The line's own size is settled first, against the whole
/// budget, and is the floor: the pair may step down together to make room, but
/// if even the smallest font cannot hold both, the second voice is dropped and
/// the line goes back to the size it would have had alone. Shrinking the lyric
/// for something that then gets dropped would be the worst of both.
fn fit_voices(
    main: &str,
    second: Option<SecondVoice<'_>>,
    preferred: &Font,
    width: usize,
    height: usize,
) -> (Fitted, Option<FittedSecond>) {
    let (layout, font) = layout::layout_fitting(main, preferred, width, height, LINE_GAP);
    let solo = Fitted { layout, font };

    let Some(second) = second.filter(|s| !s.text.trim().is_empty()) else {
        return (solo, None);
    };

    // Never larger than the line manages on its own: a font that cannot hold
    // one voice is not going to hold two.
    for font in layout::fallback_chain(&solo.font) {
        let second_font = match second.background {
            true => font::smaller_than(font.name).unwrap_or_else(|| font.clone()),
            false => font.clone(),
        };
        let lm = layout::layout(main, &font, width);
        let ls = layout::layout(second.text, &second_font, width);
        let mut rows = ls.rows(LINE_GAP);
        if !lm.is_empty() {
            rows += VOICE_GAP + lm.rows(LINE_GAP);
        }
        if rows <= height.max(1) {
            return (
                Fitted { layout: lm, font },
                Some(FittedSecond {
                    fitted: Fitted {
                        layout: ls,
                        font: second_font,
                    },
                    background: second.background,
                    highlight: second.highlight,
                    reveal: second.reveal,
                }),
            );
        }
    }

    (solo, None)
}

/// Rows of glyph art for one voice, centred horizontally but not vertically —
/// the two voices are centred together, as one block.
fn block_rows(layout: &Layout, style: RowStyle, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();

    for (i, vline) in layout.lines.iter().enumerate() {
        if i > 0 {
            for _ in 0..LINE_GAP {
                rows.push(Line::default());
            }
        }
        let pad = width.saturating_sub(vline.width) / 2;
        for row in 0..vline.height {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            for (j, cell) in vline.cells.iter().enumerate() {
                if j > 0 && layout.tracking > 0 {
                    spans.push(Span::raw(" ".repeat(layout.tracking)));
                }
                let art = cell.rows.get(row).cloned().unwrap_or_default();
                let (reached, style) = match style {
                    RowStyle::Sweep { highlight, reveal } => (
                        cell.src < reveal,
                        if cell.src < highlight {
                            Style::default().fg(theme.sung).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.unsung)
                        },
                    ),
                    RowStyle::Flat(style) => (true, style),
                };
                // Not reached yet: hold the space, draw nothing in it.
                let art = match reached {
                    true => pad_to(&art, cell.width),
                    false => " ".repeat(cell.width),
                };
                spans.push(Span::styled(art, style));
            }
            rows.push(Line::from(spans));
        }
    }

    rows
}

/// A plain character can be narrower than the cell holding it; pad so columns
/// stay aligned.
fn pad_to(s: &str, width: usize) -> String {
    let len = s.width();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

fn status_text(
    label: &str,
    message: &str,
    width: usize,
    height: usize,
    theme: Theme,
) -> Text<'static> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    if !label.is_empty() {
        rows.push(centred(label, width, Style::default().fg(theme.unsung)));
        rows.push(Line::default());
    }
    rows.push(centred(message, width, theme.dim));
    vertically_centre(rows, height, (0, 0))
}

fn centred(text: &str, width: usize, style: Style) -> Line<'static> {
    let len = text.chars().count();
    let pad = width.saturating_sub(len) / 2;
    Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(text.to_string(), style),
    ])
}

/// Centre `rows` in `height`, as if the `overhang` rows at the top and the
/// bottom were not there — but never so far that they leave the screen.
fn vertically_centre(
    rows: Vec<Line<'static>>,
    height: usize,
    overhang: (usize, usize),
) -> Text<'static> {
    if rows.len() >= height {
        return Text::from(rows);
    }
    let (above, below) = overhang;
    let core = rows.len().saturating_sub(above + below);
    let pad = ((height - core) / 2)
        .saturating_sub(above)
        .min(height - rows.len());
    let mut out: Vec<Line<'static>> = Vec::with_capacity(height);
    out.extend(std::iter::repeat_n(Line::default(), pad));
    out.extend(rows);
    Text::from(out)
}
