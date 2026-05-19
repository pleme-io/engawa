# engawa (縁側)

Typed render-graph IR for pleme-io GPU consumers. Bevy's render-graph
design distilled to a 1,000-line crate that fits mado's needs — no
ECS, no game loop, no asset pipeline, no 3D camera math.

> **An *engawa* is the wooden veranda that runs along the outside of
> a traditional Japanese house** — a layered transition space between
> the bare ground and the polished interior. The image: every visual
> effect composes onto the operator's screen through a series of
> layered passes, the way a kabuki actor steps through the engawa
> before entering the room.

## What this is

A pure-data typed IR for declaring GPU render graphs:

- **`RenderGraph`** — operator-authored DAG: resources + nodes +
  inputs + outputs.
- **`Node`** — one unit of GPU work (a render / compute / blit pass).
- **`Material`** — shader source + uniform/texture bindings.
- **`Effect`** — operator-facing effect entry (material + enable +
  ordering priority).
- **`compile() -> CompiledGraph`** — topo-sorts nodes, catches cycles,
  multiple-writer collisions, unbound inputs, unbound outputs.

```rust
use engawa::{RenderGraph, Node, Material, ResourceKind, ShaderSource};

let scanlines = Material {
    name: "scanlines".into(),
    shader: ShaderSource::path("/etc/mado/effects/scanlines.wgsl"),
    bindings: vec![],
};

let graph = RenderGraph::default()
    .with_resource("swap", ResourceKind::External)
    .with_resource("scene", ResourceKind::Texture { width: Some(800), height: Some(600) })
    .with_resource("post",  ResourceKind::Texture { width: Some(800), height: Some(600) })
    .with_input("swap")
    .with_output("post")
    .with_node(Node::clear("clear", "scene"))
    .with_node(Node::fullscreen_effect("scanlines", scanlines, "scene", "post"));

let compiled = graph.compile()?; // catches every topology error here
for node in compiled.iter_nodes() {
    // consumer dispatches against wgpu...
}
```

## What this is NOT

- Not a renderer. Engawa is the *IR*; the consumer (mado, future
  ayatsuri) owns the wgpu side. Engawa never touches a `wgpu::Device`.
- Not bevy. Bevy is the *design reference*; engawa borrows the typed-
  DAG + pass-kinds + topo-compile patterns and drops the ECS / game
  loop / asset pipeline / 3D camera that don't fit a terminal.
- Not an effect catalog. The mado consumer ships built-in effects
  (CRT, scanlines, bloom, …); engawa just owns the type they speak.

## Status

- **v0.1** — pure-data IR + topo sort + cycle detection + 20 unit
  tests. **This release.**
- **v0.2** (planned) — consumer-side wgpu dispatcher: take a
  `CompiledGraph` + a `garasu::GpuContext` + a `garasu::HeadlessTarget`
  / `wgpu::Surface`, walk the execution order, dispatch each node.
  Mado adopts; its hand-rolled 4-pass sequence becomes a graph.
- **v0.3** (planned) — tatara-lisp `(defeffect …)` / `(defmaterial …)`
  / `(defgraph …)` forms compile to engawa IR. Hot-reload via
  shikumi's notify watcher.
- **v0.4** (planned) — built-in effect catalog: CRT curvature,
  scanlines, separable Gaussian blur, glow/bloom, gamma toggles,
  colorblind LUTs. Per-effect WGSL + engawa IR shipped together.

## Why this exists

[`theory/ENGAWA.md`](docs/ENGAWA.md) is the canonical spec: why a
custom IR beats forking bevy_render, the typed-DAG design, the layering
with `garasu` + `madori` + `shikumi`, and the 4-phase roadmap.

## Test it

```bash
cargo test          # 20 unit tests, all pure-data, no GPU
```

## Layering

| Crate | Concern | Used by |
|---|---|---|
| [garasu](https://github.com/pleme-io/garasu) | GPU context, text, headless harness | mado, ayatsuri, hibikine, namimado |
| **engawa** (this crate) | Render-graph IR, effects composition | mado (next), ayatsuri (TBD) |
| [madori](https://github.com/pleme-io/madori) | winit window + event loop | mado, ayatsuri |

License: MIT
