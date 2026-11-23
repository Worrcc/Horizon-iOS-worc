
// Built-in sun shadows' caster pass (gfx/sun_shadow.rs), after terrain.wgsl: the solid terrain
// the light sees into the map, its positions relative to the map's centre (the instance's
// origin), cutout holes (leaves, plants) letting the light through.
struct SunCasterPass {
    light_view_proj: mat4x4<f32>,
    camera_view_proj: mat4x4<f32>,
}
@group(2) @binding(0) var<uniform> sun_caster: SunCasterPass;
struct SunCasterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
}
fn sun_caster_vertex(pos: vec3<f32>, uv: vec2<f32>, layer: u32) -> SunCasterOut {
    let clip = sun_caster.light_view_proj * vec4(pos, 1.0);
    var out: SunCasterOut;
    // Orthographic: w is 1. What lies past the near plane still casts, at the plane.
    out.clip = vec4(clip.xy, clamp(clip.z, 0.0, 1.0), 1.0);
    out.uv = uv;
    out.layer = layer;
    return out;
}
@vertex fn vs_sun_caster(@location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(5) origin: vec4<f32>) -> SunCasterOut {
    return sun_caster_vertex(position + origin.xyz, terrain_uv(uv, origin.w), layer);
}
@vertex fn vs_sun_caster_pulled(@builtin(vertex_index) vertex: u32,
    @location(5) origin: vec4<f32>) -> SunCasterOut {
    let c = section_corner(vertex).c;
    let uv = vec2(c.r.x & 0xffffu, c.r.x >> 16u);
    return sun_caster_vertex(bitcast<vec3<f32>>(c.p) + origin.xyz, terrain_uv(uv, origin.w),
        corner_layer(c));
}
@fragment fn fs_sun_caster(in: SunCasterOut) {
    // Bit 15 of the layer: no cutout (opaque leaves, full blocks).
    if (in.layer & 0x8000u) == 0u {
        if sprite_level(tile_uv(in.uv), in.layer & 0x7fffu, 0.0).a < 0.5 { discard; }
    }
}
