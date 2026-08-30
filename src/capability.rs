//! What a backend can actually honour — declared, and CHECKED.
//!
//! ── ★ THE DEFECT THIS EXISTS TO REMOVE ──────────────────────────────────────
//!
//! engawa's IR declares more than any one backend implements. Measured
//! 2026-08-30 across `engawa-wgpu/src/`: `node.draw`, `node.depth`,
//! `material.state`, `binding.group` and `binding.stages` are read **zero**
//! times. A consumer writing
//!
//! ```ignore
//! node.with_draw(DrawKind::Instanced { vertices, indices, index_count, instances })
//! ```
//!
//! got a **fullscreen triangle**. Not an error, not a warning, not a fallback
//! anyone chose — the field was parsed, validated, topologically sorted, and
//! then dropped on the floor. The graph compiled, the frame rendered, and the
//! picture was wrong.
//!
//! ── ★ WHY THIS BELONGS IN ENGAWA AND NOT IN A BACKEND ───────────────────────
//!
//! There are TWO backends over this IR, and they diverge. `engawa-wgpu` ignores
//! the five fields above; `asobi/crates/engawa-metal` (2,348 lines) implements
//! `DrawKind::{Indexed,Instanced}`, `DepthSpec`, blend/cull/front-face, per-stage
//! visibility, multi-group bindings and compute passes — precisely the set wgpu
//! lacks. Two independent implementations of one contract, silently disagreeing,
//! with nothing reporting the disagreement.
//!
//! That is the convergent-evidence signal: the shape is forced by the problem,
//! not chosen by an author. So the contract is owned by NEITHER backend.
//!
//! ── ★ FAIL-CLOSED, DELIBERATELY ─────────────────────────────────────────────
//!
//! [`Capabilities::MINIMAL`] is the default for any `Dispatcher` that does not
//! declare otherwise. A backend that has said nothing is assumed to support
//! nothing beyond a fullscreen draw with one bind group — so an undeclared
//! backend gets a loud refusal rather than a silent wrong picture. Defaulting to
//! "supports everything" would preserve exactly the failure being removed, and
//! would do it for every backend written after this file.

use crate::graph::CompiledGraph;
use crate::pipeline::DrawKind;

/// One optional feature of the IR that a backend may or may not honour.
///
/// Closed on purpose: a feature not named here cannot be declared, so adding an
/// IR field that a backend might ignore forces a decision about this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// `node.draw` beyond [`DrawKind::FullscreenQuad`] — indexed and instanced
    /// geometry.
    GeometryDraw,
    /// `node.depth` — a depth/stencil attachment and its compare state.
    Depth,
    /// `material.state` — blend, cull and front-face beyond the defaults.
    RenderState,
    /// `binding.group` > 0 — more than one bind group.
    MultiGroupBindings,
    /// `binding.stages` narrower than "all stages".
    StageVisibility,
    /// `PassKind::Compute`.
    ComputePass,
}

impl core::fmt::Display for Capability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::GeometryDraw => "geometry draws (DrawKind::Indexed / Instanced)",
            Self::Depth => "depth attachments (node.depth)",
            Self::RenderState => "blend/cull/front-face (material.state)",
            Self::MultiGroupBindings => "bind groups beyond group 0 (binding.group)",
            Self::StageVisibility => "per-stage binding visibility (binding.stages)",
            Self::ComputePass => "compute passes (PassKind::Compute)",
        })
    }
}

/// The set a backend honours, or a graph requires.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    set: std::collections::BTreeSet<Capability>,
}

impl Capabilities {
    /// A backend that honours nothing beyond a fullscreen draw with one bind
    /// group. **The default**, so silence means "no", never "yes".
    pub const MINIMAL: Self = Self {
        set: std::collections::BTreeSet::new(),
    };

    /// Build from an explicit list.
    #[must_use]
    pub fn from_list(caps: &[Capability]) -> Self {
        Self {
            set: caps.iter().copied().collect(),
        }
    }

    /// Every capability this IR defines — for a backend that genuinely honours
    /// all of them, and which must say so explicitly.
    #[must_use]
    pub fn all() -> Self {
        Self::from_list(&[
            Capability::GeometryDraw,
            Capability::Depth,
            Capability::RenderState,
            Capability::MultiGroupBindings,
            Capability::StageVisibility,
            Capability::ComputePass,
        ])
    }

    /// Does this set contain `c`?
    #[must_use]
    pub fn has(&self, c: Capability) -> bool {
        self.set.contains(&c)
    }

    /// Whether empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Capabilities in `self` that `other` does not have, lowest first.
    #[must_use]
    pub fn missing_from(&self, other: &Self) -> Vec<Capability> {
        self.set
            .iter()
            .copied()
            .filter(|c| !other.has(*c))
            .collect()
    }
}

/// What `graph` actually USES — not what it could use.
///
/// A graph that never sets a field requires nothing for it, so an existing
/// consumer drawing fullscreen quads through one bind group keeps working
/// against a `MINIMAL` backend. Only a consumer that declares a feature is held
/// to it, which is what makes adding this check a no-op for everything that
/// works today.
#[must_use]
pub fn required(graph: &CompiledGraph) -> Capabilities {
    let mut caps = Vec::new();
    for node in graph.iter_nodes() {
        for c in required_for_node(node).set {
            caps.push(c);
        }
    }
    Capabilities::from_list(&caps)
}

/// What one node USES. Split out so `dispatch_graph` can name the offending
/// node rather than reporting a whole-graph gap the operator must then hunt.
#[must_use]
pub fn required_for_node(node: &crate::node::Node) -> Capabilities {
    let mut caps = Vec::new();
    if !matches!(node.draw, DrawKind::FullscreenQuad) {
        caps.push(Capability::GeometryDraw);
    }
    if node.depth.is_some() {
        caps.push(Capability::Depth);
    }
    Capabilities::from_list(&caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_means_no_not_yes() {
        // The load-bearing default. If MINIMAL ever gains a member, an
        // undeclared backend silently starts claiming to honour it.
        assert!(Capabilities::MINIMAL.is_empty());
        assert!(!Capabilities::MINIMAL.has(Capability::GeometryDraw));
    }

    #[test]
    fn missing_from_names_every_gap_not_just_the_first() {
        let want = Capabilities::from_list(&[
            Capability::GeometryDraw,
            Capability::Depth,
            Capability::ComputePass,
        ]);
        let have = Capabilities::from_list(&[Capability::Depth]);
        let gaps = want.missing_from(&have);
        assert_eq!(gaps.len(), 2, "a partial answer hides the second failure");
        assert!(gaps.contains(&Capability::GeometryDraw));
        assert!(gaps.contains(&Capability::ComputePass));
    }

    #[test]
    fn a_backend_that_has_everything_is_missing_nothing() {
        assert!(
            Capabilities::all()
                .missing_from(&Capabilities::all())
                .is_empty()
        );
        assert_eq!(
            Capabilities::all()
                .missing_from(&Capabilities::MINIMAL)
                .len(),
            6,
            "MINIMAL must be missing every capability the IR defines"
        );
    }

    #[test]
    fn each_capability_renders_a_distinct_actionable_message() {
        let all = [
            Capability::GeometryDraw,
            Capability::Depth,
            Capability::RenderState,
            Capability::MultiGroupBindings,
            Capability::StageVisibility,
            Capability::ComputePass,
        ];
        let mut seen = std::collections::HashSet::new();
        for c in all {
            let m = c.to_string();
            // Each names the IR field, so an operator can find it.
            assert!(m.contains('('), "{c:?} does not name its IR field: {m}");
            assert!(seen.insert(m), "two capabilities render the same message");
        }
    }
}
