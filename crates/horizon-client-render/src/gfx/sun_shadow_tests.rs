use super::*;
use glam::{DVec3, Mat4, Vec3, Vec4Swizzles};

fn drawn(centre: [i64; 3], sun_angle: f32, revision: u64) -> Drawn {
    Drawn {
        centre,
        sun_angle,
        revision,
        level: ShadowLevel::Medium,
    }
}

#[test]
fn the_map_is_drawn_again_only_when_the_light_the_cell_or_the_terrain_changes() {
    let mut policy = Policy::default();
    let mut draws = 0;
    let mut frame = |p: &mut Policy, d: Drawn| {
        let drew = p.frame(d).is_some();
        draws += u32::from(drew);
        drew
    };
    assert!(
        frame(&mut policy, drawn([0, 64, 0], 10.0, 0)),
        "the first frame draws"
    );
    // A still scene: never again, however many frames.
    for _ in 0..100 {
        assert!(!frame(&mut policy, drawn([0, 64, 0], 10.0, 0)));
    }
    // The sun creeps: not until it has moved more than half a degree from the drawn angle.
    assert!(!frame(&mut policy, drawn([0, 64, 0], 10.4, 0)));
    assert!(frame(&mut policy, drawn([0, 64, 0], 10.6, 0)));
    // The light or the cell moving: no sooner than four frames after the last draw.
    assert!(!frame(&mut policy, drawn([8, 64, 0], 10.6, 0)));
    assert!(!frame(&mut policy, drawn([8, 64, 0], 10.6, 0)));
    assert!(!frame(&mut policy, drawn([8, 64, 0], 10.6, 0)));
    assert!(frame(&mut policy, drawn([8, 64, 0], 10.6, 0)));
    // Terrain changes alone wait longer: every 30 frames at most.
    for _ in 0..29 {
        assert!(!frame(&mut policy, drawn([8, 64, 0], 10.6, 1)));
    }
    assert!(frame(&mut policy, drawn([8, 64, 0], 10.6, 1)));
    // Across the wrap at 360 degrees the sun is near.
    let mut wrap = Policy::default();
    assert!(wrap.frame(drawn([0; 3], 359.9, 0)).is_some());
    for _ in 0..4 {
        assert!(wrap.frame(drawn([0; 3], 0.2, 0)).is_none());
    }
    assert_eq!(draws, 4);
}

#[test]
fn only_a_change_to_what_sections_near_the_map_cast_redraws_it() {
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("Skipping: no adapter");
        return;
    };
    let (device, _queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut shadow = SunShadow::new(&device);
    shadow.set_level(ShadowLevel::Low);
    shadow.policy.frame(drawn([0, 64, 0], 0.0, 0));
    let vertex = |y: f32, light: u16| crate::mesher::TerrainVertex {
        pos: [0.0, y, 0.0],
        uv: [0, 0],
        layer: 3,
        light,
        color: 0,
    };
    let revision = |s: &SunShadow| s.revision();
    let meshed = |s: &mut SunShadow, key, solid: &[crate::mesher::TerrainVertex]| {
        let old = s.take_cast(key);
        s.section_meshed(key, old, solid);
    };
    let r0 = revision(&shadow);
    // 32 blocks of reach, 8 more for the cell: sections 2 columns out lie inside.
    meshed(&mut shadow, (2, 3, -2), &[vertex(1.0, 0)]);
    assert_eq!(revision(&shadow), r0 + 1, "a new section casts");
    meshed(&mut shadow, (2, 3, -2), &[vertex(1.0, 240)]);
    assert_eq!(revision(&shadow), r0 + 1, "a light change alone does not");
    meshed(&mut shadow, (2, 3, -2), &[vertex(2.0, 240)]);
    assert_eq!(revision(&shadow), r0 + 2, "moved geometry does");
    meshed(&mut shadow, (2, 4, -2), &[]);
    assert_eq!(revision(&shadow), r0 + 2, "an empty section never cast");
    shadow.section_dropped((2, 3, -2));
    assert_eq!(revision(&shadow), r0 + 3, "a dropped one cast");
    meshed(&mut shadow, (10, 3, 0), &[vertex(1.0, 0)]);
    assert_eq!(revision(&shadow), r0 + 3, "far from the map");
}

#[test]
fn the_centre_snaps_to_the_cell_grid() {
    assert_eq!(centre(DVec3::new(3.9, 64.0, -3.9)), [0, 64, 0]);
    assert_eq!(centre(DVec3::new(4.1, 67.0, -12.5)), [8, 64, -16]);
}

#[test]
fn the_light_is_the_sun_by_day_the_moon_by_night_and_fades_at_the_horizon() {
    let (noon, strength) = light(0.0);
    assert!((noon - Vec3::Y).length() < 1e-6);
    assert_eq!(strength, 1.0);
    let (midnight, strength) = light(180.0);
    assert!((midnight - Vec3::Y).length() < 1e-5, "the moon overhead");
    assert_eq!(strength, 1.0);
    // The light's height above the horizon decides: none at sunrise.
    assert_eq!(light(90.0).1, 0.0);
    assert!(light(80.0).1 > 0.0 && light(80.0).1 < 1.0);
}

#[test]
fn a_point_and_the_point_the_light_passes_through_above_it_share_a_texel() {
    let (dir, _) = light(30.0);
    let m = light_view_proj(dir, half_size(48.0));
    let ground = m * Vec3::new(3.0, -2.0, 5.0).extend(1.0);
    let above = m * (Vec3::new(3.0, -2.0, 5.0) + dir * 40.0).extend(1.0);
    assert!((ground.xy() - above.xy()).length() < 1e-5);
    assert!(
        above.z < ground.z,
        "nearer the light, nearer the map's near plane"
    );
    for p in [ground, above] {
        assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0 && (0.0..=1.0).contains(&p.z));
    }
}

#[test]
fn a_pixel_goes_back_to_where_its_point_lies_in_the_map() {
    let camera = DVec3::new(100.3, 70.6, -40.2);
    let (dir, _) = light(20.0);
    let light = light_view_proj(dir, half_size(48.0));
    let to_map = camera_view_proj(light, camera, centre(camera));
    let view_proj = Mat4::perspective_rh(1.2, 16.0 / 9.0, 0.05, 512.0)
        * Mat4::look_to_rh(Vec3::ZERO, Vec3::new(0.3, -0.4, -1.0).normalize(), Vec3::Y);
    let size = [1280, 720];
    let pixel = pixel_view_proj(to_map, view_proj, size);
    for p in [Vec3::new(2.0, -1.5, -10.0), Vec3::new(-6.0, -3.0, -30.0)] {
        let clip = view_proj * p.extend(1.0);
        let ndc = clip.xyz() / clip.w;
        let x = (ndc.x + 1.0) * 0.5 * size[0] as f32;
        let y = (1.0 - ndc.y) * 0.5 * size[1] as f32;
        let back = pixel * glam::Vec4::new(x, y, ndc.z, 1.0);
        let expected = to_map * p.extend(1.0);
        assert!(
            (back.xyz() / back.w - expected.xyz()).length() < 2e-4,
            "{back} {expected}"
        );
    }
}

#[test]
fn the_caster_shaders_validate() {
    for source in [caster_source(), ENTITY_SOURCE.to_owned()] {
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

/// The GPU scene: a floor 20 blocks below a camera looking straight down, its left half lit
/// by the sky alone, its right half by the sky and a torch (block light 15), and a slab of
/// one top face 4 blocks over it; the sun 45 degrees from noon casts the slab's shadow 4
/// blocks toward +z.
mod gpu {
    use super::super::super::{Globals, TERRAIN_ATTRIBUTES, terrain_group, terrain_layout_entries};
    use super::*;
    use crate::mesher::TerrainVertex;
    use bytemuck::Zeroable;
    use wgpu::util::DeviceExt;

    const SIZE: u32 = 64;
    const CAMERA: DVec3 = DVec3::new(0.0, 20.0, 0.0);

    fn quad(x: [f32; 2], y: f32, z: [f32; 2], sky: u8, block: u8) -> [TerrainVertex; 4] {
        let v = |px: f32, pz: f32| TerrainVertex {
            pos: [px, y, pz],
            uv: [0, 0],
            layer: 0x8000,
            light: (u16::from(sky) * 16) | ((u16::from(block) * 16) << 8),
            color: 0xffff_ffff,
        };
        [v(x[0], z[0]), v(x[0], z[1]), v(x[1], z[1]), v(x[1], z[0])]
    }

    struct Scene {
        device: wgpu::Device,
        queue: wgpu::Queue,
        layout: wgpu::BindGroupLayout,
        resources: Resources,
        vertices: wgpu::Buffer,
        indices: wgpu::Buffer,
        quads: u32,
    }

    struct Resources {
        sprites: Vec<wgpu::TextureView>,
        sampler: wgpu::Sampler,
        lightmap: wgpu::TextureView,
        lightmap_sampler: wgpu::Sampler,
        thresholds: wgpu::TextureView,
    }

    fn scene() -> Option<Scene> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        eprintln!(
            "Sun shadow GPU readback adapter: {}",
            adapter.get_info().name
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &terrain_layout_entries(),
        });
        let texture = |size: u32, layers: u32, data: &[u8]| {
            device.create_texture_with_data(
                &queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: size,
                        height: size,
                        depth_or_array_layers: layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                data,
            )
        };
        // White opaque sprites; WebGL-like backends want two layers for an array view.
        let white = texture(16, 2, &[255; 16 * 16 * 4 * 2]);
        let array = white.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        // The lightmap's texel at (block, sky): the brighter of the two, linear in the level,
        // so a fractional sky level reads back exactly.
        let lightmap: Vec<u8> = (0..16u8)
            .flat_map(|sky| {
                (0..16u8).flat_map(move |block| {
                    let v = sky.max(block) * 17;
                    [v, v, v, 255]
                })
            })
            .collect();
        let resources = Resources {
            sprites: vec![array; 8],
            sampler: device.create_sampler(&Default::default()),
            lightmap: texture(16, 1, &lightmap).create_view(&Default::default()),
            lightmap_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            thresholds: texture(1, 1, &[255; 4]).create_view(&Default::default()),
        };
        // Camera-relative: the floor 20 below, the slab 16 below.
        let mut quads = Vec::new();
        quads.extend(quad([-8.0, 0.0], -20.0, [-8.0, 8.0], 15, 0));
        quads.extend(quad([0.0, 8.0], -20.0, [-8.0, 8.0], 15, 15));
        quads.extend(quad([-6.0, 6.0], -16.0, [-2.0, 2.0], 15, 0));
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&quads),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index: Vec<u32> = (0..3u32)
            .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| 4 * q + i))
            .collect();
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&index),
            usage: wgpu::BufferUsages::INDEX,
        });
        Some(Scene {
            device,
            queue,
            layout,
            resources,
            vertices,
            indices,
            quads: 3,
        })
    }

    impl Scene {
        fn group(
            &self,
            globals: &wgpu::Buffer,
            map: &wgpu::TextureView,
            sampler: &wgpu::Sampler,
        ) -> wgpu::BindGroup {
            let r = &self.resources;
            terrain_group(
                &self.device,
                "test",
                &self.layout,
                globals,
                &r.sprites,
                &r.sampler,
                &r.lightmap,
                &r.lightmap_sampler,
                &r.thresholds,
                (map, sampler),
            )
        }

        fn instance(&self, origin: [f32; 3]) -> wgpu::Buffer {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&super::super::super::section_instance(
                        origin, 0, 1.0,
                    )),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        }

        /// The floor's pixels with the shadows at `strength` (0: off), drawn by terrain.wgsl's
        /// `source` (`vs_terrain`, `fs_opaque`), the map's draw counted in `shadow.updates`.
        fn draw(&self, shadow: &mut SunShadow, strength: f32, source: &str) -> Vec<u8> {
            self.draw_soft(shadow, strength, 0.0, source)
        }
        /// [`Self::draw`] with an atmosphere's `softness`.
        fn draw_soft(
            &self,
            shadow: &mut SunShadow,
            strength: f32,
            softness: f32,
            source: &str,
        ) -> Vec<u8> {
            let device = &self.device;
            let view_proj = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.0, 64.0)
                * Mat4::look_to_rh(Vec3::ZERO, -Vec3::Y, -Vec3::Z);
            let mut globals = Globals::zeroed();
            globals.view_proj = view_proj.to_cols_array_2d();
            globals.fog = [1e9; 4];
            globals.filtering = [0.0, 16.0, 0.0, 0.0];
            let level = ShadowLevel::Medium;
            let (resolution, reach) = level.map().unwrap();
            shadow.set_level(level);
            shadow.ensure_maps(device, Some(level));
            let (dir, fade) = light(45.0);
            let half = half_size(reach);
            let centre = centre(CAMERA);
            let light = light_view_proj(dir, half);
            let camera = camera_view_proj(light, CAMERA, centre);
            if strength > 0.0 {
                let [inputs, toward] =
                    shader_inputs(strength * fade, softness, resolution, half, reach, dir);
                globals.shadow = inputs;
                globals.shadow_light = toward;
                globals.shadow_view_proj = camera.to_cols_array_2d();
                globals.shadow_pixel =
                    pixel_view_proj(camera, view_proj, [SIZE; 2]).to_cols_array_2d();
            }
            let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&globals),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            // The casters, as the renderer draws them: centre-relative origins.
            if strength > 0.0
                && shadow
                    .policy
                    .frame(Drawn {
                        centre,
                        sun_angle: 45.0,
                        revision: 0,
                        level,
                    })
                    .is_some()
            {
                if shadow.pipelines.is_none() {
                    let entities =
                        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                            label: None,
                            entries: &[
                                wgpu::BindGroupLayoutEntry {
                                    binding: 1,
                                    visibility: wgpu::ShaderStages::FRAGMENT,
                                    ty: wgpu::BindingType::Texture {
                                        sample_type: wgpu::TextureSampleType::Float {
                                            filterable: true,
                                        },
                                        view_dimension: wgpu::TextureViewDimension::D2,
                                        multisampled: false,
                                    },
                                    count: None,
                                },
                                wgpu::BindGroupLayoutEntry {
                                    binding: 2,
                                    visibility: wgpu::ShaderStages::FRAGMENT,
                                    ty: wgpu::BindingType::Sampler(
                                        wgpu::SamplerBindingType::Filtering,
                                    ),
                                    count: None,
                                },
                            ],
                        });
                    shadow.pipelines = Some(Pipelines::new(
                        device,
                        &self.layout,
                        None,
                        &entities,
                        [1.0, 2048.0],
                    ));
                }
                shadow.write_uniform(&self.queue, light, camera);
                let blank = map_texture(device, "blank", 1, wgpu::TextureUsages::empty());
                let group = self.group(&globals_buffer, &blank.view, &shadow.sampler);
                let offset =
                    CAMERA - DVec3::new(centre[0] as f64, centre[1] as f64, centre[2] as f64);
                let origins = self.instance(offset.as_vec3().to_array());
                let p = shadow.pipelines.as_ref().unwrap();
                let mut pass = shadow.terrain_pass(&mut encoder, None).unwrap();
                pass.set_pipeline(&p.instanced);
                pass.set_bind_group(0, &group, &[]);
                pass.set_bind_group(2, &p.group, &[]);
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_vertex_buffer(1, origins.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.quads * 6, 0, 0..1);
                drop(pass);
                shadow.updates += 1;
            }
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&self.layout)],
                ..Default::default()
            });
            let instance_attributes = wgpu::vertex_attr_array![5 => Float32x4];
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_terrain"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<TerrainVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &TERRAIN_ATTRIBUTES,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 16,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &instance_attributes,
                        }),
                    ],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_opaque"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
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
            let color = target(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::COPY_SRC,
            );
            let depth = target(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::empty(),
            );
            let (color_view, depth_view) = (
                color.create_view(&Default::default()),
                depth.create_view(&Default::default()),
            );
            let group = self.group(&globals_buffer, shadow.view(), &shadow.sampler);
            let origins = self.instance([0.0; 3]);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_vertex_buffer(1, origins.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.quads * 6, 0, 0..1);
            }
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(SIZE * SIZE * 4),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE * 4),
                        rows_per_image: None,
                    },
                },
                color.size(),
            );
            self.queue.submit([encoder.finish()]);
            read.slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let bytes = read.slice(..).get_mapped_range().unwrap().to_vec();
            read.unmap();
            bytes
        }
    }

    /// The red value at the block `(x, z)` of the floor (or what covers it).
    fn at(pixels: &[u8], x: f32, z: f32) -> u8 {
        let col = ((x + 8.0) / 16.0 * SIZE as f32) as usize;
        let row = ((z + 8.0) / 16.0 * SIZE as f32) as usize;
        pixels[(row * SIZE as usize + col) * 4]
    }

    #[test]
    fn a_slab_shades_the_sky_light_at_the_strength_and_torches_stay_lit() {
        let Some(scene) = scene() else {
            eprintln!("Skipping sun shadow GPU readback: no adapter");
            return;
        };
        let mut shadow = SunShadow::new(&scene.device);
        let production = include_str!("../terrain.wgsl");
        let lit = scene.draw(&mut shadow, 0.0, production);
        for strength in [0.5, 0.8] {
            let mut shadow = SunShadow::new(&scene.device);
            let shaded = scene.draw(&mut shadow, strength, production);
            let expected = (255.0 * (1.0 - strength)).round() as i32;
            // The slab's shadow on the sky-lit floor, 4 blocks toward +z.
            let dark = i32::from(at(&shaded, -4.0, 4.0));
            assert!(
                (dark - expected).abs() <= 2,
                "strength {strength}: {dark}, expected {expected}"
            );
            // Under it, the torch-lit floor keeps its light; out of it, the floor and the
            // slab's top stay as without shadows (no acne).
            for (x, z) in [
                (4.0, 4.0),
                (-4.0, -6.0),
                (4.0, -6.0),
                (-4.0, 0.0),
                (-7.0, 7.0),
            ] {
                assert_eq!(
                    at(&shaded, x, z),
                    at(&lit, x, z),
                    "({x}, {z}) at {strength}"
                );
            }
            assert_eq!(at(&lit, -4.0, 4.0), 255);
        }
    }

    #[test]
    fn softness_widens_the_shadow_edge_and_keeps_its_core() {
        let Some(scene) = scene() else {
            eprintln!("Skipping sun shadow GPU readback: no adapter");
            return;
        };
        let production = include_str!("../terrain.wgsl");
        let draw = |softness| {
            let mut shadow = SunShadow::new(&scene.device);
            scene.draw_soft(&mut shadow, 0.5, softness, production)
        };
        let (hard, soft) = (draw(0.0), draw(1.0));
        // Partly shaded pixels over the floor: between the shadow's 128 and the light's 255.
        let partial = |pixels: &[u8]| {
            (0..SIZE)
                .flat_map(|row| (0..SIZE).map(move |col| (row, col)))
                .filter(|&(row, col)| {
                    let v = pixels[((row * SIZE + col) * 4) as usize];
                    (132..251).contains(&v)
                })
                .count()
        };
        assert!(
            partial(&soft) > partial(&hard),
            "softness 1 left {} partly shaded pixels, hard edges {}",
            partial(&soft),
            partial(&hard)
        );
        assert!(i32::from(at(&soft, -4.0, 4.0)).abs_diff(128) <= 2);
        // Low ignores it.
        let mut low = SunShadow::new(&scene.device);
        low.set_softness(1.0);
        low.set_level(ShadowLevel::Low);
        assert_eq!(low.softness(), 0.0);
        low.set_level(ShadowLevel::High);
        assert_eq!(low.softness(), 1.0);
        low.set_softness(f32::NAN);
        assert_eq!(low.softness(), 0.0);
    }

    #[test]
    fn shadows_off_draw_exactly_what_the_shader_without_them_draws() {
        let Some(scene) = scene() else {
            eprintln!("Skipping sun shadow GPU readback: no adapter");
            return;
        };
        let production = include_str!("../terrain.wgsl");
        let without = production.replace("globals.shadow.x > 0.0", "false");
        assert_ne!(without, production);
        let mut shadow = SunShadow::new(&scene.device);
        let off = scene.draw(&mut shadow, 0.0, production);
        assert_eq!(off, scene.draw(&mut shadow, 0.0, &without));
        assert_eq!(shadow.updates, 0, "no map drawn while off");
    }

    #[test]
    fn the_cached_map_is_not_drawn_again_while_nothing_moves() {
        let Some(scene) = scene() else {
            eprintln!("Skipping sun shadow GPU readback: no adapter");
            return;
        };
        let production = include_str!("../terrain.wgsl");
        let mut shadow = SunShadow::new(&scene.device);
        let first = scene.draw(&mut shadow, 0.5, production);
        for _ in 0..6 {
            // Every frame still shows the shadow, from the map drawn once.
            assert_eq!(scene.draw(&mut shadow, 0.5, production), first);
        }
        assert_eq!(shadow.updates, 1);
    }
}
