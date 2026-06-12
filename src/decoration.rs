//! Text-decoration vocabulary + typed geometry emitters (M3-C2).
//!
//! Engawa owns the ONE definition of [`UnderlineStyle`] /
//! [`UnderlineColor`] — consumers (mado's grid `Attrs`, ayatsuri,
//! any future text surface) store and re-export these types rather
//! than mirroring local enums. The remediation review mandated this
//! single ownership: a mado-local enum "mirrored later" is the
//! duplication bug class this module deletes.
//!
//! The geometry emitters are pure `f32` arithmetic over a typed
//! [`UnderlineMetrics`] input and return plain typed structs — no
//! wgpu, no allocation. Engawa stays a pure-data crate.
//!
//! ## Coordinate space
//!
//! All emitted geometry is **cell-local**: origin at the cell's
//! top-left corner, `x` grows rightward, `y` grows downward, units
//! are pixels (whatever unit the consumer's metrics are in). The
//! renderer translates per cell — or merges adjacent cells' bands
//! into one run before dispatch.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Storage vocabulary
// ---------------------------------------------------------------------------

/// Direct-RGB payload for [`UnderlineColor::Rgb`]. Field-compatible
/// with mado's `terminal::Color` (`r`/`g`/`b` public `u8`s) so the
/// migration is a constructor swap at the SGR-parse boundary, not a
/// call-site reshape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Typed underline style — SGR `4` (Single), `4:0..4:5` sub-param
/// wire, `24` / `4:0` reset.
///
/// Variant set, order, and derive set are the drop-in contract for
/// mado's grid `Attrs` (which lives inside an interned `Style`, hence
/// `Hash`). Do not add variants without landing their geometry in
/// [`emit_underline_rects`] — the total match there is the forcing
/// function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl UnderlineStyle {
    /// Mechanical registry of every variant. Matrix tests assert
    /// len-equality against this — never hand-maintain a second list.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Single,
        Self::Double,
        Self::Curly,
        Self::Dotted,
        Self::Dashed,
    ];

    /// Lowercase wire name (mado's MCP `CellSnapshot.underline`
    /// field). Matches the serde rename — one vocabulary.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Single => "single",
            Self::Double => "double",
            Self::Curly => "curly",
            Self::Dotted => "dotted",
            Self::Dashed => "dashed",
        }
    }
}

impl fmt::Display for UnderlineStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed underline colour — SGR `58` (set, indexed or RGB) / `59`
/// (reset to `Default` = follow the cell's fg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnderlineColor {
    /// No explicit underline colour — render with the cell fg.
    #[default]
    Default,
    /// `58:5:N` / `58;5;N` — palette index.
    Indexed(u8),
    /// `58:2::r:g:b` / `58;2;r;g;b` — direct RGB.
    Rgb(Rgb),
}

impl fmt::Display for UnderlineColor {
    /// Wire rendering for mado's MCP `CellSnapshot.underline_color`
    /// field: `indexed(N)` / `#rrggbb`. `Default` is never serialized
    /// there (the snapshot carries `None` instead) but renders as
    /// `default` for completeness.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::Indexed(n) => write!(f, "indexed({n})"),
            Self::Rgb(c) => write!(f, "#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry inputs
// ---------------------------------------------------------------------------

/// Renderer-derived metrics for one cell, cell-local coordinates
/// (see module docs). All `f32`; the emitters are total pure
/// functions over whatever values arrive — they mirror degenerate
/// inputs (zero/negative sizes) into degenerate geometry rather than
/// clamping, so the consumer's L1 byte-determinism ladder holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UnderlineMetrics {
    /// Horizontal extent of the cell.
    pub cell_width: f32,
    /// Top edge of the (single) underline stroke, from cell top.
    pub underline_y: f32,
    /// Stroke thickness.
    pub thickness: f32,
    /// Baseline y, from cell top. Feeds the curly amplitude: the
    /// baseline→underline gap is the vertical room the wave owns.
    pub baseline: f32,
}

// ---------------------------------------------------------------------------
// Geometry outputs
// ---------------------------------------------------------------------------

/// Axis-aligned rect, cell-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DecorationRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// RLE segment run for Dotted/Dashed — period + duty over a band
/// rect, NOT a `Vec` of per-dot quads. The renderer (or an SDF
/// node) paints `x` where `((x - band.x) % period) < period * duty`.
/// One instance covers the whole band regardless of dot count, so
/// geometry cost is O(1) per run of cells, not O(dots).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentRun {
    /// The full extent the pattern repeats within.
    pub band: DecorationRect,
    /// Repeat distance along x.
    pub period: f32,
    /// Painted fraction of each period, in `(0, 1]`.
    pub duty: f32,
}

/// Curly-underline band — intended for an SDF material node that
/// evaluates the sine wave analytically in the fragment shader.
/// Explicitly NOT per-segment quads: tessellating the wave into
/// quads re-introduces the per-dot geometry explosion this
/// vocabulary exists to prevent, and aliases under fractional
/// scaling. Consumers must treat `rect` as the paint region and
/// `period`/`amplitude`/`thickness` as shader uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurlyBand {
    /// Conservative paint region: vertically centered on the wave
    /// centerline, `2 * amplitude + thickness` tall.
    pub rect: DecorationRect,
    /// One full wave per `period` along x (= cell width, so the
    /// pattern tiles seamlessly across a run of cells).
    pub period: f32,
    /// Peak vertical displacement of the wave centerline.
    pub amplitude: f32,
    /// Stroke thickness of the wave.
    pub thickness: f32,
}

/// Emitted underline geometry — sum-over-product so the per-style
/// shape invariants hold by construction: `Single` carries exactly
/// one rect, `Double` exactly two, Dotted/Dashed one [`SegmentRun`],
/// Curly one [`CurlyBand`]. A "Double with three rects" is
/// unrepresentable (truly: no expressible value), not validated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UnderlineGeometry {
    /// `UnderlineStyle::None` — nothing to paint.
    None,
    Single(DecorationRect),
    Double {
        upper: DecorationRect,
        lower: DecorationRect,
    },
    /// Dotted/Dashed — which one is encoded in the run's
    /// period/duty constants, not a separate variant: the renderer
    /// dispatch is identical.
    Run(SegmentRun),
    Curly(CurlyBand),
}

// ---------------------------------------------------------------------------
// Pattern constants — single source of truth; tests + shaders
// project from these, never from re-derived literals.
// ---------------------------------------------------------------------------

/// Dotted period = `DOTTED_PERIOD_PER_THICKNESS * thickness`
/// (square dots one-thickness wide, one-thickness gap).
pub const DOTTED_PERIOD_PER_THICKNESS: f32 = 2.0;
/// Painted fraction of each dotted period.
pub const DOTTED_DUTY: f32 = 0.5;
/// Dashed period = `cell_width / DASHED_PERIODS_PER_CELL` — the
/// period divides the cell width so dashes tile seamlessly across
/// cell boundaries.
pub const DASHED_PERIODS_PER_CELL: f32 = 2.0;
/// Painted fraction of each dashed period. Differs from
/// [`DOTTED_DUTY`] by construction — the matrix test pins it.
pub const DASHED_DUTY: f32 = 0.75;

// ---------------------------------------------------------------------------
// Emitters
// ---------------------------------------------------------------------------

/// Emit the typed geometry for one cell's underline.
///
/// Pure + total: every `(style, metrics)` pair produces a defined
/// value, no `Result`, no clamping (degenerate metrics mirror into
/// degenerate geometry — the renderer owns clipping).
#[must_use]
pub fn emit_underline_rects(style: UnderlineStyle, metrics: UnderlineMetrics) -> UnderlineGeometry {
    let stroke = DecorationRect {
        x: 0.0,
        y: metrics.underline_y,
        width: metrics.cell_width,
        height: metrics.thickness,
    };
    // MECHANICAL FORCING FUNCTION — this match is total with NO
    // wildcard arm: adding an UnderlineStyle variant is a compile
    // error here until its geometry exists. Never add `_ =>`.
    match style {
        UnderlineStyle::None => UnderlineGeometry::None,
        UnderlineStyle::Single => UnderlineGeometry::Single(stroke),
        UnderlineStyle::Double => UnderlineGeometry::Double {
            upper: stroke,
            lower: DecorationRect {
                // One-thickness gap between the two strokes.
                y: metrics.underline_y + 2.0 * metrics.thickness,
                ..stroke
            },
        },
        UnderlineStyle::Curly => {
            // The wave owns the baseline→underline gap; floor at one
            // thickness so degenerate metrics still wave visibly.
            let amplitude = (metrics.underline_y - metrics.baseline).max(metrics.thickness);
            UnderlineGeometry::Curly(CurlyBand {
                rect: DecorationRect {
                    x: 0.0,
                    y: metrics.underline_y - amplitude,
                    width: metrics.cell_width,
                    height: 2.0 * amplitude + metrics.thickness,
                },
                period: metrics.cell_width,
                amplitude,
                thickness: metrics.thickness,
            })
        }
        UnderlineStyle::Dotted => UnderlineGeometry::Run(SegmentRun {
            band: stroke,
            period: DOTTED_PERIOD_PER_THICKNESS * metrics.thickness,
            duty: DOTTED_DUTY,
        }),
        UnderlineStyle::Dashed => UnderlineGeometry::Run(SegmentRun {
            band: stroke,
            period: metrics.cell_width / DASHED_PERIODS_PER_CELL,
            duty: DASHED_DUTY,
        }),
    }
}

/// Overline (SGR 53) rect — flush against the cell's top edge,
/// same thickness as the underline stroke.
#[must_use]
pub fn overline_rect(metrics: UnderlineMetrics) -> DecorationRect {
    DecorationRect {
        x: 0.0,
        y: 0.0,
        width: metrics.cell_width,
        height: metrics.thickness,
    }
}

#[cfg(test)]
mod tests {
    // Exact float equality IS the contract here: the emitters are
    // deterministic arithmetic feeding a byte-deterministic render
    // ladder (mado L1) — epsilon comparison would mask drift.
    #![allow(clippy::float_cmp)]

    use super::*;

    /// FORCING FUNCTION for [`UnderlineStyle::ALL`]: the index match
    /// is total, so adding a variant fails to compile here until the
    /// registry (and its length) is extended in the same change.
    #[test]
    fn all_registry_is_total_and_distinct() {
        let mut seen = [false; UnderlineStyle::ALL.len()];
        for style in UnderlineStyle::ALL {
            let idx = match style {
                UnderlineStyle::None => 0,
                UnderlineStyle::Single => 1,
                UnderlineStyle::Double => 2,
                UnderlineStyle::Curly => 3,
                UnderlineStyle::Dotted => 4,
                UnderlineStyle::Dashed => 5,
            };
            assert!(!seen[idx], "duplicate registry entry: {style}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&s| s), "registry misses a variant");
    }

    /// Pin the trait surface mado's style interning requires:
    /// `Attrs` lives inside an interned `Style` keyed by `Hash + Eq`.
    #[test]
    fn vocabulary_trait_surface() {
        fn pin<T: Copy + Eq + std::hash::Hash + Send + Sync + Default + fmt::Debug>() {}
        // Rgb pins everything EXCEPT Default — mado's Color defaults
        // to WHITE; a derived zero-default here would silently flip
        // that to black at the migration boundary.
        fn pin_no_default<T: Copy + Eq + std::hash::Hash + Send + Sync + fmt::Debug>() {}
        pin::<UnderlineStyle>();
        pin::<UnderlineColor>();
        pin_no_default::<Rgb>();
    }

    #[test]
    fn defaults_match_mado_vocabulary() {
        assert_eq!(UnderlineStyle::default(), UnderlineStyle::None);
        assert_eq!(UnderlineColor::default(), UnderlineColor::Default);
    }

    /// Pin the serde wire shape the fleet stores — a silent change
    /// here corrupts every persisted Attrs the consumers serialize.
    #[test]
    fn serde_wire_shape_is_pinned() {
        let style_wire: Vec<(UnderlineStyle, &str)> = UnderlineStyle::ALL
            .iter()
            .map(|&s| {
                (
                    s,
                    match s {
                        UnderlineStyle::None => "\"none\"",
                        UnderlineStyle::Single => "\"single\"",
                        UnderlineStyle::Double => "\"double\"",
                        UnderlineStyle::Curly => "\"curly\"",
                        UnderlineStyle::Dotted => "\"dotted\"",
                        UnderlineStyle::Dashed => "\"dashed\"",
                    },
                )
            })
            .collect();
        let mut failures: Vec<(UnderlineStyle, String)> = Vec::new();
        for (style, expected) in style_wire {
            let got = serde_json::to_string(&style).expect("serialize");
            if got != expected {
                failures.push((style, got.clone()));
            }
            let back: UnderlineStyle = serde_json::from_str(&got).expect("roundtrip");
            if back != style {
                failures.push((style, got));
            }
        }
        assert!(failures.is_empty(), "style wire drift: {failures:?}");

        let color_wire = [
            (UnderlineColor::Default, "\"default\""),
            (UnderlineColor::Indexed(9), "{\"indexed\":9}"),
            (
                UnderlineColor::Rgb(Rgb::new(10, 11, 12)),
                "{\"rgb\":{\"r\":10,\"g\":11,\"b\":12}}",
            ),
        ];
        let mut color_failures: Vec<(UnderlineColor, String)> = Vec::new();
        for (color, expected) in color_wire {
            let got = serde_json::to_string(&color).expect("serialize");
            if got != expected {
                color_failures.push((color, got.clone()));
            }
            let back: UnderlineColor = serde_json::from_str(&got).expect("roundtrip");
            if back != color {
                color_failures.push((color, got));
            }
        }
        assert!(color_failures.is_empty(), "color wire drift: {color_failures:?}");
    }

    /// Display parity with the mado impls this module replaces —
    /// the MCP `CellSnapshot` wire reads these strings today.
    #[test]
    fn display_matches_mado_wire() {
        assert_eq!(UnderlineStyle::Curly.to_string(), "curly");
        assert_eq!(UnderlineStyle::None.as_str(), "none");
        assert_eq!(UnderlineColor::Default.to_string(), "default");
        assert_eq!(UnderlineColor::Indexed(9).to_string(), "indexed(9)");
        assert_eq!(
            UnderlineColor::Rgb(Rgb::new(0x0a, 0x0b, 0x0c)).to_string(),
            "#0a0b0c"
        );
    }

    #[test]
    fn overline_sits_on_top_edge() {
        let m = UnderlineMetrics {
            cell_width: 10.0,
            underline_y: 17.0,
            thickness: 2.0,
            baseline: 15.0,
        };
        let r = overline_rect(m);
        assert_eq!(r.y, 0.0);
        assert_eq!(r.x, 0.0);
        assert_eq!(r.width, m.cell_width);
        assert_eq!(r.height, m.thickness);
    }
}
