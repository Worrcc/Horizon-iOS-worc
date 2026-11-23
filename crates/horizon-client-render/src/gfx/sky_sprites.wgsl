// Client mods' sky sprites (sky_sprites.rs): textured quads on the sky dome.
struct SkySprites {
    // Projection * view without the camera's translation, as the stars use.
    clip_from_world: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> sky: SkySprites;
@group(0) @binding(1) var page: texture_2d<f32>;
@group(0) @binding(2) var page_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex fn vs_sky_sprite(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> Out {
    var clip = sky.clip_from_world * vec4(position, 1.0);
    // On the far plane (reverse depth: 0): the dome lies beyond a short render
    // distance's far plane, and corners behind the camera still clip at w = 0.
    clip.z = 0.0;
    return Out(clip, uv, color);
}

// Alpha blended (src alpha, one minus src alpha); the pipeline keeps the frame's alpha.
@fragment fn fs_sky_sprite_translucent(in: Out) -> @location(0) vec4<f32> {
    return textureSample(page, page_sampler, in.uv) * in.color;
}

// Adding light (one, one): the colour premultiplied by its alpha.
@fragment fn fs_sky_sprite_emissive(in: Out) -> @location(0) vec4<f32> {
    let c = textureSample(page, page_sampler, in.uv) * in.color;
    return vec4(c.rgb * c.a, 0.0);
}
