//! Tatara-lisp authoring surface — graphics expressed as Lisp data.
//!
//! This is engawa's "ishou for graphics": the same way ishou-tokens
//! let the fleet author *themes* as typed data, this module lets the
//! fleet author *effects* as typed data. A `(defeffect …)` /
//! `(defmaterial …)` form compiles 1:1 to the pure-data IR
//! ([`Effect`] / [`Material`]) every engawa consumer already speaks —
//! so an effect authored once in Lisp is portable to mado, ayatsuri,
//! and every future GPU consumer, and hot-reloadable from a shikumi
//! watcher (lisp source → IR → live graph swap).
//!
//! ## The Rust + Lisp pattern (org Pillar 1)
//!
//! Rust owns the types + invariants (the IR in [`crate::material`] /
//! [`crate::effect`]); tatara-lisp owns declarative authoring. The
//! boundary is the `#[derive(DeriveTataraDomain)]` proc-macro: each
//! spec struct declares one `(def… )` keyword and parses its keyword
//! args into a validated value. The spec types REUSE the IR types
//! directly (`ShaderSource`, `UniformBinding`, `BindingKind` are
//! already serde types), so `compile()` is a thin, total projection —
//! no parallel vocabulary, no drift.
//!
//! ## Form
//!
//! ```lisp
//! (defeffect
//!   :name     "scanlines"
//!   :priority 600
//!   :material (:name     "scanlines"
//!              :shader   (:kind inline :wgsl "…")
//!              :bindings ((:binding 0 :kind texture :resource "scene")
//!                         (:binding 1 :kind sampler :resource "catalog:sampler")
//!                         (:binding 2 :kind uniform :resource "scanlines:params"))))
//! ```
//!
//! Nested forms (`:material …`, `:shader …`, each `:bindings` row) are
//! plain keyword sub-lists consumed through tatara-lisp's
//! sexp→json→serde bridge into the reused IR types — exactly the
//! pattern `sui-spec` uses for its nested phase lists.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::effect::Effect;
use crate::material::{Material, ShaderSource, UniformBinding};

/// `(defmaterial …)` — a shader + its binding declarations. Compiles
/// 1:1 to [`Material`] (render state defaults to opaque/no-cull, like
/// every existing full-screen effect).
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone)]
#[tatara(keyword = "defmaterial")]
pub struct MaterialSpec {
    /// Operator-friendly name (matches the IR `Material::name`).
    pub name: String,
    /// WGSL/MSL source — `(:kind inline :wgsl "…")` or
    /// `(:kind path :path "wgsl/foo.wgsl")`. Reuses the IR
    /// [`ShaderSource`] enum (serde-tagged on `kind`).
    pub shader: ShaderSource,
    /// `@group(0) @binding(N)` declarations, in binding order. Each row
    /// is a reused IR [`UniformBinding`]; `group`/`stages` default.
    #[serde(default)]
    pub bindings: Vec<UniformBinding>,
}

impl MaterialSpec {
    /// Total projection to the IR [`Material`].
    #[must_use]
    pub fn compile(self) -> Material {
        Material::new(self.name, self.shader, self.bindings)
    }
}

/// `(defeffect …)` — an operator-toggleable effect: a material plus the
/// enable bit + render-order priority. Compiles 1:1 to [`Effect`].
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone)]
#[tatara(keyword = "defeffect")]
pub struct EffectSpec {
    /// Operator-friendly name (the YAML key + the IR `Effect::name`).
    pub name: String,
    /// Render-order priority (higher = later/closer to present). The IR
    /// caps this at `u16`; authored as an int here.
    pub priority: u32,
    /// Whether the effect contributes nodes. Omitted ⇒ `true` (an
    /// authored effect is on unless the consumer's config disables it).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// The material this effect contributes.
    pub material: MaterialSpec,
}

impl EffectSpec {
    /// Total projection to the IR [`Effect`].
    #[must_use]
    pub fn compile(self) -> Effect {
        Effect {
            name: self.name,
            enabled: self.enabled.unwrap_or(true),
            priority: self.priority as u16,
            material: self.material.compile(),
        }
    }
}

/// Parse every `(defeffect …)` form in `src` into IR [`Effect`]s.
///
/// # Errors
/// Propagates any tatara-lisp read / expand / compile error (bad
/// syntax, missing required keyword, ill-typed value).
pub fn effects_from_str(src: &str) -> tatara_lisp::Result<Vec<Effect>> {
    Ok(tatara_lisp::compile_typed::<EffectSpec>(src)?
        .into_iter()
        .map(EffectSpec::compile)
        .collect())
}

/// Parse every `(defmaterial …)` form in `src` into IR [`Material`]s.
///
/// # Errors
/// Propagates any tatara-lisp read / expand / compile error.
pub fn materials_from_str(src: &str) -> tatara_lisp::Result<Vec<Material>> {
    Ok(tatara_lisp::compile_typed::<MaterialSpec>(src)?
        .into_iter()
        .map(MaterialSpec::compile)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{BindingKind, ShaderSource};
    use crate::resource::ResourceId;

    const SCANLINES: &str = r#"
        (defeffect
          :name     "scanlines"
          :priority 600
          :material (:name     "scanlines"
                     :shader   (:kind inline :wgsl "// scanlines wgsl")
                     :bindings ((:binding 0 :kind texture :resource "scene")
                                (:binding 1 :kind sampler :resource "catalog:sampler")
                                (:binding 2 :kind uniform :resource "scanlines:params"))))
    "#;

    #[test]
    fn defeffect_compiles_to_ir() {
        let effects = effects_from_str(SCANLINES).expect("compile defeffect");
        assert_eq!(effects.len(), 1);
        let e = &effects[0];
        assert_eq!(e.name, "scanlines");
        assert_eq!(e.priority, 600);
        assert!(e.enabled, "omitted :enabled defaults to true");
        assert_eq!(e.material.name, "scanlines");
        assert!(matches!(e.material.shader, ShaderSource::Inline { .. }));
        assert_eq!(e.material.bindings.len(), 3);
        assert_eq!(e.material.bindings[0].binding, 0);
        assert_eq!(e.material.bindings[0].kind, BindingKind::Texture);
        assert_eq!(e.material.bindings[1].kind, BindingKind::Sampler);
        assert_eq!(e.material.bindings[2].kind, BindingKind::Uniform);
        assert_eq!(
            e.material.bindings[2].resource,
            ResourceId("scanlines:params".to_string())
        );
    }

    #[test]
    fn defmaterial_compiles_standalone() {
        let src = r#"
            (defmaterial
              :name   "tint"
              :shader (:kind path :path "wgsl/tint.wgsl")
              :bindings ((:binding 0 :kind texture :resource "scene")))
        "#;
        let mats = materials_from_str(src).expect("compile defmaterial");
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0].name, "tint");
        assert!(matches!(mats[0].shader, ShaderSource::Path { .. }));
        assert_eq!(mats[0].bindings.len(), 1);
    }

    #[test]
    fn explicit_enabled_false_is_honoured() {
        let src = r#"
            (defeffect
              :name     "off-by-default"
              :priority 700
              :enabled  #f
              :material (:name "x" :shader (:kind inline :wgsl "//") :bindings ()))
        "#;
        let effects = effects_from_str(src).expect("compile");
        assert!(!effects[0].enabled);
    }
}
