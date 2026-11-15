//! LDR dual-filter bloom, after the world pass. Textures exist only while active.
use super::{
    pack_timing::PassTiming,
    timing::{GpuTimer, Scope},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BloomLevel {
    #[default]
    Off,
    Low,
    High,
}
impl BloomLevel {
    pub fn intensity(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Low => 0.55,
            Self::High => 0.8,
        }
    }
    fn levels(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Low => 3,
            Self::High => 4,
        }
    }
}

/// An atmosphere's post keys that ride the bloom (API level 34): the luma threshold on the
/// light sources (0: none) and the composite's exposure and saturation (1: unchanged).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub threshold: f32,
    pub exposure: f32,
    pub saturation: f32,
}
impl Default for Grade {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            exposure: 1.0,
            saturation: 1.0,
        }
    }
}
impl Grade {
    /// Within the API's ranges; what is not finite is unchanged.
    pub fn checked(self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, none: f32| {
            if v.is_finite() { v.clamp(lo, hi) } else { none }
        };
        Self {
            threshold: fix(self.threshold, 0.0, 1.0, 0.0),
            exposure: fix(self.exposure, 0.25, 4.0, 1.0),
            saturation: fix(self.saturation, 0.0, 2.0, 1.0),
        }
    }
}

struct Chain {
    size: [u32; 2],
    level: BloomLevel,
    views: Vec<wgpu::TextureView>,
    down: Vec<wgpu::BindGroup>,
    up: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
}

pub struct Bloom {
    layout: wgpu::BindGroupLayout,
    pub composite_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    threshold: wgpu::Buffer,
    intensity: wgpu::Buffer,
    down0: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    chain: Option<Chain>,
}
impl Bloom {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom filter"),
            entries: &[
                texture,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform(2),
            ],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom composite"),
            entries: &[texture, uniform(1)],
        });
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom filter"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = |entry, additive: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Bloom filter"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: additive.then_some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            down0: pipeline("down0", false),
            down: pipeline("down", false),
            up: pipeline("up", true),
            layout,
            composite_layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            threshold: buffer("Bloom threshold"),
            intensity: buffer("Bloom intensity"),
            chain: None,
        }
    }
    pub fn clear(&mut self) {
        self.chain = None;
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        level: BloomLevel,
        world: &wgpu::TextureView,
    ) {
        if level == BloomLevel::Off {
            self.clear();
            return;
        }
        if self
            .chain
            .as_ref()
            .is_some_and(|c| c.size == size && c.level == level)
        {
            return;
        }
        self.clear();
        let views: Vec<_> = (0..level.levels())
            .map(|i| {
                let divisor = 1 << (i + if level == BloomLevel::High { 1 } else { 2 });
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("Bloom level"),
                        size: wgpu::Extent3d {
                            width: (size[0] / divisor).max(1),
                            height: (size[1] / divisor).max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            })
            .collect();
        let bind = |source| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Bloom filter"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.threshold.as_entire_binding(),
                    },
                ],
            })
        };
        let down = std::iter::once(world)
            .chain(views.iter().take(views.len() - 1))
            .map(bind)
            .collect();
        let up = views.iter().skip(1).map(bind).collect();
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.intensity.as_entire_binding(),
                },
            ],
        });
        self.chain = Some(Chain {
            size,
            level,
            views,
            down,
            up,
            composite,
        });
    }
    pub fn run(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        scale: f32,
        grade: Grade,
        mut timing: Option<&mut PassTiming>,
        mut timer: Option<&mut GpuTimer>,
    ) {
        let Some(c) = &self.chain else { return };
        let grade = grade.checked();
        queue.write_buffer(
            &self.threshold,
            0,
            // 0: no luma threshold, the light sources' marks alone decide.
            bytemuck::cast_slice(&[grade.threshold, 0.18, 0.0, 0.0]),
        );
        queue.write_buffer(
            &self.intensity,
            0,
            bytemuck::cast_slice(&[
                c.level.intensity() * scale / c.views.len() as f32,
                grade.exposure,
                grade.saturation,
                0.0,
            ]),
        );
        for (up, i) in (0..c.views.len())
            .map(|i| (false, i))
            .chain((0..c.views.len() - 1).rev().map(|i| (true, i)))
        {
            let name = format!("Bloom {} {i}", if up { "up" } else { "down" });
            let writes = timing
                .as_deref_mut()
                .and_then(|t| t.writes(&name))
                .or_else(|| {
                    timer
                        .as_deref_mut()
                        .and_then(|t| t.scope_writes(Scope::Bloom, !up && i == 0, up && i == 0))
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&name),
                timestamp_writes: writes,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &c.views[i],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if up {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(if up {
                &self.up
            } else if i == 0 {
                &self.down0
            } else {
                &self.down
            });
            pass.set_bind_group(0, if up { &c.up[i] } else { &c.down[i] }, &[]);
            pass.draw(0..3, 0..1);
        }
    }
    pub fn composite(&self) -> Option<&wgpu::BindGroup> {
        self.chain.as_ref().map(|c| &c.composite)
    }
}

#[cfg(test)]
mod tests {
    use super::super::world_post::Post;
    use super::*;
    const SIZE: u32 = 64;

    fn draw(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        post: &mut Post,
        image: &[u8],
        level: BloomLevel,
        scale: f32,
    ) -> Vec<u8> {
        post.set_bloom(level);
        post.bloom_scale = scale;
        post.bloom_on = level != BloomLevel::Off && scale > 0.0;
        let (world, _) = post.prepare(device, [SIZE; 2], true).unwrap();
        let texture = |usage| {
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
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };
        let input = texture(wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING);
        queue.write_texture(
            input.as_image_copy(),
            image,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
            input.size(),
        );
        let output =
            texture(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let source = post.bind(device, &input.create_view(&Default::default()));
        let mut encoder = device.create_command_encoder(&Default::default());
        post.finish_with(&mut encoder, &world, &source, None);
        post.finish(
            queue,
            &mut encoder,
            &output.create_view(&Default::default()),
            None,
            None,
        );
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
            },
            output.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = read.slice(..).get_mapped_range().unwrap().to_vec();
        read.unmap();
        bytes
    }

    #[test]
    fn bloom_spreads_marked_light_sources_only_and_off_is_identical() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            eprintln!("Skipping bloom GPU readback: no adapter");
            return;
        };
        eprintln!("Bloom GPU readback adapter: {}", adapter.get_info().name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut post = Post::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let patterned: Vec<u8> = (0..SIZE * SIZE)
            .flat_map(|i| [i as u8, (i / 7) as u8, (i / 17) as u8, (i / 3) as u8])
            .collect();
        assert_eq!(
            draw(&device, &queue, &mut post, &patterned, BloomLevel::Off, 1.0),
            patterned
        );
        assert_eq!(
            draw(
                &device,
                &queue,
                &mut post,
                &patterned,
                BloomLevel::High,
                0.0
            ),
            patterned
        );
        assert!(post.bloom.composite().is_none());
        let at = |x: usize, y: usize| (y * SIZE as usize + x) * 4;
        // A dark world, unmarked (alpha 1), with an `[r, g, b, a]` square at `origin`.
        let world = |origin: usize, size: usize, texel: [u8; 4]| {
            let mut image = [20, 20, 20, 255].repeat((SIZE * SIZE) as usize);
            for y in origin..origin + size {
                for x in origin..origin + size {
                    image[at(x, y)..at(x, y) + 4].copy_from_slice(&texel);
                }
            }
            image
        };
        // White but unmarked: the sky's alpha (1 - 0.24 at full daylight) and a lit block's.
        for alpha in [194, 255] {
            let sky = world(16, 32, [255, 255, 255, alpha]);
            assert_eq!(
                draw(&device, &queue, &mut post, &sky, BloomLevel::High, 1.0),
                sky,
                "an unmarked bright texel bloomed (alpha {alpha})"
            );
        }
        // A light source's texel (terrain.wgsl's bloom mark 0.5) of the same colour as the
        // sky's spreads; the frame's alpha passes through unchanged.
        for (level, origin, spot_size) in [(BloomLevel::High, 31, 2), (BloomLevel::Low, 32, 4)] {
            let image = world(origin, spot_size, [255, 240, 200, 128]);
            let output = draw(&device, &queue, &mut post, &image, level, 1.0);
            for i in (3..image.len()).step_by(4) {
                assert_eq!(output[i], image[i], "world alpha changed");
            }
            for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
                assert_eq!(&output[at(x, y)..at(x, y) + 3], &[20; 3]);
            }
            assert!(output[at(32, 32)] <= image[at(32, 32)]);
            assert!(
                output[at(origin + spot_size, 32)] > 25
                    && (3..=6).any(|d| output[at(origin + spot_size - 1 + d, 32)] > 20),
                "no halo 3-6 pixels from {level:?} spot"
            );
            // Unmarked, the same spot leaves the frame as it was.
            let unmarked = world(origin, spot_size, [255, 240, 200, 255]);
            assert_eq!(
                draw(&device, &queue, &mut post, &unmarked, level, 1.0),
                unmarked
            );
        }
        post.set_bloom(BloomLevel::Off);
        assert!(post.bloom.composite().is_none());

        // An atmosphere's grade (API level 34) rides the composite: exposure scales the frame,
        // saturation 0 greys it, both only while bloom draws.
        let flat = world(0, 0, [0, 0, 0, 0]);
        let colour = world(16, 32, [200, 100, 40, 255]);
        post.bloom_grade = Grade {
            threshold: 0.0,
            exposure: 0.5,
            saturation: 1.0,
        };
        let dark = draw(&device, &queue, &mut post, &colour, BloomLevel::High, 1.0);
        assert!(
            dark[at(32, 32)].abs_diff(100) <= 1 && dark[at(32, 32) + 1].abs_diff(50) <= 1,
            "exposure 0.5: {:?}",
            &dark[at(32, 32)..at(32, 32) + 4]
        );
        assert_eq!(dark[at(32, 32) + 3], 255, "world alpha changed");
        assert_eq!(
            draw(&device, &queue, &mut post, &colour, BloomLevel::Off, 1.0),
            colour,
            "the grade applied with bloom off"
        );
        post.bloom_grade = Grade {
            threshold: 0.0,
            exposure: 1.0,
            saturation: 0.0,
        };
        let grey = draw(&device, &queue, &mut post, &colour, BloomLevel::Low, 1.0);
        let px = &grey[at(32, 32)..at(32, 32) + 3];
        assert!(
            px[0].abs_diff(px[1]) <= 1 && px[1].abs_diff(px[2]) <= 1,
            "{px:?}"
        );
        assert_eq!(
            draw(&device, &queue, &mut post, &flat, BloomLevel::Low, 1.0)[at(5, 5)],
            20
        );
        // A threshold keeps a dim light source from blooming while a bright one still does.
        let halo = |image: &[u8], post: &mut Post| {
            let out = draw(&device, &queue, post, image, BloomLevel::High, 1.0);
            out[at(37, 32)]
        };
        let dim = world(28, 8, [150, 150, 150, 128]);
        let bright = world(28, 8, [255, 255, 255, 128]);
        post.bloom_grade = Grade::default();
        assert!(
            halo(&dim, &mut post) > 20,
            "the dim spot blooms with no threshold"
        );
        post.bloom_grade.threshold = 0.85;
        assert_eq!(
            halo(&dim, &mut post),
            20,
            "the dim spot bloomed past the threshold"
        );
        assert!(
            halo(&bright, &mut post) > 20,
            "the bright spot stopped blooming"
        );
        post.bloom_grade = Grade::default();
    }
}
