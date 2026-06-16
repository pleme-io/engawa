//! Engawa (縁側) — typed render-graph IR for pleme-io GPU consumers.
//!
//! An *engawa* is the wooden veranda that runs along the outside of
//! a traditional Japanese house — a layered transition space
//! between the bare ground and the polished interior. The image:
//! every visual effect composes onto the operator's screen through
//! a series of layered passes, the way a kabuki actor steps through
//! the engawa before entering the room.
//!
//! ## Frame
//!
//! Bevy's `bevy_render` is the reference for what a "good" render
//! graph looks like — typed nodes, explicit dependencies, deterministic
//! compilation. **Engawa borrows the design, not the dependency.**
//! Bevy assumes a game-loop ECS world; mado (engawa's first
//! consumer) is a terminal emulator that's idle most of the time
//! and demands byte-deterministic rendering (see mado's L1/L2/L3
//! verification ladder). Pulling in bevy wholesale would invalidate
//! that work.
//!
//! Engawa keeps the parts of `bevy_render`'s design that fit:
//!
//! * **Typed DAG of render nodes** with explicit input/output
//!   resources (textures, samplers, uniform buffers, storage
//!   buffers).
//! * **Pass kinds** — render, compute, blit — declared per node so
//!   the compiler picks the right wgpu encoder dispatch.
//! * **Topological compile** — nodes are sorted into a deterministic
//!   execution order; missing inputs / cycles are caught at compile
//!   time, not at the first frame.
//! * **Material/effect composition** — a chain of effects composes
//!   via shared render targets; turn one off, the graph re-orders
//!   without breaking dependencies.
//! * **Hot-reload-ready IR** — pure data, serializable, swappable
//!   from a shikumi watcher (effects authored in tatara-lisp →
//!   compiled to engawa IR → live-reloaded into the graph).
//!
//! Engawa drops the parts that don't fit a terminal:
//!
//! * No ECS world. Mado tracks its own cell grid + cursor; engawa
//!   doesn't impose a component model.
//! * No render-every-vsync assumption. Engawa graphs are *passive*
//!   — the consumer decides when to dispatch.
//! * No asset pipeline. Shader source is a `String` or a path;
//!   shikumi's notify watcher (already shipped fleet-wide) handles
//!   the hot-reload story.
//! * No 3D camera / lighting / PBR. Effects target a 2D viewport;
//!   the math collapses to one resolution uniform.
//!
//! ## Layering
//!
//! Engawa is one of three pleme-io GPU primitives. Each owns a
//! distinct concern:
//!
//! | Crate | Concern | Used by |
//! |---|---|---|
//! | [`garasu`] | GPU context, text rendering, headless harness | mado, ayatsuri, hibikine, namimado, … |
//! | `engawa` (this crate) | Render-graph IR, effects composition | mado (next), ayatsuri (TBD) |
//! | `madori` | winit window + event loop | mado, ayatsuri |
//!
//! [`garasu`]: https://github.com/pleme-io/garasu
//!
//! ## Status
//!
//! v0.1.0 ships the pure-data IR + topo-sort + validation +
//! extensive unit-test coverage. Wgpu wiring (taking a
//! `CompiledGraph` and dispatching against a `wgpu::Device` + a
//! garasu `HeadlessTarget`) lands in v0.2 once the IR has been
//! exercised against mado's existing post-pipeline.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/engawa/0.1.0")]

pub mod decoration;
pub mod dispatch;
pub mod effect;
pub mod error;
pub mod graph;
pub mod material;
pub mod node;
pub mod pass;
pub mod pipeline;
pub mod resource;

pub use decoration::{
    CurlyBand, DecorationRect, Rgb, SegmentRun, UnderlineColor, UnderlineGeometry,
    UnderlineMetrics, UnderlineStyle, emit_underline_rects, overline_rect,
};
pub use dispatch::{
    DispatchError, Dispatcher, RecordedDispatch, RecordingDispatcher, ResourceBindings,
    ResourceHandle,
};
pub use effect::Effect;
pub use error::{EngawaError, ValidationError};
pub use graph::{CompiledGraph, RenderGraph};
pub use material::{BindingKind, Material, ShaderSource, ShaderStages, UniformBinding};
pub use node::{Node, NodeId};
pub use pass::{ComputeDispatch, PassKind};
pub use pipeline::{
    BlendMode, CompareFunction, CullMode, DepthSpec, DrawKind, FrontFace, RenderState,
    TextureFormat, Topology,
};
pub use resource::{ResourceId, ResourceKind};
