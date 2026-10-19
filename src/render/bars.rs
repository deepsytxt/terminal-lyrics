//! Spectrum bars, drawn in eighth blocks.
//!
//! Each cell can show a bar one to eight eighths full, so a strip `rows` tall
//! has `rows * 8` steps. Levels come in on a fixed scale, up to `max`, and are
//! fitted to however many rows there are this frame.

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

/// Columns per bar, and between bars. Two wide reads as a bar rather than a
/// line, and the gap keeps neighbours from merging into a skyline.
pub const BAR_WIDTH: usize = 2;
pub const BAR_GAP: usize = 1;

const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// How many bars fit across `width` columns. Always even, because cava's
/// stereo output mirrors one channel against the other and wants pairs; zero
/// when not even a pair fits.
pub fn bar_count(width: usize) -> usize {
    let n = (width + BAR_GAP) / (BAR_WIDTH + BAR_GAP);
    n - n % 2
}

/// The strip, `rows` tall and centred across `width`, a level of `max`
/// reaching the top. Bars that do not fit — a frame from before a resize — are
/// dropped from both ends, so the middle, where stereo puts the bass, stays
/// put.
pub fn render(levels: &[u16], max: u16, rows: usize, width: usize, style: Style) -> Text<'static> {
    let steps = rows * 8;
    let max = usize::from(max.max(1));
    // Rounded, so a whisper of noise does not leave a sliver under every bar.
    let scale = |level: u16| (usize::from(level) * steps + max / 2) / max;
    let fits = bar_count(width);
    let skip = levels.len().saturating_sub(fits) / 2;
    let shown = &levels[skip..levels.len().min(skip + fits)];
    let used = (shown.len() * (BAR_WIDTH + BAR_GAP)).saturating_sub(BAR_GAP);
    let pad = " ".repeat(width.saturating_sub(used) / 2);

    (0..rows)
        .map(|row| {
            // How many eighths sit below this row's cell.
            let floor = (rows - 1 - row) * 8;
            let mut text = pad.clone();
            for (i, &level) in shown.iter().enumerate() {
                if i > 0 {
                    text.extend(std::iter::repeat_n(' ', BAR_GAP));
                }
                let fill = scale(level).saturating_sub(floor).min(8);
                text.extend(std::iter::repeat_n(EIGHTHS[fill], BAR_WIDTH));
            }
            Line::from(Span::styled(text.trim_end().to_string(), style))
        })
        .collect::<Vec<_>>()
        .into()
}
