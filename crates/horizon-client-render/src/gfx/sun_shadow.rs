//! Built-in sun and moon shadows (Video Settings' Shadows, docs/graphics.md), without a shader
//! pack: one square `Depth16Unorm` map of the solid terrain around the player, seen from the
//! light, drawn again only when the light has moved more than `SUN_STEP` degrees or the player
//! has walked into another `SNAP`-block cell (no more than once every `MIN_FRAMES` frames), or
//! the solid terrain in it changed (once every `TERRAIN_FRAMES` frames at most). The terrain's fragment shaders (terrain.wgsl's
//! `sun_shadow_scale`, visbuffer.wgsl's resolve) read it once per lit pixel with one hardware
//! 2x2 comparison, skip faces turned away from the light, and only scale the sky light level
//! they light the face with: block light (torches, lava) stays as it was.
//!
//! The caster pass is a pass of its own before the world: it draws from the renderer's arena
//! with its own lists, so the camera's draws keep every fast path (flat draws, the compact
//! layout, the visibility buffer, GPU culling), unlike a shader pack's shadow pass, which
//! shares the camera's lists. On High the entities cast too: each frame the cached terrain
//! map is copied into a second map and the entities drawn over it.

use glam::{DMat4, DVec3, Mat4, Vec3};

/// The map's format: 16 bits are enough over the box's depth (`DEPTH` blocks, 1/100 block a
/// step) and halve the bandwidth of `Depth32Float`.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

/// The strength the shadows have unless an atmosphere asks for another
/// (`Renderer::set_shadow_strength`): the share of the sky light a shadow takes away.
pub const DEFAULT_STRENGTH: f32 = 0.5;

/// The cell, in blocks, the map's centre keeps to while the player walks.
const SNAP: f64 = 8.0;
/// How far the light moves, in degrees, before the map is drawn again (about every 1.7 s of
/// the day at 20 ticks a second).
pub const SUN_STEP: f32 = 0.5;
/// The fewest frames between two draws of the map.
pub const MIN_FRAMES: u32 = 4;
/// The fewest frames between two draws for terrain changes alone: chunks streaming in and
/// blocks remeshed around a moving player would otherwise redraw it every `MIN_FRAMES`.
pub const TERRAIN_FRAMES: u32 = 30;
/// The map's near plane, in blocks toward the light from its centre: past the build height
/// from anywhere a player stands, so whatever is overhead casts.
const NEAR: f32 = 448.0;
/// How far below the centre the box reaches past its half size.
const BELOW: f32 = 64.0;

/// The player's Shadows setting as the renderer takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShadowLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl ShadowLevel {
    /// The map's resolution and how far the shadows reach from the player, in blocks.
    pub fn map(self) -> Option<(u32, f32)> {
        match self {
            Self::Off => None,
            Self::Low => Some((768, 32.0)),
            Self::Medium => Some((1024, 48.0)),
            Self::High => Some((2048, 96.0)),
        }
    }
    /// Whether entities cast (High); below it, mobs keep their vanilla blob shadows.
    pub fn entities(self) -> bool {
        self == Self::High
    }
}

/// The half size of the map's square, in blocks: the reach and the cell the centre may be
/// off from the player.
pub fn half_size(reach: f32) -> f32 {
    reach + SNAP as f32
}

/// The map's centre for a camera at `camera`: the nearest corner of the `SNAP` grid.
pub fn centre(camera: DVec3) -> [i64; 3] {
    let c = (camera / SNAP).round() * SNAP;
    [c.x as i64, c.y as i64, c.z as i64]
}

/// The direction toward the light for the sun at `sun_angle` degrees (0 at noon, as
/// `Sky::sun_angle`): the sun while it is above the horizon, else the moon opposite it; and how
/// strongly it casts, fading out as it nears the horizon (shadows long past the map).
pub fn light(sun_angle: f32) -> (Vec3, f32) {
    let a = sun_angle.to_radians();
    let sun = Vec3::new(0.0, a.cos(), -a.sin());
    let dir = if sun.y >= 0.0 { sun } else { -sun };
    let t = ((dir.y - 0.08) / (0.3 - 0.08)).clamp(0.0, 1.0);
    (dir, t * t * (3.0 - 2.0 * t))
}

/// The map's view and projection for positions relative to its centre, the light toward `dir`,
/// over `half` blocks each side.
pub fn light_view_proj(dir: Vec3, half: f32) -> Mat4 {
    let view = Mat4::look_to_rh(dir * NEAR, -dir, Vec3::X);
    let proj = Mat4::orthographic_rh(-half, half, -half, half, 0.0, depth(half));
    proj * view
}

/// The depth the box spans along the light, in blocks.
fn depth(half: f32) -> f32 {
    NEAR + 2.0 * half + BELOW
}

/// The map's matrix for camera-relative positions: `light_view_proj` after the move from the
/// camera to the centre.
pub fn camera_view_proj(light_view_proj: Mat4, camera: DVec3, centre: [i64; 3]) -> Mat4 {
    let offset = camera - DVec3::new(centre[0] as f64, centre[1] as f64, centre[2] as f64);
    (light_view_proj.as_dmat4() * DMat4::from_translation(offset)).as_mat4()
}

/// The map's matrix for a pixel of the world target (its x and y in pixels, its depth): back
/// through the camera's `view_proj` (jittered as drawn) to a camera-relative point, then into
/// the map. Worked out in f64, as the inverse of a projection with a near plane at 0.05 blocks.
pub fn pixel_view_proj(camera_view_proj: Mat4, view_proj: Mat4, size: [u32; 2]) -> Mat4 {
    let [w, h] = size.map(|s| f64::from(s.max(1)));
    // Pixel (x, y) to normalised device coordinates: x 2/w - 1, 1 - y 2/h.
    let pixel = DMat4::from_cols_array(&[
        2.0 / w,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / h,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0,
        1.0,
        0.0,
        1.0,
    ]);
    (camera_view_proj.as_dmat4() * view_proj.as_dmat4().inverse() * pixel).as_mat4()
}

/// The shadow inputs terrain.wgsl reads (`Globals`' `shadow` and `shadow_light`) for a map of
/// `resolution` over `half` blocks each side: the strength now (0: off), the normal offset in
/// blocks, the reach, the depth bias; the direction toward the light and the softening's
/// radius in map coordinates (0: one lookup), `softness` (0..1) times two texels.
pub fn shader_inputs(
    strength: f32,
    softness: f32,
    resolution: u32,
    half: f32,
    reach: f32,
    dir: Vec3,
) -> [[f32; 4]; 2] {
    let texel = 2.0 * half / resolution.max(1) as f32;
    // A step of the 16-bit depth is depth / 65535 blocks: two of them, and 1/50 block.
    let bias = 2.0 / 65535.0 + 0.02 / depth(half);
    [
        [strength, 1.5 * texel, reach, bias],
        [
            dir.x,
            dir.y,
            dir.z,
            softness.clamp(0.0, 1.0) * 2.0 / resolution.max(1) as f32,
        ],
    ]
}

/// What the cached map was drawn for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub centre: [i64; 3],
    pub sun_angle: f32,
    pub revision: u64,
    pub level: ShadowLevel,
}

/// When the map is drawn again.
#[derive(Default, Debug)]
pub struct Policy {
    drawn: Option<Drawn>,
    since: u32,
}

impl Policy {
    /// One frame's decision: `Some` with what to draw the map for when it is due, else `None`
    /// and the cached map stays (`drawn`).
    pub fn frame(&mut self, now: Drawn) -> Option<Drawn> {
        self.since = self.since.saturating_add(1);
        if let Some(d) = &self.drawn {
            let moved = angle_between(d.sun_angle, now.sun_angle) > SUN_STEP
                || d.centre != now.centre
                || d.level != now.level;
            let due = if moved {
                MIN_FRAMES
            } else if d.revision != now.revision {
                TERRAIN_FRAMES
            } else {
                return None;
            };
            if self.since < due {
                return None;
            }
        }
        self.drawn = Some(now);
        self.since = 0;
        self.drawn
    }
    /// What the cached map holds, if anything.
    pub fn drawn(&self) -> Option<Drawn> {
        self.drawn
    }
    /// Forget the map (it was released or its contents lost).
    pub fn reset(&mut self) {
        self.drawn = None;
    }
}

/// The angle between two sun angles in degrees, across the wrap at 360.
fn angle_between(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// A map texture and its view.
struct Map {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

fn map_texture(device: &wgpu::Device, label: &str, size: u32, usage: wgpu::TextureUsages) -> Map {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | usage,
        view_formats: &[],
    });
    Map {
        view: texture.create_view(&Default::default()),
        texture,
    }
}

/// The caster pass's uniform: the map's matrix for centre-relative positions (terrain) and for
/// camera-relative ones (entities).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CasterUniform {
    light_view_proj: [[f32; 4]; 4],
    camera_view_proj: [[f32; 4]; 4],
}

/// The caster pass's pipelines and its group 2.
pub struct Pipelines {
    /// Terrain from vertex buffers (`TerrainVertex`), the origin per instance.
    pub instanced: wgpu::RenderPipeline,
    /// Terrain read from the arena page (`Flat::page_group`, either layout of solid quads).
    pub pulled: Option<wgpu::RenderPipeline>,
    /// The entities' opaque quads (High).
    pub entity: wgpu::RenderPipeline,
    pub group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}

/// The caster pass's shader: terrain.wgsl for its vertex decoding and sprites, then the
/// caster entries.
fn caster_source() -> String {
    include_str!("../terrain.wgsl").to_owned() + include_str!("sun_shadow.wgsl")
}

/// The entity caster's shader (entity.wgsl's group 0 for the atlas).
const ENTITY_SOURCE: &str = "
struct SunCasterPass {
    light_view_proj: mat4x4<f32>,
    camera_view_proj: mat4x4<f32>,
}
@group(2) @binding(0) var<uniform> sun_caster: SunCasterPass;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
struct SunEntityOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
}
@vertex fn vs_sun_entity(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>) -> SunEntityOut {
    let clip = sun_caster.camera_view_proj * vec4(position, 1.0);
    let overlay = layer & 0xffffu;
    // entity.wgsl's unlit overlays (the sky's bodies, clouds, beams, portals, cracks) cast
    // nothing: outside the clip volume.
    let unlit = overlay == 2u || overlay == 4u || overlay == 6u
        || (overlay >= 9u && overlay <= 12u);
    var out: SunEntityOut;
    out.clip = select(vec4(clip.xy, clamp(clip.z, 0.0, 1.0), 1.0), vec4(2.0, 2.0, 2.0, 1.0), unlit);
    out.uv = uv;
    out.alpha = color.a;
    return out;
}
@fragment fn fs_sun_entity(in: SunEntityOut) {
    if textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).a * in.alpha < 0.5 { discard; }
}
";

/// Depth state of every caster pipeline: nearer the light wins, with a slope-scaled bias so
/// the faces that cast do not shadow themselves.
fn caster_depth() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: wgpu::DepthBiasState {
            constant: 2,
            slope_scale: 2.0,
            clamp: 0.0,
        },
    }
}

impl Pipelines {
    /// `terrain`: terrain.wgsl's group 0; `flat`: the flat draws' page group, where the solid
    /// draws read the page (`Flat::draw_layout`); `entities`: entity.wgsl's group 0.
    pub fn new(
        device: &wgpu::Device,
        terrain: &wgpu::BindGroupLayout,
        flat: Option<&wgpu::BindGroupLayout>,
        entities: &wgpu::BindGroupLayout,
        sprites: [f64; 2],
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sun shadow casters"),
            source: wgpu::ShaderSource::Wgsl(caster_source().into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sun shadow caster pass"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<CasterUniform>() as u64),
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sun shadow caster pass"),
            size: size_of::<CasterUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sun shadow caster pass"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = |first: &wgpu::BindGroupLayout, page: Option<&wgpu::BindGroupLayout>| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Sun shadow casters"),
                bind_group_layouts: &[Some(first), page, Some(&uniform_layout)],
                ..Default::default()
            })
        };
        let constants = [("SPRITE_ARRAYS", sprites[0]), ("SPRITE_SPLIT", sprites[1])];
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let instance_attributes = wgpu::vertex_attr_array![5 => Float32x4];
        let instances = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        };
        let vertices = wgpu::VertexBufferLayout {
            array_stride: size_of::<super::TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &super::TERRAIN_ATTRIBUTES,
        };
        let pipeline = |layout: &wgpu::PipelineLayout,
                        vertex: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Sun shadow casters"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    compilation_options: options.clone(),
                    buffers,
                },
                // Both sides: the faces toward the light are drawn, and plants' crossed quads
                // cast from either side.
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(caster_depth()),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_sun_caster"),
                    compilation_options: options.clone(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let instanced = pipeline(
            &layout(terrain, None),
            "vs_sun_caster",
            &[Some(vertices), Some(instances.clone())],
        );
        let pulled = flat.map(|flat| {
            pipeline(
                &layout(terrain, Some(flat)),
                "vs_sun_caster_pulled",
                &[None, Some(instances)],
            )
        });
        let entity_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sun shadow entity casters"),
            source: wgpu::ShaderSource::Wgsl(ENTITY_SOURCE.into()),
        });
        let entity_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Unorm8x4];
        let entity = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sun shadow entity casters"),
            layout: Some(&layout(entities, None)),
            vertex: wgpu::VertexState {
                module: &entity_module,
                entry_point: Some("vs_sun_entity"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<super::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &entity_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(caster_depth()),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &entity_module,
                entry_point: Some("fs_sun_entity"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            instanced,
            pulled,
            entity,
            group,
            uniform,
        }
    }
}

/// The built-in shadows' state: the setting, the strength, the maps and the update policy.
pub struct SunShadow {
    level: ShadowLevel,
    strength: f32,
    /// An atmosphere's `shadow-softness`, 0..1 (Low ignores it).
    softness: f32,
    pub policy: Policy,
    /// The terrain map, cached between draws; on High the entities go over a copy of it
    /// (`with_entities`), which is what the terrain reads.
    terrain: Option<Map>,
    with_entities: Option<Map>,
    /// A 1x1 map for the bindings while there is none (never read: the strength is 0).
    blank: Map,
    pub sampler: wgpu::Sampler,
    pub pipelines: Option<Pipelines>,
    /// The caster draws' origins, relative to the map's centre (`section_instance`).
    pub instances: wgpu::Buffer,
    instance_capacity: u64,
    /// Changes to the terrain within the map's reach since it was drawn
    /// (`note_section`).
    revision: u64,
    /// While shadows are set, what each section's solid quads cast (`cast_hash`): a remesh
    /// that only changed their light leaves the map as it is.
    casts: crate::fast_hash::FxHashMap<(i32, i32, i32), u64>,
    /// Map draws so far.
    pub updates: u64,
    /// Whether the last frame drew with shadows on (the terrain reads the map).
    pub active: bool,
}

impl SunShadow {
    pub fn new(device: &wgpu::Device) -> Self {
        let instance_capacity = 1024;
        Self {
            level: ShadowLevel::Off,
            strength: DEFAULT_STRENGTH,
            softness: 0.0,
            policy: Policy::default(),
            terrain: None,
            with_entities: None,
            blank: map_texture(
                device,
                "Sun shadow map (none)",
                1,
                wgpu::TextureUsages::empty(),
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Sun shadow comparison"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            pipelines: None,
            instances: Self::instance_buffer(device, instance_capacity),
            instance_capacity,
            revision: 0,
            casts: Default::default(),
            updates: 0,
            active: false,
        }
    }

    fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sun shadow caster origins"),
            size: capacity * 16,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Room for `count` caster origins.
    pub fn reserve_instances(&mut self, device: &wgpu::Device, count: u64) {
        if count > self.instance_capacity {
            self.instance_capacity = count.next_power_of_two();
            self.instances = Self::instance_buffer(device, self.instance_capacity);
        }
    }

    pub fn level(&self) -> ShadowLevel {
        self.level
    }
    pub fn set_level(&mut self, level: ShadowLevel) {
        self.level = level;
    }
    pub fn strength(&self) -> f32 {
        self.strength
    }
    /// The softening that applies at this level: none on Low.
    pub fn softness(&self) -> f32 {
        if self.level == ShadowLevel::Low {
            0.0
        } else {
            self.softness
        }
    }
    pub fn set_softness(&mut self, softness: f32) {
        self.softness = if softness.is_finite() {
            softness.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    pub fn set_strength(&mut self, strength: f32) {
        self.strength = if strength.is_finite() {
            strength.clamp(0.0, 1.0)
        } else {
            DEFAULT_STRENGTH
        };
    }

    /// What the section at `key` cast, taken before its old mesh is released for a new one
    /// (`section_meshed`), so the release does not count as a change.
    pub fn take_cast(&mut self, key: (i32, i32, i32)) -> Option<u64> {
        self.casts.remove(&key)
    }
    /// The section at `key`, which cast `old` (`take_cast`), was meshed with these `solid`
    /// vertices: the map is drawn again if what they cast changed.
    pub fn section_meshed(
        &mut self,
        key: (i32, i32, i32),
        old: Option<u64>,
        solid: &[crate::mesher::TerrainVertex],
    ) {
        if self.level == ShadowLevel::Off {
            if !self.casts.is_empty() {
                self.casts = Default::default();
            }
            return;
        }
        let new = (!solid.is_empty()).then(|| cast_hash(solid));
        if old != new {
            self.note_section(key);
        }
        if let Some(h) = new {
            self.casts.insert(key, h);
        }
    }
    /// The section at `key` is gone.
    pub fn section_dropped(&mut self, key: (i32, i32, i32)) {
        if self.casts.remove(&key).is_some() {
            self.note_section(key);
        }
    }

    /// The section at `key` (section coordinates) changed what it casts: the map is drawn
    /// again if its column lies within the map.
    fn note_section(&mut self, key: (i32, i32, i32)) {
        let (Some(d), Some((_, reach))) = (self.policy.drawn(), self.level.map()) else {
            return;
        };
        // Every height: toward the light a section far above can still cast into the map.
        let half = f64::from(half_size(reach)) + 16.0;
        let within = |section: i32, centre: i64| {
            ((f64::from(section) * 16.0 + 8.0) - centre as f64).abs() <= half
        };
        if within(key.0, d.centre[0]) && within(key.2, d.centre[2]) {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Makes (or releases, `None`) the maps for `level`: true when the view the terrain reads
    /// changed and its bind group needs making again.
    pub fn ensure_maps(&mut self, device: &wgpu::Device, level: Option<ShadowLevel>) -> bool {
        let want = level.and_then(|l| l.map().map(|(size, _)| (size, l.entities())));
        let have = self
            .terrain
            .as_ref()
            .map(|m| (m.texture.width(), self.with_entities.is_some()));
        if want == have {
            return false;
        }
        self.policy.reset();
        self.terrain = None;
        self.with_entities = None;
        if let Some((size, entities)) = want {
            let copy = if entities {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            };
            self.terrain = Some(map_texture(
                device,
                "Sun shadow map",
                size,
                wgpu::TextureUsages::RENDER_ATTACHMENT | copy,
            ));
            if entities {
                self.with_entities = Some(map_texture(
                    device,
                    "Sun shadow map with entities",
                    size,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                ));
            }
        }
        true
    }

    /// The map the terrain reads (the blank one without shadows).
    pub fn view(&self) -> &wgpu::TextureView {
        self.with_entities
            .as_ref()
            .or(self.terrain.as_ref())
            .map_or(&self.blank.view, |m| &m.view)
    }

    /// The pass that draws the terrain into the cached map, cleared to "nothing casts".
    pub fn terrain_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Option<wgpu::RenderPass<'e>> {
        let map = self.terrain.as_ref()?;
        Some(depth_pass(
            encoder,
            "Sun shadow terrain",
            &map.view,
            true,
            writes,
        ))
    }

    /// High: the cached map copied into the one the terrain reads, and a pass over it for the
    /// entities.
    pub fn entity_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Option<wgpu::RenderPass<'e>> {
        let (from, to) = (self.terrain.as_ref()?, self.with_entities.as_ref()?);
        encoder.copy_texture_to_texture(
            from.texture.as_image_copy(),
            to.texture.as_image_copy(),
            from.texture.size(),
        );
        Some(depth_pass(
            encoder,
            "Sun shadow entities",
            &to.view,
            false,
            writes,
        ))
    }

    /// Writes the caster pass's matrices.
    pub fn write_uniform(
        &self,
        queue: &wgpu::Queue,
        light_view_proj: Mat4,
        camera_view_proj: Mat4,
    ) {
        if let Some(p) = &self.pipelines {
            let u = CasterUniform {
                light_view_proj: light_view_proj.to_cols_array_2d(),
                camera_view_proj: camera_view_proj.to_cols_array_2d(),
            };
            queue.write_buffer(&p.uniform, 0, bytemuck::bytes_of(&u));
        }
    }
}

/// What a section's solid vertices cast: their positions and sprite layers (cutouts), not
/// their light or colour.
fn cast_hash(solid: &[crate::mesher::TerrainVertex]) -> u64 {
    use std::hash::Hasher;
    let mut h = crate::fast_hash::FxHasher::default();
    for v in solid {
        for p in v.pos {
            h.write_u32(p.to_bits());
        }
        h.write_u16(v.layer);
    }
    h.finish()
}

fn depth_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    clear: bool,
    writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        timestamp_writes: writes,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(1.0)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "sun_shadow_tests.rs"]
mod tests;
