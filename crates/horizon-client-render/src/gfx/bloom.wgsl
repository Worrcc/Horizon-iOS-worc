// Original dual-filter bloom. All input/output colours remain in world gamma space.
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> params: vec4<f32>; // threshold (0: none), knee
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vertex {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var v: Vertex;
    v.position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    v.uv = vec2(p.x, 1.0 - p.y);
    return v;
}
fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear_sampler, uv, 0.0).rgb;
}
// What of a world texel blooms: light sources' marks in the frame's alpha (terrain.wgsl's
// `solid_finish_at` and HDR output's `hdr_mark`, below 0.75). Everything else, the sky
// and clouds included (0.75 and up), adds nothing.
fn marked(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(source, linear_sampler, uv, 0.0);
    var rgb = c.rgb * clamp((0.75 - c.a) * 4.0, 0.0, 1.0);
    if params.x > 0.0 {
        rgb = threshold(rgb);
    }
    return rgb;
}
// An optional soft-knee luma threshold on the marked light (a mod's `bloom-threshold`, M3),
// as a 0..1 mask that is 1 at white: the frame is LDR, so the light above the threshold
// alone (at most 1 - threshold) would leave only a faint halo.
fn threshold(c: vec3<f32>) -> vec3<f32> {
    let l = dot(c, vec3(0.2126, 0.7152, 0.0722));
    var s = clamp(l - params.x + params.y, 0.0, 2.0 * params.y);
    s = s * s / (4.0 * params.y + 1e-4);
    let w = min(max(s, l - params.x) / max(1.0 - params.x, 1e-4), 1.0);
    return c * w;
}
@fragment fn down0(v: Vertex) -> @location(0) vec4<f32> {
    let d = 1.0 / vec2<f32>(textureDimensions(source));
    let c = marked(v.uv + vec2(-1.0, -1.0) * d)
        + marked(v.uv + vec2(1.0, -1.0) * d)
        + marked(v.uv + vec2(-1.0, 1.0) * d)
        + marked(v.uv + vec2(1.0, 1.0) * d);
    return vec4(c * 0.25, 0.0);
}
@fragment fn down(v: Vertex) -> @location(0) vec4<f32> {
    let d = 1.0 / vec2<f32>(textureDimensions(source));
    let corners = tap(v.uv + vec2(-1.0, -1.0) * d)
        + tap(v.uv + vec2(1.0, -1.0) * d)
        + tap(v.uv + vec2(-1.0, 1.0) * d)
        + tap(v.uv + vec2(1.0, 1.0) * d);
    return vec4(tap(v.uv) * 0.5 + corners * 0.125, 0.0);
}
@fragment fn up(v: Vertex) -> @location(0) vec4<f32> {
    let d = 0.5 / vec2<f32>(textureDimensions(source));
    let c = tap(v.uv + vec2(-1.0, -1.0) * d)
        + tap(v.uv + vec2(1.0, -1.0) * d)
        + tap(v.uv + vec2(-1.0, 1.0) * d)
        + tap(v.uv + vec2(1.0, 1.0) * d);
    return vec4(c * 0.25, 0.0);
}
