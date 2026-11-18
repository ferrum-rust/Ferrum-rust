struct Sky {
    projection_view: mat4x4<f32>,
    orbit: mat4x4<f32>,
    center: vec4<f32>,
    rim: vec4<f32>,
    state: vec4<f32>,
    sprites: vec4<u32>,
    extent: vec4<f32>,
}
@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var nearest: sampler;
struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
}
@vertex fn vs(@location(0) p:vec3<f32>, @location(1) uv:vec2<f32>, @location(2) kind:u32) -> Varying {
    var o: Varying;
    var world: vec4<f32>;
    o.uv = uv;
    o.kind = kind;
    if kind <= 1u {
        world = vec4(-sky.state.y * p.y, -p.z * sky.state.x, sky.state.y * p.x, 1.0);
        o.color = select(sky.rim, sky.center, kind == 0u);
    } else {
        world = sky.orbit * vec4(p, 1.0);
        o.color = select(vec4(1.0,1.0,1.0,sky.state.z), vec4(sky.state.w), kind == 4u);
        if kind == 2u { o.uv = uv * sky.extent.xy; }
        if kind == 3u { o.uv = (uv + vec2<f32>(sky.sprites.zw)) * vec2(0.25,0.5) * sky.extent.zw; }
    }
    o.clip = sky.projection_view * world;
    o.clip.z = o.clip.w;
    return o;
}
@fragment fn fs(i:Varying) -> @location(0) vec4<f32> {
    if i.kind == 2u || i.kind == 3u {
        let layer = select(sky.sprites.x,sky.sprites.y,i.kind == 3u);
        return i.color * textureSampleLevel(atlas,nearest,i.uv,i32(layer),0.0);
    }
    return i.color;
}
