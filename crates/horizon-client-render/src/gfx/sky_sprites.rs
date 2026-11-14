//! Client mods' sky sprites (capability `atmosphere`, API level 33): textured
//! quads on the sky dome, drawn after the sky and stars and before clouds and
//! terrain, with no depth. The game places them each frame
//! (`horizon_mods::atmosphere_sky`); here their textures go onto one page of
//! their own, loaded from the resources on first use (or made, for Horizon's
//! `horizon:sky/*`), and the quads are drawn alpha blended, then adding light.
//! The frame's alpha (HDR output's mark) is left as the sky wrote it.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The texture page's side, in texels.
pub const PAGE: u32 = 1024;
/// The largest side of one sky texture.
pub const TEXTURE_MAX: u32 = 512;
/// Quads drawn in one frame, over every mod.
pub const MAX_QUADS: usize = 1024;
/// Textures read from the resources in one frame (the rest wait a frame).
const LOADS_PER_FRAME: usize = 4;

/// One quad as the game placed it: camera-relative corners (bottom left,
/// bottom right, top right, top left) for the texture's part `uv`
/// (u0, v0, u1, v1), and the colour its texels are multiplied by.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub texture: String,
    pub uv: [f32; 4],
    pub corners: [[f32; 3]; 4],
    pub rgba: [f32; 4],
    pub emissive: bool,
}

/// This frame's sky sprites and the resource root their textures come from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input<'a> {
    pub root: Option<&'a Path>,
    pub sprites: &'a [Sprite],
}

/// Shelves of texture rects on the page, first fit; never freed (a page
/// lives as long as its renderer, which a resource reload replaces).
#[derive(Debug, Default)]
pub struct Packer {
    /// Each shelf's top, height and the x where its free part starts.
    shelves: Vec<(u32, u32, u32)>,
}

impl Packer {
    /// Room for a `width` by `height` rect: its corner, or none when the page is full.
    pub fn alloc(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > PAGE || height > PAGE {
            return None;
        }
        for shelf in &mut self.shelves {
            if height <= shelf.1 && shelf.2 + width <= PAGE {
                let x = shelf.2;
                shelf.2 += width;
                return Some((x, shelf.0));
            }
        }
        let top = self.shelves.last().map_or(0, |s| s.0 + s.1);
        if top + height > PAGE {
            return None;
        }
        self.shelves.push((top, height, width));
        Some((0, top))
    }
}

/// Horizon's own sky textures (`horizon:sky/*`): white texels whose alpha is
/// the shape, so a sprite's tint colours them.
pub fn built_in(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, shape): (u32, u32, fn(f32, f32) -> f32) = match name {
        // A soft round glow: Gaussian falloff to nothing at the rim.
        "horizon:sky/glow" => (64, 64, |x, y| {
            let r2 = x * x + y * y;
            ((-4.0 * r2).exp() - (-4.0f32).exp()).max(0.0) / (1.0 - (-4.0f32).exp())
        }),
        // A disc with a one-texel soft edge.
        "horizon:sky/disc" => (64, 64, |x, y| {
            ((1.0 - (x * x + y * y).sqrt()) * 32.0).clamp(0.0, 1.0)
        }),
        // A streak: brightest at its right end, a tail fading to the left, thin across.
        "horizon:sky/streak" => (128, 16, |x, y| {
            let along = ((x + 1.0) * 0.5).powf(2.5);
            let head = (-((x - 0.9) * 10.0).powi(2)).exp() * 0.6;
            let across = (-(y * y) * 6.0).exp();
            ((along + head).min(1.0) * across).clamp(0.0, 1.0)
        }),
        // A curtain: bright along its bottom edge, fading upwards and towards its sides.
        "horizon:sky/veil" => (64, 128, |x, y| {
            let up = (y + 1.0) * 0.5; // 0 at the bottom edge, 1 at the top
            let rise = (up * 25.0).min(1.0);
            let fall = (1.0 - up).powf(2.2);
            let sides = (1.0 - x * x).max(0.0).sqrt();
            (rise * fall * sides).clamp(0.0, 1.0)
        }),
        _ => return None,
    };
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h {
        for i in 0..w {
            // Texel centres in -1..1, y up.
            let x = (i as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let y = 1.0 - (j as f32 + 0.5) / h as f32 * 2.0;
            let a = (shape(x, y) * 255.0).round() as u8;
            pixels.extend([255, 255, 255, a]);
        }
    }
    Some((w, h, pixels))
}

/// A sky texture's texels: Horizon's own, or the PNG under the root.
fn load(root: Option<&Path>, name: &str) -> Result<(u32, u32, Vec<u8>), String> {
    if let Some(made) = built_in(name) {
        return Ok(made);
    }
    let path = resource_path(name).ok_or("not <namespace>:<path>")?;
    let root = root.ok_or("no resources loaded")?;
    let (w, h, pixels) = crate::sprites::read_rgba(&root.join(path)).map_err(|e| e.to_string())?;
    if w > TEXTURE_MAX || h > TEXTURE_MAX {
        return Err(format!(
            "{w}x{h} is larger than {TEXTURE_MAX}x{TEXTURE_MAX}"
        ));
    }
    Ok((w, h, pixels))
}

/// `assets/<ns>/textures/<path>.png` (the name was checked by the mod runtime).
fn resource_path(name: &str) -> Option<String> {
    let (ns, path) = name.split_once(':')?;
    (!ns.is_empty() && !path.is_empty() && !path.contains("..") && !name.contains('\\'))
        .then(|| format!("assets/{ns}/textures/{path}.png"))
}

/// One vertex: camera-relative position, page UV and colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// The triangles of one sprite whose texture sits at `rect` (x, y, width,
/// height texels) on the page: the texture's part mapped inside the rect,
/// half a texel in from its edges so filtering never reads a neighbour.
pub fn quad(sprite: &Sprite, rect: [u32; 4]) -> [Vertex; 6] {
    let [x, y, w, h] = rect.map(|v| v as f32);
    let inset = |t: f32, size: f32| t.clamp(0.5 / size, 1.0 - 0.5 / size);
    let u = |t: f32| (x + inset(t, w) * w) / PAGE as f32;
    let v = |t: f32| (y + inset(t, h) * h) / PAGE as f32;
    let [u0, v0, u1, v1] = sprite.uv;
    let uvs = [
        [u(u0), v(v1)],
        [u(u1), v(v1)],
        [u(u1), v(v0)],
        [u(u0), v(v0)],
    ];
    let vertex = |i: usize| Vertex {
        position: sprite.corners[i],
        uv: uvs[i],
        color: sprite.rgba,
    };
    [
        vertex(0),
        vertex(1),
        vertex(2),
        vertex(0),
        vertex(2),
        vertex(3),
    ]
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    clip_from_world: [[f32; 4]; 4],
}

struct Page {
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
}

pub(super) struct SkySprites {
    translucent: wgpu::RenderPipeline,
    emissive: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    vertices: Option<wgpu::Buffer>,
    page: Option<Page>,
    packer: Packer,
    /// Every texture asked for: its rect on the page, or none when it could not be had.
    rects: HashMap<String, Option<[u32; 4]>>,
    root: Option<PathBuf>,
    /// Vertices of the alpha-blended quads, then of the light-adding ones.
    counts: (u32, u32),
    warned: HashSet<String>,
}

impl SkySprites {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sky sprites transform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sky sprites bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sky sprites sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky sprites shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky_sprites.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sky sprites layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
        // The frame's alpha stays the sky's: HDR output reads it as a mark (hdr.wgsl).
        let keep_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = |entry: &str, color: wgpu::BlendComponent| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Sky sprites"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_sky_sprite"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState {
                            color,
                            alpha: keep_alpha,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let translucent = pipeline(
            "fs_sky_sprite_translucent",
            wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        );
        let emissive = pipeline(
            "fs_sky_sprite_emissive",
            wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        );
        Self {
            translucent,
            emissive,
            layout,
            sampler,
            uniform,
            vertices: None,
            page: None,
            packer: Packer::default(),
            rects: HashMap::new(),
            root: None,
            counts: (0, 0),
            warned: HashSet::new(),
        }
    }

    /// Quads drawn last frame, and textures on the page.
    pub(super) fn stats(&self) -> (usize, usize) {
        let (a, b) = self.counts;
        (
            (a + b) as usize / 6,
            self.rects.values().filter(|r| r.is_some()).count(),
        )
    }

    fn page(&mut self, device: &wgpu::Device) -> &Page {
        self.page.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Sky sprites page"),
                size: wgpu::Extent3d {
                    width: PAGE,
                    height: PAGE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Sky sprites resources"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            Page { texture, group }
        })
    }

    /// Where `name` is on the page, loading it (at most [`LOADS_PER_FRAME`]
    /// a frame); none while it waits or when it cannot be had (warned once).
    fn rect(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        root: Option<&Path>,
        name: &str,
        loads: &mut usize,
    ) -> Option<[u32; 4]> {
        if let Some(rect) = self.rects.get(name) {
            return *rect;
        }
        if *loads >= LOADS_PER_FRAME {
            return None;
        }
        *loads += 1;
        let placed = load(root, name).and_then(|(w, h, pixels)| {
            let (x, y) = self
                .packer
                .alloc(w, h)
                .ok_or_else(|| format!("the {PAGE}x{PAGE} sky texture page is full"))?;
            Ok(([x, y, w, h], pixels))
        });
        let rect = match placed {
            Ok((rect, pixels)) => {
                let page = self.page(device);
                uploads.texture(
                    encoder,
                    wgpu::TexelCopyTextureInfo {
                        texture: &page.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: rect[0],
                            y: rect[1],
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    rect[2],
                    rect[3],
                    &pixels,
                );
                Some(rect)
            }
            Err(e) => {
                if self.warned.insert(name.to_owned()) {
                    log::warn!("Horizon: sky sprite texture {name} is not drawn: {e}");
                }
                None
            }
        };
        self.rects.insert(name.to_owned(), rect);
        rect
    }

    /// This frame's quads (`clip_from_world`: the view projection without
    /// the camera's translation, as the stars use).
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        input: Input<'_>,
        clip_from_world: glam::Mat4,
    ) {
        self.counts = (0, 0);
        if input.sprites.is_empty() {
            return;
        }
        if input.root != self.root.as_deref() {
            // Another resource root: every texture is read again.
            self.root = input.root.map(Path::to_path_buf);
            self.rects.clear();
            self.packer = Packer::default();
        }
        let mut loads = 0;
        let (mut translucent, mut emissive) = (Vec::new(), Vec::new());
        for sprite in input.sprites.iter().take(MAX_QUADS) {
            let Some(rect) = self.rect(
                device,
                uploads,
                encoder,
                input.root,
                &sprite.texture,
                &mut loads,
            ) else {
                continue;
            };
            let quad = quad(sprite, rect);
            if sprite.emissive {
                emissive.extend(quad);
            } else {
                translucent.extend(quad);
            }
        }
        if translucent.is_empty() && emissive.is_empty() {
            return;
        }
        self.counts = (translucent.len() as u32, emissive.len() as u32);
        translucent.append(&mut emissive);
        let buffer = self.vertices.get_or_insert_with(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Sky sprites vertices"),
                size: (MAX_QUADS * 6 * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        uploads.buffer(encoder, buffer, 0, bytemuck::cast_slice(&translucent));
        let uniform = Uniform {
            clip_from_world: clip_from_world.to_cols_array_2d(),
        };
        uploads.buffer(encoder, &self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Draws what [`SkySprites::prepare`] took; binds its own group 0.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (a, b) = self.counts;
        let (Some(page), Some(vertices)) = (&self.page, &self.vertices) else {
            return;
        };
        if a + b == 0 {
            return;
        }
        pass.set_bind_group(0, &page.group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        if a > 0 {
            pass.set_pipeline(&self.translucent);
            pass.draw(0..a, 0..1);
        }
        if b > 0 {
            pass.set_pipeline(&self.emissive);
            pass.draw(a..a + b, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};

    #[test]
    fn the_packer_fills_shelves_and_refuses_what_does_not_fit() {
        let mut p = Packer::default();
        assert_eq!(p.alloc(512, 64), Some((0, 0)));
        assert_eq!(p.alloc(512, 32), Some((512, 0)));
        assert_eq!(p.alloc(64, 64), Some((0, 64)), "the first shelf is full");
        assert_eq!(
            p.alloc(64, 128),
            Some((0, 128)),
            "too tall for the second shelf"
        );
        assert_eq!(p.alloc(32, 32), Some((64, 64)));
        assert_eq!(p.alloc(0, 4), None);
        assert_eq!(p.alloc(2048, 4), None);
        let mut full = Packer::default();
        assert!((0..2).all(|_| full.alloc(1024, 512).is_some()));
        assert_eq!(full.alloc(1, 1), None);
    }

    #[test]
    fn built_in_textures_have_their_shapes() {
        let alpha = |name: &str, x: u32, y: u32| {
            let (w, _, px) = built_in(name).unwrap();
            px[((y * w + x) * 4 + 3) as usize]
        };
        let (w, h, px) = built_in("horizon:sky/glow").unwrap();
        assert_eq!((w, h, px.len()), (64, 64, 64 * 64 * 4));
        assert!(px.chunks(4).all(|t| t[..3] == [255, 255, 255]));
        assert!(alpha("horizon:sky/glow", 32, 32) > 250);
        assert_eq!(alpha("horizon:sky/glow", 0, 0), 0);
        assert!(alpha("horizon:sky/glow", 48, 32) < alpha("horizon:sky/glow", 40, 32));
        assert_eq!(alpha("horizon:sky/disc", 32, 32), 255);
        assert_eq!(alpha("horizon:sky/disc", 1, 1), 0);
        // The streak's head is on the right, its tail fades to the left.
        assert!(alpha("horizon:sky/streak", 120, 8) > 200);
        assert!(alpha("horizon:sky/streak", 10, 8) < 10);
        assert!(alpha("horizon:sky/streak", 120, 0) < alpha("horizon:sky/streak", 120, 8));
        // The veil is brightest just above its bottom edge and fades upwards.
        let (vw, vh, _) = built_in("horizon:sky/veil").unwrap();
        let low = alpha("horizon:sky/veil", vw / 2, vh - 8);
        assert!(low > 200, "{low}");
        assert!(alpha("horizon:sky/veil", vw / 2, vh / 2) < low);
        assert!(alpha("horizon:sky/veil", vw / 2, 2) < 10);
        assert!(alpha("horizon:sky/veil", 0, vh - 8) < low / 2);
        assert!(
            alpha("horizon:sky/veil", vw / 2, vh - 1) < low,
            "a soft bottom edge"
        );
        assert_eq!(built_in("horizon:sky/moon"), None);
    }

    #[test]
    fn quads_map_the_texture_part_inside_its_rect() {
        let sprite = Sprite {
            texture: "t".into(),
            uv: [0.0, 0.0, 0.5, 1.0],
            corners: [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            rgba: [1.0, 0.5, 0.0, 0.25],
            emissive: false,
        };
        let q = quad(&sprite, [64, 128, 32, 16]);
        let page = PAGE as f32;
        // Bottom left: u0 (half a texel in), v1 (half a texel up).
        assert_eq!(q[0].uv, [(64.0 + 0.5) / page, (128.0 + 15.5) / page]);
        // Top right: u1 = half the width, v0.
        assert_eq!(q[2].uv, [(64.0 + 16.0) / page, (128.0 + 0.5) / page]);
        assert_eq!(
            (q[0], q[2]),
            (q[3], q[4]),
            "two triangles sharing a diagonal"
        );
        assert!(q.iter().all(|v| v.color == sprite.rgba));
        assert_eq!(q[1].position, [1.0, 0.0, 0.0]);
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    /// A blue sky with alpha 0.5 (HDR output's mark), then sprites drawn over
    /// it as the world pass does: the 64 by 64 frame's RGBA read back.
    fn draw(device: &wgpu::Device, queue: &wgpu::Queue, sprites: &[Sprite]) -> Vec<u8> {
        const SIZE: u32 = 64;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut sky = SkySprites::new(device, format);
        let target = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
                view_formats: &[],
            })
        };
        let color = target(format, wgpu::TextureUsages::COPY_SRC);
        let depth = target(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::empty(),
        );
        // Facing south (+z) with a 90° field of view and a far plane nearer than the dome.
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let projection = Mat4::perspective_rh(90f32.to_radians(), 1.0, 16.0, 0.05);
        let mut uploads = super::super::upload::Uploads::new(device);
        let mut encoder = device.create_command_encoder(&Default::default());
        sky.prepare(
            device,
            &mut uploads,
            &mut encoder,
            Input {
                root: None,
                sprites,
            },
            projection * view,
        );
        uploads.finish(&encoder);
        {
            let color_view = color.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 1.0,
                            a: 0.5,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            sky.draw(&mut pass);
        }
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        read.slice(..).get_mapped_range().expect("mapped").to_vec()
    }

    /// A square sprite straight ahead, `size` degrees wide.
    fn ahead(texture: &str, size: f32, rgba: [f32; 4], emissive: bool) -> Sprite {
        let h = (size * 0.5).to_radians().tan() * 100.0;
        Sprite {
            texture: texture.into(),
            uv: [0.0, 0.0, 1.0, 1.0],
            corners: [
                [h, -h, 100.0],
                [-h, -h, 100.0],
                [-h, h, 100.0],
                [h, h, 100.0],
            ],
            rgba,
            emissive,
        }
    }

    #[test]
    fn sprites_draw_past_the_far_plane_blend_and_keep_the_frames_alpha() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let at = |px: &[u8], x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // A red disc half covering the sky, 45° wide (half the screen).
        let px = draw(
            &device,
            &queue,
            &[ahead("horizon:sky/disc", 45.0, [1.0, 0.0, 0.0, 0.5], false)],
        );
        let centre = at(&px, 32, 32);
        assert!(
            (120..=136).contains(&centre[0]) && (120..=136).contains(&centre[2]),
            "{centre:?}"
        );
        assert_eq!(centre[3], 128, "the sky's alpha is kept");
        assert_eq!(at(&px, 2, 2), [0, 0, 255, 128], "outside the sprite");
        // Its edge: the disc's 45° spans half of the 90° view about the centre.
        let edge = (64.0 * (22.5f32.to_radians().tan() / 45f32.to_radians().tan())) as u32;
        assert_eq!(at(&px, 32 + edge / 2 + 3, 32), [0, 0, 255, 128]);
        // A green glow adding light: the blue stays, green comes in, alpha as before;
        // drawn after the translucent red, whatever their order in the list.
        let px = draw(
            &device,
            &queue,
            &[
                ahead("horizon:sky/glow", 45.0, [0.0, 1.0, 0.0, 1.0], true),
                ahead("horizon:sky/disc", 45.0, [1.0, 0.0, 0.0, 1.0], false),
            ],
        );
        let centre = at(&px, 32, 32);
        assert!(
            centre[0] > 250 && centre[1] > 240 && centre[2] < 5,
            "{centre:?}"
        );
        assert_eq!(centre[3], 128);
        // A sprite behind the camera draws nothing; a missing texture is skipped.
        let mut behind = ahead("horizon:sky/disc", 45.0, [1.0, 0.0, 0.0, 1.0], false);
        for c in &mut behind.corners {
            c[2] = -c[2];
        }
        let px = draw(
            &device,
            &queue,
            &[behind, ahead("minecraft:nothing", 45.0, [1.0; 4], false)],
        );
        assert!(px.chunks(4).all(|t| t == [0, 0, 255, 128]));
    }
}
