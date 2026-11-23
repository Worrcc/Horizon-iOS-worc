# Apple render-pass audit (performance plan step 3.1)

Investigated 2026-11-09 at `1c6970eeffaa6f630f4751ce8f7cd708e8e202a0`.
This is a source/lifetime audit, not an A16 GPU capture or a measured speedup.
Only this document was added. References below use the line numbers at that revision.
`R/` means `crates/horizon-client-render/src/`; `G/` means
`crates/horizon-client/src/`. All proposals are for a later implementation.

## Frame and defaults actually audited

A visible world, normal first-person play, Metal on an Apple GPU, no shader pack,
no native render hooks, no screenshot/recording, no debug overlay, no open menu,
and no entity with a glowing outline. Weather, entities, particles, clouds and the
block-selection outline are draws inside the world pass, not separate passes.

| Setting or condition | Checked-in default / consequence | Evidence |
| --- | --- | --- |
| Resolution scale | 100%; also audit the requested below-100% case. Let scale be `s`. | `R/graphics.rs:812`; `R/gfx/world_post.rs:267` |
| Upscaling, FXAA, TAA | Native, false, false. A scaled frame uses one bilinear copy, not a temporal upscaler. | `R/graphics.rs:813`; `R/gfx/world_post.rs:559`; `R/world_post.wgsl:20` |
| HDR, frame generation | Both off. No HDR mask/copy/output or generated-frame passes. | `R/graphics.rs:817` |
| Improved transparency / OIT | Off; ordinary transparency remains inside the world pass. | `R/settings.rs:242`; `R/gfx.rs:8358` |
| GPU culling option | False, and its GPU-driven path is not offered on Metal. This is distinct from the older two-phase occlusion test. | `R/graphics.rs:787`; `R/gfx.rs:7854` |
| Two-phase section occlusion | Available by default on hardware, but Metal requires at least 1,600 visible sections and an adaptive profitability/probe gate. Do not assume every world frame takes this branch. | `R/gfx.rs:91`, `R/gfx.rs:4465`, `R/gfx.rs:7824`; `R/gfx/cull.rs:59`, `R/gfx/cull.rs:73` |
| Visibility buffer | Off unless `HORIZON_VISBUFFER=1`; flat drawing alone does not enable it. | `R/gfx/visbuffer.rs:29`; `R/gfx.rs:4385` |
| Sky, fog, weather | Enabled. Sky and fog are world shading, not a post chain. | `R/graphics.rs:803`; `R/gfx.rs:9371` |
| Vignette | Enabled, but its quad can be omitted when it contributes nothing. Other screen effects depend on water/fire/portal/equipment/player state. | `R/settings.rs:239`; `G/game/render_tick.rs:333`; `R/screen_effects.rs:250`, `R/screen_effects.rs:360` |
| Vanilla post chains | The game requests `minecraft:end_of_frame` on every world frame, plus server/spectator effects when applicable. Missing chains return `None`, producing no pass. | `G/game.rs:16797`; `R/post_chain.rs:782`, `R/post_chain.rs:1150` |
| Menu blur | A nonzero setting does not blur ordinary gameplay; an appropriate screen/dialog must be open. | `G/game.rs:16808`; `R/gfx.rs:10990` |

The prepared resources available read-only through the sibling checkout
`../ios-update/.local/horizon-client/26.3/assets/minecraft/post_effect/` contain
`blur`, `creeper`, `entity_outline`, `invert`, and `spider`, but no `end_of_frame.json`.
Thus the baseline above has no executing vanilla post chain. This inventory is
supporting evidence, not a claim about every player's active resource pack. A
resource pack can supply `end_of_frame` even when no shader pack is selected; see
the conditional chain table below. No prepared resources were copied into the diff.

## Attachment identities

`W × H` is the surface's physical drawable size, not UIKit points. `P = W H`.
With TAA off, `w = clamp(round(W s), 1, 8192)` and likewise for `h`; `Q = w h`.
At 75%, `Q ≈ 0.5625 P`. The actual dimensions come from surface configuration,
not the handoff's 430 × 932 point screen description.

| Symbol | Texture / view | Format and size | Creation / lifetime |
| --- | --- | --- | --- |
| F | `target_texture`, `view_target`, normally the acquired surface image | `Bgra8Unorm`, `W × H`, one sample | `R/gfx.rs:1952`, `R/gfx.rs:7175`, `R/gfx.rs:8809`; Metal offers BGRA8 at `third_party/wgpu-hal/src/metal/adapter.rs:433`. Renderer chooses non-sRGB gamma-space rendering. |
| Df | `self.depth`, labelled `Horizon depth` | `Depth32Float`, `W × H`, one sample | `R/gfx.rs:4921`; currently render attachment + texture binding + copy source, reusable across world/hand/debug/HUD. |
| Cw | `world_post.target.color`, labelled `Scaled world` | Same frame format as F, `w × h`, one sample | `R/gfx/world_post.rs:377`; render attachment + texture binding + copy source. Only allocated when scale/effects/forced intermediate require it. |
| Dw | `world_post.target.depth`, labelled `Scaled world depth` | `Depth32Float`, `w × h`, one sample | `R/gfx/world_post.rs:416`; same sampled/copyable usage, used for culling/TAA/packs when active. |
| C, D | `world_view`, `world_depth` aliases | At 100% without effects: F, Df. With scaling: Cw, Dw. | `R/gfx.rs:8917` |

No MSAA colour/depth attachments or resolve targets exist in this baseline:
the textures and pipelines use one sample. FXAA/TAA are separate algorithms, not MSAA.

## Every baseline render pass, in encoding order

`Clear(0)` for depth is the renderer's reversed-depth far plane. A read-only depth
attachment (`depth_ops: None`) has no explicit load/store pair; it is not a discard.
The table lists descriptor semantics, not measured DRAM transfers. Later blending
and depth testing count as consumers even without a texture sample.

| Order / execution condition | File:line; label | Colour attachment, load → store | Depth attachment, load → store | Later consumers / sample or copy |
| --- | --- | --- | --- | --- |
| 1, always | `R/gfx.rs:9350`; `Horizon world` | C (BGRA8, `W × H` or `w × h`), `Clear(fog/sky colour)` → `Store` | D (D32F, matching C), `Clear(0)` → `Discard` normally; `Store` if culling or `keep_depth`/visbuffer needs it | C: later overlays load/blend; Cw: `Horizon post` samples it. D: used throughout this pass. On culling frames it is sampled by the next pass and loaded for phase 2; otherwise the default world contents die here. |
| 2a, only when the occlusion gate is active and test boxes exist | `R/gfx/cull.rs:620`; `Section culling`, called at `R/gfx.rs:9612` (flat alternative `R/gfx.rs:9541`) | None | D, read-only (`depth_ops: None`, no stencil) | The fragment shader samples D and writes result/phase-2 indirect buffers. D must also survive for phase 2. This is a genuine Metal texture-read boundary. |
| 2b, whenever the world takes the culling split, including an empty-test case | `R/gfx.rs:282`; `Horizon world, after culling`, calls at `R/gfx.rs:9615` / `R/gfx.rs:9545` | C, `Load` → `Store` | D, `Load` → `Discard` normally, `Store` if `keep_depth` | Draws phase-2 terrain and the rest of the world. Phase-1 colour/depth cannot be cleared. After this, C has the same consumers as row 1; default D is dead. |
| 3, scale differs from 100% (or FXAA/TAA/forced intermediate, outside defaults) | `R/gfx/world_post.rs:552`; `Horizon post`, called at `R/gfx.rs:10468` | F (BGRA8, `W × H`), `Clear(black)` → `Store` | None | Samples Cw with `fs_copy` at default AA settings; does not read Dw. F is loaded by hand/screen/HUD and presented. Cw's contents are dead after its last sample. |
| 4, hand has quads and was not drawn in the world | `R/gfx.rs:10766`; `First-person hand` | F, `Load` → `Store` | Df (D32F, `W × H`), `Clear(0)` → `Discard` with HDR off | F survives for overlays/presentation. Df's hand contents have no later consumer in the baseline. At 100%, `hand_in_world` already draws the hand after a depth-reset draw inside row 1/2b (`R/gfx.rs:8925`, `R/gfx.rs:10282`); therefore this row is normally absent there. At reduced scale it normally exists and draws the hand at full resolution. |
| 5, `frame.screen` is nonempty | `R/gfx.rs:10906`; `Screen effects` | F, `Load` → `Store` | None | Vignette/tints/other camera overlays blend over the existing frame. Later HUD loads F; nothing here samples F itself. Its shaders sample the atlas. |
| 6, HUD vertices exist | `R/hud.rs:2866`; `HUD`, called at `R/gfx.rs:11025` | F, `Load` → `Store` | Normally none. Each `Blend::Depth(clip)` run instead attaches Df, `Clear(1)` → `Discard`. | F is presented (and used by the timestamp pass below). HUD samples its atlas, not F. `draw_range` emits a pass for each depth run and each contiguous non-depth segment, so this row can repeat. Default hotbar/text/touch controls normally share a non-depth segment. |
| 7, timer available, a query slot free, and timestamps only supported at pass boundaries | `R/gfx/timing.rs:188`; `Frame timestamp`, called at `R/gfx.rs:10519` | F, `Load` → `Store` | None | End timestamp; a 1 × 1 scissored draw has colour writes masked off. F must retain the displayed frame. No texture sampling/copy by this pass. It exists even with the stats UI off: timer creation is unconditional when timestamp queries are supported (`R/gfx.rs:2589`). Actual touched-tile/DRAM cost needs capture. |

The timer also starts a one-workgroup **compute** pass before the world
(`R/gfx/timing.rs:159`). With encoder-level timestamps it uses timestamps without
either marker pass; without timestamp support it emits neither. Texture animation,
skinning/flat-list preparation and uploads can precede row 1, but do not add render
attachment passes in this baseline. The 16 × 16 lightmap is computed on the CPU and
uploaded (`R/gfx.rs:6783`); it is not a hidden lightmap render pass.

Typical counts, with one HUD segment, screen effects and the timer marker present:
100% scale = **4 render passes** (world, screen, HUD, timestamp); below 100% =
**6** (world, post, hand, screen, HUD, timestamp). Subtract absent effects/hand/timer;
add two on an active nonempty Metal occlusion-test frame. Scene content and timer
slots make a single unconditional pass count misleading.

`split_solid` does **not** mean two `begin_render_pass` calls. The opaque and
cutout pipelines draw successively inside the same world render pass, in each
occlusion phase if needed (`R/gfx.rs:4289`, `R/gfx.rs:7845`, `R/gfx.rs:9294`,
`R/gfx.rs:9514`, `R/gfx.rs:9574`). Preserve this Apple hidden-surface-removal
optimization; there is no store/load boundary between those two draws to remove.

## Conditional passes and why they are excluded from the count

These prevent a lifetime optimization from silently breaking a non-baseline mode.
Arbitrary packs define their own pass counts, formats and sizes; there is no fixed
pack pass sequence to add to a default frame.

| Feature / insertion point | Passes and attachment operations | Consumption that prevents transient use |
| --- | --- | --- |
| TAA, before `Horizon post` | `R/gfx/world_post.rs:522`, `Horizon TAA`: history[i], `Rgba16Float` (`HISTORY_FORMAT`), output size from `sizes`, `Clear(black)` → `Store`, no depth attachment. | Samples world colour/depth and previous history; current history is sampled for presentation and a later frame. Both depth and history require backing. |
| Visibility buffer, inside world before entities | Initial sky/world pass stores D. `R/gfx/visbuffer.rs:250`, `Horizon visibility buffer`: IDs (`R32Uint`, world size), first `Clear(0)` → `Store`, phase 2 `Load` → `Store`; D `Load` → `Store`. Optional intervening colour-only-world continuation + Metal culling test/flat compaction, then ID phase 2. `R/gfx/visbuffer.rs:393`, `Horizon visibility buffer resolve`: C `Load` → `Store`, no depth; finally world resumes through `gfx.rs:282`. | Resolve samples IDs, blocks and vertex pages, preserving sky where IDs are empty; following entities/transparency still need D. IDs and D cannot be memoryless across these passes. |
| Glowing entities, after hand and before post chains | `R/gfx/entity_outline.rs:337` helper, sequence at `:276`: `Entity outline` → `Entity outline sobel` → `Entity outline blur across` → `Entity outline blur up` → `Entity outline blit`. First four write alternating outline/swap (`Rgba8Unorm`, `W × H`), `Clear(transparent)` → `Store`; last writes F, `Load` → `Store`. No depth attachments. | Every intermediate is sampled by the next step; final blit blends with F. Silhouettes require alpha tests. |
| Vanilla/resource-pack post chain, after outline and before screen effects | `R/post_chain.rs:1193`: copy F to staging (BGRA8, `W × H`); initial flip staging → work and final flip work → F both use `Post effect pass` at `:1558`, `Load` → `Store`, no depth. Per chain, `Post effect depth clear` (`:1313`) clears fallback D32F to 1 and stores, at each target's size; `Post effect target clear` (`:1332`) clears new/nonpersistent colour targets and stores. Ordered translated (`:1468`) or fallback (`:1558`) `Post effect pass` calls write configured targets at configured sizes, `Load` → `Store`, no depth attachment. | Passes sample main/internal target colour, optional depth or effect images. Internal colour uses the frame format, optional width/height overrides (`:1093`, `:1263`); persistent targets can survive frames. The loaded resource JSON/shaders determine the exact expansion. |
| Menu blur, between two HUD ranges | `R/menu_blur.rs:304`, `Menu blur pass`: **six** passes, alternating horizontal/vertical (`:15`), after copying F → target[0]. Outputs target[1], target[0], target[1], target[0], target[1], F; each BGRA8, `W × H`, `Clear(black)` → `Store`, no depth. Called at `R/gfx.rs:11013`. | Every output is sampled by the next pass. The software-only half-size mode adds a downsample, but is not the iOS Metal path. HUD before and after the copy/blur cannot simply merge. |
| Debug crosshair, after screen effects | `R/gfx.rs:10974`, `3d crosshair`: F `Load` → `Store`; Df `Clear(0)` → `Discard`. | Colour blends into F; crosshair depth is pass-local. Ordinary HUD crosshair is not this pass. |
| Native shader packs | `R/gfx/native_post.rs:1023`: named post passes write chain/named `Rgba16Float` intermediates (world size, or named target's scale × world size at `:815`) or F, `Clear(black)` → `Store`, no depth attachment. Native shadow, optional separately timed sky (`R/gfx.rs:9119`), mirror (`:9210`), surface passes (`:243`), and opaque copies (`:9922`) can surround/split the world. | World depth/colour, native surface, intermediate and history reads/copies make backing necessary. Shadow/mirror depth that is clear/discard is a separate possible transient candidate. None runs without a pack. |
| GLSL-format shader packs | `R/gfx/pack_post.rs:3291`: pack terrain/shadow passes, configured colour buffers and D32F depth, usually `Load` → `Store` after initial clears; `:4665` clears groups, `:4717` world import, `:4840` post draws, `:4901` final output; `:3982` mip draws. Formats/dimensions and histories are pack-defined. The existing `overwrite` path at `:4626` already uses Clear. | Deferred/composite stages, mip generation, depth copies and ping-pong/history textures explicitly read earlier outputs. Do not apply blanket transient/discard/clear substitutions. |
| HDR output | `R/gfx/hdr.rs:420`, `Horizon HDR UI mask`, after copying frame → kept world; `:451`, `Horizon HDR hand mark`: frame `Load` → `Store`, no depth. Additional hand-light marks at `R/gfx.rs:10805` load Df then discard; kept-world screen tints at `:10945`. `R/gfx/hdr.rs:597`, `Horizon HDR`: output `Clear(black)` → `Store`, no depth. Frame/kept world format follows renderer creation (normally `Rgba16Unorm` with Smooth Gradients, otherwise 8-bit); HDR output is `Rgb10a2Unorm` or `Rgba16Float`, full drawable size. | HDR output samples frame/kept world; hand marking needs preserved hand depth. The world copy is an unavoidable boundary for the current algorithm. HDR is off by default. |

OIT has particularly large attachments but is **not enabled by default**. Its exact
ordered expansion, at world size, is (`R/gfx/oit.rs:616`):

| Order | File:line / label | Colour attachments; load → store | Depth; load → store | Subsequent read/copy |
| --- | --- | --- | --- | --- |
| Optional first | `:820`, `OIT water mask` | C, `Load` → `Store` (colour writes disabled by mask pipeline) | terrain-depth copy (D32F), `Load` → `Store`, after copy from D | Terrain stages use the patched depth; copy prevents memoryless. |
| Bounds stage | `:682`, `OIT depth bounds` | bounds (`Rgba32Float`), `Clear(BOUNDS_CLEAR)` → `Store` | terrain depth, read-only | Bounds copied into culled bounds and sampled by the cull. |
| Optional features, each stage | `:695`, same stage label | Bounds, coefficients[0..1], or accumulation, matching stage, `Load` → `Store` | World D, read-only | Same downstream consumers as terrain stage. A boat mask can make this depth different from terrain's. |
| After bounds features | `:864`, `OIT cull depth bounds` | culled (`Rgba32Float`), `Load` → `Store`, after bounds copy | D, `Load` → `Store` | Culled bounds sampled by later stages; D updated and used later. |
| Optional clouds, each stage | `:782`, same stage label | Culled bounds, coefficients[0..1], or accumulation, `Load` → `Store` | Cloud D32F: bounds stage copies D then `Load` → `Store`; later stages read-only | Cloud depth survives multiple stages; colour outputs are sampled downstream. |
| Optional after bounds clouds | `:820`, second `OIT water mask` | As first mask | Recopy D, `Load` → `Store` | Rebuilds terrain mask from depth changed by cull. |
| Transmittance stage | `:682`, `OIT transmittance` | Two `Rgba16Float` coefficients, each `Clear(transparent)` → `Store`, then features/clouds as above | Terrain depth, read-only | Accumulation samples coefficients. |
| Accumulation stage | `:682`, `OIT accumulate` | `Rgba16Float` accumulation, `Clear(transparent)` → `Store`, then features/clouds as above | Terrain depth, read-only | Composite samples accumulation and transmittance. |
| Composite | `:735`, `OIT composite` | C, `Load` → `Store` | D, `Load` → `Store` | Then `R/gfx.rs:282` world continuation (`World after OIT`, call at `:10097`) loads C/D and finishes remaining draws. |

## Candidates, evidence and risks

### A. Memoryless attachments

The public wgpu 30.0.1 name is **`TextureUsages::TRANSIENT_ATTACHMENT`**, not
`TextureUsages::TRANSIENT`. The latter name in the plan refers to internal
`TextureUses::TRANSIENT`. Checked `wgpu-types` 30.0.1 `src/texture.rs:192` in the
local Cargo registry: the public flag must be combined **only** with
`RENDER_ATTACHMENT`, requires clear/don't-care load and discard store, and cannot
coexist with texture binding or copy usages. The locked dependency is recorded at
`Cargo.lock:5401`. `third_party/wgpu-hal/src/metal/device.rs:561` maps the internal
flag to `MTLStorageMode::Memoryless` only when the adapter supports it
(`metal/adapter.rs:1107`).

This matches Apple's [memoryless storage rules](https://developer.apple.com/documentation/metal/mtlstoragemode/memoryless):
the contents are pass-local and cannot be loaded/stored to system memory.

| Candidate | Evidence / safe lifetime | Risk / conservative implementation |
| --- | --- | --- |
| Full-resolution hand depth at reduced scale, debug crosshair depth, depth-clipped HUD runs | Already Clear → Discard (`gfx.rs:10773`, `:10981`; `hud.rs:2873`); subsequent passes either do not read it or clear it again. | Introduce a dedicated attachment-only scratch depth texture, not a flag on today's shared Df. Keep backed Df for world depth readers and HDR; preserve depth clear value, format, size and all HUD run boundaries. Strongest straightforward residency candidate. |
| World depth on frames with no external reader and no world-pass split | Default no-cull world is Clear → Discard (`gfx.rs:9357`). At 100% the hand's depth reset/draw stays in that same pass. At reduced scale `fs_copy` reads only colour. | Needs an attachment-only depth variant chosen before encoding. Exclude culling, visbuffer, OIT, TAA, frame generation, material copies, depth-reading native hooks and active depth-reading post chains. Switching modes must recreate/select backed targets before a read. Currently `keep_depth` is not a complete post-chain dependency analysis; do not treat its negation alone as proof. |
| Native timed-sky/mirror scratch depth | `gfx.rs:9133` and `:9228` already Clear → Discard. | Outside default; verify pack/native interfaces and give these separate usage-compatible textures. Do not touch mirror colour, which is sampled. |
| MSAA depth/colour | No candidate in the current frame graph. | A hypothetical future multisample attachment can be transient when only its resolve survives; the resolve texture must remain backed. Do not add MSAA to obtain a memoryless optimization. |
| OIT accumulation, bounds, coefficients; Cw; visbuffer IDs; post/blur/history | **Not candidates.** Every one crosses a sampling/copy/pass boundary in the tables. | Transient contents cannot become inputs to the current next stage. Requires a different tile-local algorithm/API, not a usage-bit change. |

Existing Clear/Discard depth may already avoid its DRAM load/store on Metal.
Memoryless guarantees removal of backing allocation; it does **not** establish an
additional full-depth-buffer bandwidth saving over the existing descriptor.
Benefit must be separated into resident memory, allocation pressure and measured
traffic. Restrict any implementation initially to `target_os = "ios"` plus Metal
and memoryless-capable Apple GPUs, preserving all desktop descriptors and paths.

### B. Load → Clear

No unconditional Load → Clear change is safe in the baseline table. World
continuations, hand, screen effects and HUD preserve earlier colour/depth;
clearing would erase it. World start and spatial upscale already Clear. The
timestamp pass must preserve F even though it writes no colour. Apple's
[load/store guidance](https://developer.apple.com/documentation/metal/setting-load-and-store-actions/)
supports clearing an attachment only when earlier contents are unnecessary.

Outside baseline, the explicit staging/work blits in `post_chain.rs:1199` and
`:1228` use the known full-screen `vs_quad`/`fs_blit` pipeline with no blend,
depth test, discard or partial colour mask (`:746`; `post_chain.wgsl:42`). Their
destination Load can be Clear; expected avoided read is one destination image
per blit. Known complete-overwrite stand-in chain passes can similarly qualify.
Do not generalize to translated/custom shaders or `vs_rotscale`, which can leave
pixels untouched. Preserve persistent targets and configured clear colours;
fuse a target's standalone clear into its first write only after proving that
no earlier pass samples the cleared target. These opportunities are not
intrinsically Metal-only, but should initially use the iOS/Apple gate.

### C. Store → Discard

No unconditional baseline colour Store is redundant: F is presented and Cw is
sampled. A texture becoming dead **after sampling** does not permit discarding
the producer's store. Normal world/hand/HUD depth already discards at its last
write; making their creation memoryless is a separate change.

One conditional opportunity is OIT: composite currently stores D and the
`World after OIT` continuation is called with `keep_depth=true`. Once all
external readers are ruled out, the continuation's final depth store can be
Discard, rather than blindly keeping D. Composite itself must preserve depth
for that continuation (or be merged with it). `gfx.rs:10097`, `oit.rs:735` are
the evidence; shader packs, native hooks, TAA, frame generation and post chains
are the risk. The current baseline never takes this branch.

### D. Consecutive-pass merging

| Candidate | Evidence / saving mechanism | Risk and necessary boundaries |
| --- | --- | --- |
| Scaled post → hand → screen effects → HUD | All write F consecutively; only the first samples Cw. Atlas sampling is independent of F. Reuse one F render pass, clear scratch Df for the hand, then draw compatible overlay/HUD pipelines with depth disabled. | Preserve native world/hand callbacks, screenshots/world-icon capture, outlines/post chains and menu blur as explicit breaks. Reconcile pipeline depth attachment compatibility, viewport/scissor, bind groups and per-pass timing. Depth-clipped HUD runs need their own clear/reset handling. |
| At 100%, world (including hand) → screen effects → HUD | World and hand already share a pass; screen/HUD can append without sampling C/D in the baseline. | Same breaks; ensure pipelines are compatible with the attached D32F and do not test/write world/hand depth. Preserve draw order and gamma-space attachment format. |
| Screen effects → ordinary HUD alone | Easiest first slice: both have only F; no intermediate dependency on F. | A vanilla post chain/outline before screen effects is fine; a menu blur copy between HUD ranges is a break. Keep clipped-HUD splits initially. |
| Timer marker → preceding final pass | Attach the end query to the real final render pass, preserving its ordering, rather than opening a marker encoder just to depend on F. | Metal requires actual GPU work to get a timestamp. Maintain the eight-slot lifecycle, delayed resolve, and correct frame endpoints on every last-pass variant; do not replace with an independent compute marker (existing comments explain it can overlap). |
| OIT terrain → features within each stage, **without boat masks** | Same colour targets and same world depth in that case (`oit.rs:682`, `:695`); append the features closure to the terrain pass. | With boat masks the terrain depth differs from feature depth; cannot merge without preserving semantics. Clouds have a distinct depth texture. Bounds-cull/coefficients/accumulate/composite have true read-after-write dependencies. Off by default. |
| OIT composite → world continuation | Same C/D, no intervening sampling needed by ordinary trailing draws. | Need compatible pipelines, keep HDR marking and exact see-through/outline/hand order. OIT stages before composite remain separate. |

Do **not** merge through Metal occlusion testing: phase 1's depth is sampled and
the test writes indirect commands used in phase 2. Likewise post ping-pong,
blur, native opaque copies and HDR kept-world copies are real dependencies.
Sky/opaque/cutout/transparency already share a pass in the default path.
Merging is broadly useful on tiled GPUs, but the conservative implementation
gate is iOS + Apple Metal; existing `split_solid` applies to Metal generally,
including macOS, and should remain unchanged.

## f16 WGSL: useful work and actual compiler support

**Yes: vendored naga 30.0.1 supports `enable f16;` and MSL half output.**
Evidence is local to the locked compiler, not a claim about a newer release:

- `third_party/naga/Cargo.toml:16` pins 30.0.1; MIT/Apache-2.0 licensing was checked.
- `src/front/wgsl/parse/directive/enable_extension.rs:153` maps `f16` to an
  implemented extension; `:207` describes f16/half in all shader languages.
- `src/valid/type.rs:303` accepts two-byte floats with
  `Capabilities::SHADER_FLOAT16`.
- `src/back/msl/writer.rs:567` maps float width 2 to MSL `half`; vector/matrix
  emission builds on this scalar mapping. The writer also emits f16 literals.
- `third_party/wgpu-hal/src/metal/adapter.rs:1193` advertises
  `Features::SHADER_F16`; renderer device feature selection at `R/gfx.rs:2191`
  currently does **not** request it. Merely changing WGSL is insufficient.

| Priority / shader | Maths to consider | Keep f32 / risk |
| --- | --- | --- |
| 1: `terrain.wgsl:618`, `:637`, `:657`, `:714` (`fs_opaque`, solid/cutout, translucent) | After texture sampling and alpha decisions: RGB tint/light multiplication, emissive overlay colour mix, final fog-colour mix. This runs across most world pixels and translucent overdraw. | Keep UVs/gradients, packed vertex decoding, position/projection/depth, fade/dither and alpha-test comparisons f32/integer. Compute fog distance/factor in f32, cast the bounded factor for the final mix. `f32::MAX` environmental fog sent by `gfx.rs:7301` overflows half; a wholesale cast of fog uniforms can produce infinities/NaNs. Preserve no-discard opaque pipeline. |
| 2: `entity.wgsl:281`, `:262`, `:580` (`entity_color`, `glint_over`, weather) | RGB texture × tint × light, hurt overlay, glint and final fog mix; same machinery shades the hand, clouds and particles. | Keep atlas UVs, alpha/discard/dissolve decisions, depth, long-distance fog and portal projection f32. Much lightmap work already happens in CPU/vertex code, so do not count it as fragment ALU saved. Portal layer colour summation may tolerate half, but accumulation and highlights need visual checks. |
| 3: `terrain.wgsl:759`, `:843`, `:855` (sky), screen/vignette/HUD colour | Bounded colour interpolation after ray/fog-factor calculation; large screen coverage. | Sky gradients and dark vignette are sensitive to banding. Ray construction/trigonometry and texture coordinates should stay f32. |
| 4: `world_post.wgsl:30`, `:97`; `menu_blur.wgsl`; outline blur | Optional FXAA luma/RGB blending, blur colour sums, TAA colour neighbourhood bounds. | These are off/absent in normal play. Default `fs_copy` is just a texture fetch: little ALU to halve. Keep texel coordinates, TAA reprojection/depth/history-validity and edge decisions f32; watch long blur sums and temporal instability. |
| Defer: `oit.wgsl:30`, `:67`, `:92` | Possibly the final bounded colour work only. | OIT uses deep logarithmic/exponential absorption and coefficients; its bounds clear is `-f32::MAX`. Half conversion is high risk and its `Rgba16Float` attachments already have 16-bit storage. HDR PQ/luminance work also needs a separate range/precision audit. |

Request F16 only when the iOS Apple Metal adapter supports it, compile a separate
shader-source variant with `enable f16;` at module scope, and leave desktop f32
modules/feature requests untouched. WGSL has no C preprocessor: a shader
"define" needs source selection/generation; an override boolean alone cannot
conditionally enable a type extension. Initially keep uniform layouts and shader
interfaces f32 and cast local colour values, avoiding host-layout and pack-prefix
changes. Naga explicitly rejects derivatives on f16 operands
(`third_party/naga/src/valid/expression.rs:1183`), so `dpdx`/`dpdy` paths must stay
f32. Texture sample coordinates/results need conversions at the right boundary.

Expected first benefit is lower register/ALU pressure, potentially occupancy and
energy, not a guaranteed attachment-bandwidth reduction: f16 locals do not turn
BGRA8 or D32F into smaller textures. Validate generated MSL and profile rather
than equating f16 arithmetic with halved frame bandwidth.

## Top five follow-up changes, ranked for the requested workload

These are ranked by plausible bandwidth benefit **in normal/default play**, with
explicit conditional gains. There are not five independent unconditional
load/store fixes in this renderer. The estimates are uncompressed logical
image-volume bounds, not expected physical DRAM bytes: compression, caches,
scissors, Metal scheduling and load/store elision can reduce them. One BGRA8
store + reload is `8P` bytes (or `8Q` at world resolution). At an illustrative
1920 × 1080, `8P = 16.6 MB/frame`, about 1.0 GB/s at 60 fps.

| Rank | Change sketch | Estimated saving and applicability |
| --- | --- | --- |
| 1 | Merge the presentation tail. Start with screen effects + normal HUD; extend to full-resolution hand after the scaled post, and to world + overlays at 100%, guarded against the dependency breaks listed above. | Each eliminated full-size colour boundary saves up to `8P`; three removed boundaries in scaled post → hand → screen → HUD give up to `24P` (~49.8 MB/frame at 1080p). At 100%, world → screen → HUD has up to `16P`. These are alternatives, not additive. Highest broadly applicable candidate. |
| 2 | Revisit the Metal two-phase occlusion profitability gate using device measurements; skip the test/split on frames where it loses, drawing all terrain in the existing single world pass. Retain the current conservative 1,600-section/adaptive gate until evidence justifies a change. | On an affected frame, removes a C store/reload (`8Q`) plus intermediate D store/reload (`8Q`) and the test's depth sampling. At 75%, those logical image volumes total about `9P`. Often **zero** gain at ordinary distances because the existing gate already skips the split. Additional vertex/shading work may cost more than saved bandwidth. |
| 3 | Put the GPU end timestamp on the final real pass instead of a separate `Frame timestamp` pass. | Removes one preservation-only boundary, logically up to `8P`, but actual benefit may be small/zero because it scissors one pixel and masks all colour writes. Measure this before assigning it full-frame traffic. Preserve accurate timing so subsequent comparisons remain useful. |
| 4 | Clear known overwrite destinations in vanilla post-chain flip/blit passes and fold eligible standalone target clears into first writes. | `4P` per full-size destination Load removed; up to `8P` for the two bridge blits when a chain runs, plus eligible internal targets. **Zero in the audited default resource set**, since no chain executes. No claim that custom passes are overwrite-safe. |
| 5 | Give pass-local hand/HUD/debug depth and eligible unsplit-world depth attachment-only transient variants on iOS Apple Metal. | Removes up to `4P` full-resolution scratch-depth backing, plus `4Q` world-depth backing at reduced scale, subject to reuse/aliasing. Incremental DRAM bandwidth over existing Clear/Discard may be **zero**. Strong residency/pressure candidate, but below proven colour-boundary removal in a bandwidth ranking. |

If improved transparency is enabled later, merging each terrain/features pair
with identical depth has a larger conditional logical saving: bounds `32Q`,
two coefficients `32Q`, accumulation `16Q`, totalling up to **`80Q`** per frame
where all three feature passes run. Composite + continuation can remove another
`8Q` colour and potentially depth boundary. This is outside the default-workload
ranking; OIT remains off and must not be disabled or altered to obtain an audit gain.

## Checks, deviations and real-device work

- `CARGO_TARGET_DIR=$HOME/Projects/iaf-swarm/ios-update/target cargo fmt --all -- --check`:
  **passed**, exit 0, no output.
- Clippy and `ios/scripts/check-ios.sh`: skipped as explicitly requested for this
  investigation; the iOS script was read. No touched Rust crate, so no crate tests
  or build were run. No game was started. No build failures to paste.
- Reviewed descriptor creation and each relevant producer/consumer path. Final
  working-tree status shows only `?? ios/APPLE-PASS-AUDIT.md`; tracked-file diff
  is empty. No commits, pushes or Git history changes.

Deviations from the plan/assumptions:

1. Deliverable is this separate file, as requested, rather than adding a table to
   `APPLE-PERF-PLAN.md`. Source and plan are unchanged.
2. Use the actual public `TRANSIENT_ATTACHMENT` name in a later implementation;
   the plan's `TRANSIENT` spelling is the internal usage name.
3. Do not assume OIT accumulation or shared world depth can be memoryless. The
   former is sampled; the latter sometimes crosses a culling/copy/effect boundary.
   Choose separate scratch/eligible target variants, with backed fallback.
4. `split_solid` separates pipeline draws, not render-pass encoders. Sky/terrain
   merging and many final depth discards already exist.
5. The checked-in default scale is still 100%, not step 1's proposed 75%. Both
   paths are audited. No assumption is made about another worker's pending changes.
6. A no-shader-pack world still requests a resource-defined end-of-frame chain.
   Its absence was checked against available prepared resources; the player's
   active resources, settings, callbacks and timestamp capability still need a capture.

On a real iPhone, capture representative 100% and 75% frames with Xcode's Metal
tools: verify pass/texture counts, pixel dimensions, store actions, memoryless
storage and actual external read/write bytes. Include open terrain and caves
(occlusion gate off/on), foliage/cutouts/fading chunks, water/boats/clouds/rain,
first/third person, clipped HUD, glowing entities, menus, and resource post effects.
For later changes compare screenshots for draw order, HUD depth/scissor, alpha
edges and fog/sky/night gradients; test resize/orientation and turning on TAA,
OIT, HDR or a depth-reading pack after scratch targets exist. Verify timestamp
values remain credible, then compare GPU ms, memory, heat and frame pacing over
matched sustained runs using step 0's `perf:` log. This audit alone establishes
neither speedup nor visual equivalence for any proposed implementation.
