//! Pure: state in, a ratatui `Text` out; only `tui.rs` touches the terminal.

pub mod bars;
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

/// Twice the gap between wrapped lines, so two voices read as two people rather
/// than one phrase that wrapped.
pub const VOICE_GAP: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub sung: Color,
    pub unsung: Color,
    /// A style rather than a colour: the default is the lyric colour drawn
    /// faint.
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
    /// Repaints `unsung`, not `sung`: the sweep is off by default, so `sung` is
    /// rarely drawn and an accent there would change nothing on screen.
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

/// Faint rather than palette 8, which is often barely above the background and
/// vanishes on a translucent one; the terminal keeps faint a readable step
/// down.
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
    /// As for [`Screen::Lyric`]. A background vocal is never swept, so only
    /// `reveal` applies to it.
    pub highlight: usize,
    pub reveal: usize,
}

/// What the screen should show right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen<'a> {
    /// `highlight` is how far the sweep has got; `reveal` is how much is inked.
    /// A syllable-timed word is laid out whole and its unreached part held
    /// blank.
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
                // Dimmed and flat for a background vocal; a duet partner is
                // drawn exactly as the line is, so the two read as equals.
                let style = match s.background {
                    true => RowStyle::Flat {
                        style: theme.dim,
                        reveal: s.reveal,
                    },
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
            // Centring ignores the rows an accent or a descender added, so the
            // capitals stay put from one word to the next.
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
    /// A background vocal: one style, never bold, and only the first `reveal`
    /// inked.
    Flat { style: Style, reveal: usize },
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

/// The line's size is settled first and is the floor: if the smallest font
/// cannot hold both, the second voice is dropped rather than shrinking the
/// line.
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
                    RowStyle::Flat { style, reveal } => (cell.src < reveal, style),
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
