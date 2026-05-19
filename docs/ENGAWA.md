# engawa — typed render-graph IR for pleme-io

Canonical spec. The companion to `~/code/github/pleme-io/theory/`
docs that cite this from the GPU-consumer chapters.

## I. The frame

pleme-io's GPU stack has three roles, currently split across two
crates:

| Role | Concern | Where it lives today |
|---|---|---|
| **Surface + window** | winit, swapchain config, present | `madori` |
| **Device + text + headless** | wgpu adapter/device/queue, glyphon, test harness | `garasu` |
| **Cells + cursor + render pipeline** | terminal grid, rect emission, post-process passes | `mado` (hand-rolled) |

The third role is the gap. Mado's render pipeline is a hand-rolled
4-pass sequence in `mado/src/render.rs`:

1. Clear pass — `LoadOp::Clear(bg_color)` against the scene target.
2. Rect pass — instanced quads for cell backgrounds + cursor + decorations.
3. Text pass — glyphon batched glyphs.
4. Post-process pass — optional colorblind LUT blit to swapchain.

This works, but the topology is hardcoded. Adding effects (CRT
curvature, scanlines, bloom on bell, particle bursts) means
*editing the render function*. Operators can't compose effects
declaratively. Every new effect is a code change.

**Engawa is the typed IR that lifts the topology into data.**
Operators declare effects in YAML / tatara-lisp; engawa compiles
to a `CompiledGraph` of typed nodes; mado walks the execution
order and dispatches against wgpu.

## II. Why not bevy?

Bevy's `bevy_render` is the reference for what "good" looks like.
The temptation is to depend on it directly. The temptation is wrong:

| Bevy feature | Why it doesn't fit |
|---|---|
| ECS world (entities, components, systems) | Mado has its own cell grid + cursor model; ECS overhead per cell is wrong. |
| Render-every-vsync game loop | Mado is idle 95% of the time and has a damage gate (now removed for correctness, but the cost model is still "render only when something changed"). |
| Asset pipeline | Shikumi's notify watcher already handles hot-reload fleet-wide. |
| 3D camera + lighting + PBR | Effects target a 2D viewport; the math collapses. |
| `bevy_pbr`, `bevy_animation`, etc. | Compile-time + binary-size cost for code mado will never run. |

Pulling bevy in wholesale would:

1. Invalidate mado's L1/L2/L3 verification ladder (built on garasu's
   deterministic headless harness, not bevy's render path).
2. Add ~50 MB of compiled code + significant build time.
3. Force mado into bevy's plugin model + render-graph idioms, even
   for the cell hot path that's fine where it is.

The right move is what every other pleme-io substrate primitive
does: **distill the design lesson, own the typed surface, depend
only on what fits.**

## III. The typed IR

Eight types, all `Serialize + Deserialize`, all pure data:

```text
RenderGraph
├── resources : BTreeMap<ResourceId, ResourceKind>
├── inputs    : BTreeSet<ResourceId>      // consumer-provided
├── outputs   : BTreeSet<ResourceId>      // consumer reads these
└── nodes     : Vec<Node>

Node
├── id        : NodeId
├── pass      : PassKind (Render | Compute | Blit)
├── inputs    : Vec<ResourceId>
├── outputs   : Vec<ResourceId>           // exactly-one-writer enforced
└── material  : Option<Material>

Material
├── name     : String
├── shader   : ShaderSource (Inline | Path)
└── bindings : Vec<UniformBinding>

UniformBinding
├── binding  : u32
├── kind     : BindingKind (Uniform | StorageRead | StorageReadWrite | Texture | Sampler)
└── resource : ResourceId

ResourceKind
├── Texture  { width, height : Option<u32> }
├── Uniform  { size_bytes : u32 }
├── Storage  { size_bytes : u32 }
├── Sampler
└── External  // consumer-owned handle (e.g. swapchain surface)

Effect  // operator-facing layer
├── name     : String
├── enabled  : bool
├── priority : u16
└── material : Material

PassKind  // enum: Render | Compute | Blit
```

## IV. The compile algorithm

`RenderGraph::compile()` runs five validations + a topo sort:

1. **Duplicate node ids** — every node id is unique.
2. **Multiple writers** — every resource has at most one producer
   node. Pleme-io's "solve once, in one place" rule applied to GPU
   resources.
3. **Unbound inputs** — every node input is either a graph input
   OR another node's output.
4. **Unbound outputs** — every declared graph output is produced.
5. **Cycle detection** — Kahn's algorithm with a BTreeSet ready-pool
   for deterministic ordering. If the order doesn't drain to the
   full node count, the residual nodes are reported as the cycle.

The ready-pool is `BTreeSet<NodeId>` (not `Vec`) specifically so
the execution order is byte-deterministic across runs — same input
graph, byte-identical compiled execution order. This is the same
discipline mado's L1/L2/L3 verification ladder demands.

## V. The 4-phase roadmap

| Phase | Scope | Status |
|---|---|---|
| **v0.1** | Pure-data IR + topo-sort + 20 tests | **shipped** |
| v0.2 | Consumer dispatcher (engawa + garasu + wgpu → frame) | next |
| v0.3 | tatara-lisp `(defeffect …) (defmaterial …) (defgraph …)` forms | follows v0.2 |
| v0.4 | Built-in effect catalog (CRT, scanlines, bloom, blur, colorblind LUTs) | follows v0.3 |

### v0.2 — dispatcher

A new `engawa::dispatch` module that takes:

- `&CompiledGraph` — the validated topology
- `&garasu::GpuContext` — device + queue
- A `ResourceBindings` map — operator-supplied wgpu handles for
  each declared `ResourceId`

…and walks `execution_order`, creating bind groups + pipelines +
encoder dispatches per node. Mado adopts: its hand-rolled 4-pass
sequence becomes a graph. The L1/L2/L3 verification ladder
continues to bound the pipeline; nothing about rendering correctness
changes, only the *authorship* of the topology.

### v0.3 — tatara-lisp authoring

```lisp
(defmaterial scanlines
  :shader (path "/etc/mado/effects/scanlines.wgsl")
  :bindings
    ((0 uniform u_resolution)
     (1 texture scene)
     (2 sampler scene-sampler)))

(defeffect scanlines-effect
  :material scanlines
  :priority 500
  :enabled true)

(defgraph mado-default-pipeline
  :resources
    ((swap     external)
     (scene    (texture :width 800 :height 600))
     (post     (texture :width 800 :height 600)))
  :inputs (swap)
  :outputs (post)
  :nodes
    ((clear-scene  clear :outputs (scene))
     (scanlines-pass fullscreen-effect
       :material scanlines
       :inputs (scene)
       :outputs (post))))
```

The lisp compiles to engawa IR; shikumi watches the file; the
graph hot-reloads. Operators author effects without touching Rust.

### v0.4 — catalog

A small fleet of built-in effects shipped as `.wgsl` + tatara-lisp
declarations:

- **CRT** — Barrel distortion + chromatic aberration + scanlines.
- **Bloom** — Gaussian-blur threshold pass + additive blend.
- **Scanlines** — Single-pass horizontal stripe overlay.
- **Glow on bell** — Brief radial glow centered on the bell pane.
- **Colorblind LUTs** — Already exists in mado's post pipeline;
  migrated into the catalog.
- **Gamma / contrast** — Operator-tunable per-pane uniform.

Each ships under MIT in a dedicated `engawa-catalog` crate.

## VI. Cross-tool composition

Once engawa is the IR every GPU consumer speaks, fleet-wide
composition becomes possible:

- **mado** declares its base graph (cells → text → cursor → post).
- **ayatsuri** (status bar) declares its overlay graph (chrome
  layer at priority 800+).
- Both compile to engawa IR; a unified compositor at the
  swapchain level composes both into one frame. The status bar
  becomes a typed effect, not a hand-rolled overlay.

Same pattern applies for any future surface tool — namimado's
browser chrome, hibikine's video overlay, etc.

## VII. Verification ladder

Engawa inherits the pleme-io discipline:

- **L1 (always-on, 20 tests)** — graph topology invariants: every
  validation-error variant has a test, topo-sort determinism is
  pinned, serde round-trip catches schema drift.
- **L2 (planned, gpu_tests)** — once v0.2 ships, run each built-in
  effect through garasu's `HeadlessHarness` + `frame_hash`; assert
  the output is pixel-deterministic across runs.
- **L3 (planned, gpu_tests)** — golden-hash per effect; intentional
  visual changes accept via the `MADO_GOLDEN_UPDATE=1` auto-recorder
  pattern already shipped for mado scenarios.

## VIII. The why

Three operator-facing wins:

1. **Effects become data.** Operators add a CRT or bloom by editing
   YAML / lisp, not Rust. Hot-reload via shikumi means save → live.
2. **Composition is mechanical.** Multiple effects + their order +
   their bindings compose deterministically; no operator-side
   topology bugs (engawa catches them at compile).
3. **Cross-consumer integration.** Mado + ayatsuri (and any future
   surface tool) speak one IR; the compositor at the swapchain
   level becomes a typed graph operation, not a coordination
   exercise.

Engawa is the renderer's typed surface. Everything visual mado
ships from v0.2 onward composes through it.
