//! Decoration geometry matrix — one row per [`UnderlineStyle`]
//! variant, len-equality-pinned against the mechanical registry
//! [`UnderlineStyle::ALL`]. Failures aggregate across the whole
//! matrix before the single assert, so one run reports every broken
//! variant, not just the first.
//!
//! Exact float equality IS the contract: the emitters are
//! deterministic arithmetic feeding mado's byte-deterministic L1
//! render ladder — epsilon comparison would mask drift.
#![allow(clippy::float_cmp)]

use engawa::decoration::{
    DASHED_DUTY, DASHED_PERIODS_PER_CELL, DOTTED_DUTY, DOTTED_PERIOD_PER_THICKNESS,
    DecorationRect, UnderlineGeometry, UnderlineMetrics, UnderlineStyle, emit_underline_rects,
    overline_rect,
};

const METRICS: UnderlineMetrics = UnderlineMetrics {
    cell_width: 10.0,
    underline_y: 17.0,
    thickness: 2.0,
    baseline: 15.0,
};

fn rect_eq(rect: DecorationRect, x: f32, y: f32, width: f32, height: f32) -> bool {
    rect.x == x && rect.y == y && rect.width == width && rect.height == height
}

struct MatrixRow {
    style: UnderlineStyle,
    /// Returns the violated-invariant labels (empty = row passes).
    check: fn(&UnderlineGeometry) -> Vec<&'static str>,
}

const MATRIX: &[MatrixRow] = &[
    MatrixRow {
        style: UnderlineStyle::None,
        check: |g| match g {
            UnderlineGeometry::None => vec![],
            _ => vec!["None must emit no geometry"],
        },
    },
    MatrixRow {
        style: UnderlineStyle::Single,
        check: |g| match g {
            UnderlineGeometry::Single(r) => {
                let mut v = vec![];
                if !rect_eq(*r, 0.0, METRICS.underline_y, METRICS.cell_width, METRICS.thickness) {
                    v.push("Single rect must be the canonical stroke rect");
                }
                v
            }
            _ => vec!["Single must emit exactly one rect"],
        },
    },
    MatrixRow {
        style: UnderlineStyle::Double,
        check: |g| match g {
            UnderlineGeometry::Double { upper, lower } => {
                let mut v = vec![];
                if upper.y == lower.y {
                    v.push("Double rects must have distinct y");
                }
                if lower.y <= upper.y {
                    v.push("Double lower rect must sit below upper");
                }
                if upper.height != METRICS.thickness || lower.height != METRICS.thickness {
                    v.push("Double rects must both be one-thickness tall");
                }
                if upper.width != METRICS.cell_width || lower.width != METRICS.cell_width {
                    v.push("Double rects must span the cell width");
                }
                v
            }
            _ => vec!["Double must emit exactly two rects"],
        },
    },
    MatrixRow {
        style: UnderlineStyle::Curly,
        check: |g| match g {
            UnderlineGeometry::Curly(band) => {
                let mut v = vec![];
                if band.amplitude <= 0.0 {
                    v.push("Curly amplitude must be positive");
                }
                if band.thickness != METRICS.thickness {
                    v.push("Curly thickness must mirror the metrics stroke");
                }
                if band.period != METRICS.cell_width {
                    v.push("Curly period must tile per cell");
                }
                if band.rect.height != 2.0 * band.amplitude + band.thickness {
                    v.push("Curly band rect must enclose wave + stroke");
                }
                v
            }
            // The SDF-band constraint: rects/runs here mean someone
            // tessellated the wave into quads — the exact regression
            // this row exists to catch.
            _ => vec!["Curly must emit a band, never rects"],
        },
    },
    MatrixRow {
        style: UnderlineStyle::Dotted,
        check: |g| match g {
            UnderlineGeometry::Run(run) => {
                let mut v = vec![];
                if run.period != DOTTED_PERIOD_PER_THICKNESS * METRICS.thickness {
                    v.push("Dotted period must be thickness-anchored");
                }
                if run.duty != DOTTED_DUTY {
                    v.push("Dotted duty must match the catalog constant");
                }
                if !rect_eq(
                    run.band,
                    0.0,
                    METRICS.underline_y,
                    METRICS.cell_width,
                    METRICS.thickness,
                ) {
                    v.push("Dotted band must be the canonical stroke rect");
                }
                v
            }
            _ => vec!["Dotted must emit a segment run, never per-dot quads"],
        },
    },
    MatrixRow {
        style: UnderlineStyle::Dashed,
        check: |g| match g {
            UnderlineGeometry::Run(run) => {
                let mut v = vec![];
                if run.period != METRICS.cell_width / DASHED_PERIODS_PER_CELL {
                    v.push("Dashed period must be cell-anchored");
                }
                if run.duty != DASHED_DUTY {
                    v.push("Dashed duty must match the catalog constant");
                }
                v
            }
            _ => vec!["Dashed must emit a segment run, never per-dash quads"],
        },
    },
];

/// Every style variant has a matrix row (len-pinned against the
/// mechanical registry, each registry entry exactly once) and every
/// row's geometry invariants hold. Aggregate-then-assert.
#[test]
fn every_underline_style_emits_its_geometry_shape() {
    assert_eq!(
        MATRIX.len(),
        UnderlineStyle::ALL.len(),
        "matrix must carry one row per UnderlineStyle variant"
    );
    let mut failures: Vec<(UnderlineStyle, &'static str)> = Vec::new();
    for style in UnderlineStyle::ALL {
        match MATRIX.iter().filter(|row| row.style == style).count() {
            1 => {}
            0 => failures.push((style, "registry variant missing a matrix row")),
            _ => failures.push((style, "registry variant has duplicate matrix rows")),
        }
    }
    for row in MATRIX {
        let geometry = emit_underline_rects(row.style, METRICS);
        for violation in (row.check)(&geometry) {
            failures.push((row.style, violation));
        }
    }
    assert!(
        failures.is_empty(),
        "{} matrix violations: {failures:#?}",
        failures.len()
    );
}

/// Dotted and Dashed are distinct patterns, not one shape with two
/// names — they must differ in period AND duty under real metrics.
#[test]
fn dotted_and_dashed_runs_are_distinct() {
    let dotted = emit_underline_rects(UnderlineStyle::Dotted, METRICS);
    let dashed = emit_underline_rects(UnderlineStyle::Dashed, METRICS);
    let (UnderlineGeometry::Run(dot), UnderlineGeometry::Run(dash)) = (dotted, dashed) else {
        panic!("dotted/dashed must both emit runs: {dotted:?} / {dashed:?}");
    };
    assert_ne!(dot.duty, dash.duty, "duty must distinguish dotted from dashed");
    assert_ne!(
        dot.period, dash.period,
        "period must distinguish dotted from dashed under these metrics"
    );
}

/// Overline is the top-edge mirror of the underline stroke.
#[test]
fn overline_is_top_edge_stroke() {
    let r = overline_rect(METRICS);
    assert!(
        rect_eq(r, 0.0, 0.0, METRICS.cell_width, METRICS.thickness),
        "overline must hug the cell top edge: {r:?}"
    );
}
