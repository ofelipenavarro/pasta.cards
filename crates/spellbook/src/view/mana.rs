//! Mana symbols: colored pips and mana-cost strings.
//!
//! Port of `desktop/ui/js/icons.js`'s `manaCostHtml` in the form the native
//! renderer needs — no SVG paths (the engine only rasterises text and
//! rounded shapes; inlining 9 SVG path parsers is out of scope), so pips
//! are the CSS fallback treatment: round pill in the mana color's own
//! background with the letter (or number) drawn on top, dark on light
//! like the JS's `#1a1a1a` glyph fill.
//!
//! The module is `pub` for the deck-detail work that consumes it next;
//! meanwhile only the tests exercise it, hence the allow.

#![allow(dead_code)]

use engine::compositor::{Compositor, SceneNode};
use engine::theme::Theme;

use super::text;

/// Mana-pip background colors — `deck-bits.js`'s FILTER_COLORS / `icons.js`'s
/// MANA_COLORS share the same palette (the pastels a dark glyph reads on).
pub fn pip_color(letter: char) -> [f32; 4] {
    let hex = match letter {
        'W' => 0xfffbd5,
        'U' => 0xaae0fa,
        'B' => 0xcbc2bf,
        'R' => 0xf9aa8f,
        'G' => 0x9bd3ae,
        'S' => 0xdcedf7,
        _ => 0xd8d3c9, // generic/gold number background
    };
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// Ink on top of [`pip_color`] — the JS draws glyphs in `#1a1a1a`.
const PIP_INK: [f32; 4] = [0.10, 0.10, 0.12, 1.0];

/// One symbol of a cost: a face letter, a number, or a compound ("W/U", "2/B").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pip {
    Letter(char),
    Number(String),
    Compound(String),
}

/// Parse one `{...}` inner into a pip. `Phyrexian` keeps the color letter;
/// the pip draws the two parts tiny, which is also how the JS renders
/// `{2/B}` hybrids it has no glyph for.
fn parse_inner(inner: &str) -> Option<Pip> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    if inner.contains('/') {
        Some(Pip::Compound(inner.to_string()))
    } else if inner.chars().all(|c| c.is_ascii_digit()) {
        Some(Pip::Number(inner.to_string()))
    } else {
        inner
            .chars()
            .next()
            .map(|c| Pip::Letter(c.to_ascii_uppercase()))
    }
}

/// Every pip of a mana-cost string, in order, preserving `//` face splits
/// as [`Cost::split`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cost {
    pub pips: Vec<Pip>,
    /// Indexes into `pips` where a new face begins (`{2}{W} // {1}{U}` → [0, 2]).
    pub splits: Vec<usize>,
}

/// Parse a Scryfall mana cost. Answers an empty cost for anything without
/// braces — callers render nothing, like `manaCostHtml("")`.
pub fn parse_cost(cost: &str) -> Cost {
    let mut pips = Vec::new();
    let mut splits = vec![0usize];
    for face in cost.split("//") {
        if !splits.ends_with(&[pips.len()]) {
            splits.push(pips.len());
        }
        let mut bytes = face.as_bytes();
        let mut offset = 0usize;
        while let Some(pos) = bytes.iter().position(|&b| b == b'{') {
            let rel_end = bytes[pos..].iter().position(|&b| b == b'}');
            let Some(rel_end) = rel_end else { break };
            if let Some(pip) = parse_inner(&face[offset + pos + 1..offset + pos + rel_end]) {
                pips.push(pip);
            }
            let consumed = pos + rel_end + 1;
            bytes = &bytes[consumed..];
            offset += consumed;
        }
    }
    Cost { pips, splits }
}

impl Pip {
    /// The string drawn inside the pill: numbers and compounds as-is, a
    /// letter as the letter. Phyrexian `{B/P}` shows "B/P" tiny — the JS
    /// replaces it with the Phyrexian glyph, which we cannot rasterise.
    pub fn label(&self) -> String {
        match self {
            Pip::Letter(c) => c.to_string(),
            Pip::Number(n) => n.clone(),
            Pip::Compound(c) => c.to_string(),
        }
    }

    /// Background of the pill. Compounds take the color of the first part
    /// that is a mana letter (`2/B` → black, `B/P` → black, `W/U` → white);
    /// numbers are generic. The JS uses the colored background with a dark
    /// glyph, and Phyrexian costs keep the mana color, not the gray "P".
    pub fn color(&self) -> [f32; 4] {
        match self {
            Pip::Letter(c) => pip_color(*c),
            Pip::Number(_) => pip_color('0'),
            Pip::Compound(c) => {
                let letter = c
                    .split('/')
                    .find_map(|part| {
                        let ch = part.chars().next()?;
                        if "WUBRGSC".contains(ch) { Some(ch) } else { None }
                    })
                    .unwrap_or('C');
                pip_color(letter)
            }
        }
    }
}

/// Diameter of one pip.
pub const PIP_D: f32 = 18.0;
/// Gap between pips.
pub const PIP_GAP: f32 = 3.0;

/// Width a cost string renders at, so layouts can reserve space.
pub fn cost_width(cost: &str) -> f32 {
    let n = parse_cost(cost).pips.len();
    if n == 0 {
        0.0
    } else {
        n as f32 * PIP_D + (n - 1) as f32 * PIP_GAP
    }
}

/// Draw one pip centred in `rect` (the pip is `PIP_D` wide, centred
/// horizontally; `y` is the top of the pip).
pub fn render_pip(c: &mut Compositor, x: f32, y: f32, pip: &Pip, _theme: &Theme) {
    c.push(SceneNode::RoundedRect {
        x,
        y,
        w: PIP_D,
        h: PIP_D,
        color: pip.color(),
        corner_radius: PIP_D / 2.0,
        border_width: 0.0,
        border_color: [0.0; 4],
    });
    let label = pip.label();
    let size = match pip {
        Pip::Letter(_) => 11.0,
        Pip::Number(_) => 10.0,
        Pip::Compound(_) => 7.0,
    };
    let w = label.len() as f32 * size * 0.62;
    text(
        c,
        &label,
        size,
        700,
        x + (PIP_D - w) / 2.0,
        y + PIP_D / 2.0 - size * 0.72,
        PIP_INK,
    );
}

/// Render a mana-cost string (`{3}{B}{B}`) as pips starting at (x, y).
/// Returns the x after the last pip, so rows can chain more content.
/// Split faces get a visible `//` between them, as in `manaCostHtml`.
pub fn render_mana_cost(c: &mut Compositor, cost: &str, x: f32, y: f32, theme: &Theme) -> f32 {
    let parsed = parse_cost(cost);
    if parsed.pips.is_empty() {
        return x;
    }
    let mut cx = x;
    for (i, pip) in parsed.pips.iter().enumerate() {
        if parsed.splits.contains(&i) && i > 0 {
            text(c, "//", 10.0, 500, cx, y + 4.0, theme.colors.text_dim.0);
            cx += 16.0;
        }
        render_pip(c, cx, y, pip, theme);
        cx += PIP_D + PIP_GAP;
    }
    cx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_costs() {
        let cost = parse_cost("{3}{B}{B}");
        assert_eq!(
            cost.pips,
            vec![
                Pip::Number("3".into()),
                Pip::Letter('B'),
                Pip::Letter('B'),
            ]
        );
        assert_eq!(cost.splits, vec![0]);
    }

    #[test]
    fn parses_split_faces() {
        let cost = parse_cost("{2}{W} // {1}{U}");
        assert_eq!(
            cost.pips,
            vec![
                Pip::Number("2".into()),
                Pip::Letter('W'),
                Pip::Number("1".into()),
                Pip::Letter('U'),
            ]
        );
        assert_eq!(cost.splits, vec![0, 2]);
    }

    #[test]
    fn parses_hybrid_and_phyrexian() {
        assert_eq!(parse_cost("{W/U}").pips, vec![Pip::Compound("W/U".into())]);
        assert_eq!(parse_cost("{B/P}").pips, vec![Pip::Compound("B/P".into())]);
        assert_eq!(parse_cost("{2/B}").pips, vec![Pip::Compound("2/B".into())]);
    }

    #[test]
    fn compound_takes_first_parts_color() {
        assert_eq!(Pip::Compound("2/B".into()).color(), pip_color('B'));
        assert_eq!(Pip::Compound("B/P".into()).color(), pip_color('B'));
        assert_eq!(Pip::Compound("W/U".into()).color(), pip_color('W'));
    }

    #[test]
    fn junk_costs_are_empty() {
        assert!(parse_cost("").pips.is_empty());
        assert!(parse_cost("no braces").pips.is_empty());
        assert!(cost_width("") == 0.0);
    }

    #[test]
    fn width_matches_layout_math() {
        let w = parse_cost("{1}{W}{U}").pips.len() as f32;
        assert!((cost_width("{1}{W}{U}") - (3.0 * PIP_D + 2.0 * PIP_GAP)).abs() < 0.001);
        let _ = w;
    }
}