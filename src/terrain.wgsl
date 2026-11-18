struct Camera { view_proj: mat4x4<f32>, eye_fog: vec4<f32>, outline_min: vec4<f32>, outline_max: vec4<f32>, lighting: vec4<f32>, effects: vec4<f32>, sky_color: vec4<f32>, fog_ranges: vec4<f32>, sky_origin: vec4<f32>, hand_view: vec4<f32>, distant_origin: vec4<i32>, distant_mask: array<vec4<u32>,32>, distant_far: vec4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tiles: texture_2d_array<f32>;
@group(0) @binding(2) var tile_sampler: sampler;
@group(0) @binding(3) var atlas: texture_2d_array<f32>;
@group(0) @binding(4) var atlas_sampler: sampler;
@group(0) @binding(5) var lightmap: texture_2d_array<f32>;
@group(0) @binding(6) var light_sampler: sampler;
@group(0) @binding(7) var flowing_tiles: texture_2d_array<f32>;
struct Output { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) @interpolate(flat) layer:u32, @location(2) color:vec4<f32>, @location(3) world:vec3<f32>, @location(4) light:vec2<f32>, @location(5) fog_depth:f32, @location(6) glint_uv:vec2<f32>, @location(7) @interpolate(flat) distant_chunk:vec2<i32> };
const END_PORTAL_COLORS: array<vec3<f32>, 16> = array<vec3<f32>, 16>(
  vec3<f32>(0.10000000, 0.10000000, 0.10000000),
  vec3<f32>(0.00631738, 0.05095978, 0.04753906),
  vec3<f32>(0.01473936, 0.05423397, 0.05350704),
  vec3<f32>(0.02494494, 0.05886573, 0.06152015),
  vec3<f32>(0.03494681, 0.06337459, 0.05233256),
  vec3<f32>(0.03453727, 0.04706804, 0.06697500),
  vec3<f32>(0.04626409, 0.06108771, 0.09075278),
  vec3<f32>(0.05361915, 0.08476582, 0.05008522),
  vec3<f32>(0.05897343, 0.07285756, 0.10843927),
  vec3<f32>(0.05496822, 0.06198072, 0.10531616),
  vec3<f32>(0.07629505, 0.07901610, 0.08490379),
  vec3<f32>(0.04083666, 0.14194381, 0.13754530),
  vec3<f32>(0.11805945, 0.08573964, 0.12881733),
  vec3<f32>(0.02955061, 0.19708616, 0.20123141),
  vec3<f32>(0.13644970, 0.26000684, 0.20137758),
  vec3<f32>(0.06071605, 0.23611547, 0.49611820)
);
const END_PORTAL_ROT: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
  vec2<f32>(1.00000000, 0.00000000),
  vec2<f32>(0.93969262, 0.34202014),
  vec2<f32>(0.71933980, 0.69465837),
  vec2<f32>(0.30901699, 0.95105652),
  vec2<f32>(-0.24192190, 0.97029573),
  vec2<f32>(-0.76604444, 0.64278761),
  vec2<f32>(-1.00000000, 0.00000000),
  vec2<f32>(-0.71933980, -0.69465837),
  vec2<f32>(0.03489950, -0.99939083),
  vec2<f32>(0.80901699, -0.58778525),
  vec2<f32>(0.93969262, 0.34202014),
  vec2<f32>(0.17364818, 0.98480775),
  vec2<f32>(-0.80901699, 0.58778525),
  vec2<f32>(-0.84804810, -0.52991926),
  vec2<f32>(0.24192190, -0.97029573),
  vec2<f32>(1.00000000, 0.00000000)
);
@vertex fn vs_main(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) color:vec4<f32>,@location(4) light:vec4<f32>)->Output {
 var p=position; if (layer & 1048576u)!=0u { p.x+=sin(camera.effects.y*0.008)*12.0; }
 if (layer & 8388608u)!=0u { p+=select(camera.sky_origin.xyz, camera.eye_fog.xyz, (layer & 65536u)!=0u); }
 var o:Output; o.clip=camera.view_proj*vec4(p,1.0);o.uv=uv;o.layer=layer;o.color=color;o.world=p;o.light=light.xy*(255.0/16.0);
 o.fog_depth=o.clip.w; o.glint_uv=light.zw;
 o.distant_chunk=vec2<i32>(0);
 if (layer & 2147483648u)!=0u { o.distant_chunk=vec2<i32>(floor((p.xz+round(light.zw*255.0)-vec2(128.0))/16.0)); }
 if (layer & 8388608u)!=0u { o.clip.z=o.clip.w; }
 if (layer & 4194304u)!=0u {
   let hand_xy = vec2(dot(camera.hand_view.xy, p.xy), dot(camera.hand_view.zw, p.xy));
   let w=-p.z; o.clip=vec4(hand_xy.x*camera.effects.w,hand_xy.y,(w-0.05)*0.001,w); o.world=camera.eye_fog.xyz; o.fog_depth=0.0;
   if (layer & 536870912u)!=0u {
     let overlay_xy = p.xy * length(camera.hand_view.xy);
     o.clip=vec4(overlay_xy.x*camera.effects.w,overlay_xy.y,0.0,w);
   }
 } else if (layer & 536870912u)!=0u {
   o.clip.z = select(o.clip.z, 0.0, o.clip.z >= 0.0);
 }
 return o;
}
@fragment fn fs_main(i:Output)->@location(0) vec4<f32> {
 let layer = i32(i.layer & 65535u);
 let uv_dx = dpdx(i.uv);
 let uv_dy = dpdy(i.uv);
 if (i.layer & 2147483648u)!=0u {
   if i.world.x<camera.distant_far.x || i.world.z<camera.distant_far.y ||
       i.world.x>=camera.distant_far.z || i.world.z>=camera.distant_far.w { discard; }
   let local=i.distant_chunk-camera.distant_origin.xy;
   if all(local>=vec2<i32>(0)) && all(local<vec2<i32>(64)) {
     let index=u32(local.y*64+local.x);
     if (camera.distant_mask[index/128u][(index/32u)%4u] & (1u<<(index%32u)))!=0u { discard; }
   }
 }
 let sky = (i.layer & 8388608u) != 0u;
 let fog_color = vec3(camera.lighting.zw, camera.effects.z);
 let fog_range = select(camera.fog_ranges.xy, camera.fog_ranges.zw, sky);
 var fog = clamp((i.fog_depth - fog_range.x) / (fog_range.y - fog_range.x), 0.0, 1.0);
 if camera.sky_color.a > 0.0 { fog = 1.0 - exp(-camera.sky_color.a * max(i.fog_depth, 0.0)); }
 if sky {
   if (i.layer & 65536u) != 0u {
     let max_atlas = max(i32(textureNumLayers(atlas)) - 1, 0);
     let tex = textureSampleLevel(atlas, atlas_sampler, i.uv, min(layer, max_atlas), 0.0);
     return vec4(tex.rgb * i.color.rgb, 1.0);
   }
   return vec4(mix(camera.sky_color.rgb, fog_color, fog), 1.0);
 }
 if (i.layer & 69206016u) == 69206016u {
   let max_atlas = max(i32(textureNumLayers(atlas)) - 1, 0);
   let sky_layer = min(i32(i.layer & 65535u), max_atlas);
   let sky_ext = i.uv;
   let portal_layer = min(i32(u32(round(i.color.r * 255.0)) | (u32(round(i.color.g * 255.0)) << 8u)), max_atlas);
   let portal_ext = i.color.ba;
   let eye = camera.eye_fog.xyz;
   let scroll = fract(camera.effects.y / 700.0);
   let dy = eye.y - i.world.y;
   let dy_safe = select(dy, 1e-4, abs(dy) < 1e-4);
   let rel_xz = i.world.xz - eye.xz;
   var acc = vec3<f32>(0.0);
   for (var k = 0; k < 16; k = k + 1) {
     let depth_factor = (dy_safe + f32(16 - k)) / dy_safe;
     let centered = rel_xz * depth_factor - vec2<f32>(0.5, 0.5);
     let rot = END_PORTAL_ROT[k];
     let rotated = vec2<f32>(
       centered.x * rot.x - centered.y * rot.y,
       centered.x * rot.y + centered.y * rot.x
     ) + vec2<f32>(0.5, 0.5);
     let uv = rotated * 0.0625 + vec2<f32>(0.0, scroll);
     let ext = select(portal_ext, sky_ext, k == 0);
     let layer_id = select(portal_layer, sky_layer, k == 0);
     acc += textureSampleLevel(atlas, atlas_sampler, fract(uv) * ext, layer_id, 0.0).rgb * END_PORTAL_COLORS[k];
   }
   let rgb = min(acc, vec3<f32>(1.0));
   return vec4(mix(rgb, fog_color, fog), 1.0);
 }
 var texel = vec4(1.0);
 if (i.layer & 2097152u) == 0u {
   if (i.layer & 1073741824u) != 0u {
     texel = textureSampleGrad(atlas, atlas_sampler, fract(i.uv) * vec2(0.25, 1.0), layer, uv_dx * vec2(0.25, 1.0), uv_dy * vec2(0.25, 1.0));
   } else if (i.layer & 67108864u) != 0u && (i.layer & 65536u) == 0u {
     texel = textureSampleGrad(flowing_tiles, tile_sampler, i.uv, layer, uv_dx, uv_dy);
   } else if (i.layer & 16777216u) != 0u {
     if (i.layer & 65536u) != 0u { texel=textureSampleLevel(atlas,atlas_sampler,i.uv,min(layer,i32(textureNumLayers(atlas))-1),0.0); }
     else { texel=textureSampleLevel(tiles,tile_sampler,i.uv,min(layer,i32(textureNumLayers(tiles))-1),0.0); }
   } else if (i.layer & 65536u) != 0u {
     texel = textureSampleGrad(atlas, atlas_sampler, i.uv, min(layer, i32(textureNumLayers(atlas))-1), uv_dx, uv_dy);
   } else {
     texel = textureSampleGrad(tiles, tile_sampler, i.uv, min(layer, i32(textureNumLayers(tiles))-1), uv_dx, uv_dy);
   }
 }
 let alpha_cutoff = select(0.12, 0.004, (i.layer & 262144u) != 0u);
 if (i.layer & 67174400u) == 67174400u && texel.a <= i.glint_uv.x { discard; }
 if texel.a < alpha_cutoff && (i.layer & 524288u) == 0u && (i.layer & 1073741824u) == 0u { discard; }
 if (i.layer & 1073741824u)!=0u && texel.a*i.color.a<=0.1 { discard; }
 var color=texel.rgb*i.color.rgb;
 if (i.layer & 268435456u)!=0u {
   color=mix(color,vec3(1.0,0.0,0.0),0.3);
 }
 if (i.layer & 131072u)==0u {
   color *= textureSampleLevel(lightmap, light_sampler, (i.light + vec2(0.5)) / 16.0, 0, 0.0).rgb;
 }
 if (i.layer & 134217728u)!=0u {
   let uv = 0.25 * i.glint_uv;
   let pass_a = vec2(dot(uv, vec2(0.642788, 0.766044)), dot(uv, vec2(-0.766044, 0.642788)));
   let pass_b = vec2(dot(uv, vec2(0.984808, -0.173648)), dot(uv, vec2(0.173648, 0.984808)));
   let a = textureSampleLevel(atlas, atlas_sampler, fract(pass_a + vec2(fract(camera.effects.y / 3.0), 0.0)), i32(camera.effects.x), 0.0).rgb;
   let b = textureSampleLevel(atlas, atlas_sampler, fract(pass_b - vec2(fract(camera.effects.y / 4.873), 0.0)), i32(camera.effects.x), 0.0).rgb;
   let tint = vec3(128., 64., 204.) / 255.;
   color = min(vec3(1.), color + (a*a + b*b)*tint*tint);
 }
 // RenderTNTPrimed adds the white flash to the lit surface using SRC_ALPHA, DST_ALPHA.
 if (i.layer & 33554432u)!=0u { color=min(vec3(1.0),color+vec3(i.color.a)); }
 return vec4(mix(color,fog_color,fog),select(1.0,texel.a*i.color.a,(i.layer & 262144u)!=0u));
}

// GL_LINE_STRIP at width 2, expanded in physical viewport pixels so that the
// stroke keeps its width under perspective and at different GUI scales.
@vertex fn vs_outline(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let edge = id / 6u;
    let axis = edge / 4u;
    let corner = edge % 4u;
    var start = camera.outline_min.xyz;
    let u = (axis + 1u) % 3u;
    let v = (axis + 2u) % 3u;
    if (corner & 1u) != 0u { start[u] = camera.outline_max[u]; }
    if (corner & 2u) != 0u { start[v] = camera.outline_max[v]; }
    var end = start;
    end[axis] = camera.outline_max[axis];
    var a = camera.view_proj * vec4(start, 1.);
    var b = camera.view_proj * vec4(end, 1.);
    if a.z < 0. && b.z < 0. { return vec4(0.,0.,-1.,1.); }
    if a.z < 0. { a = mix(a, b, -a.z / (b.z-a.z)); }
    if b.z < 0. { b = mix(b, a, -b.z / (a.z-b.z)); }
    let pixel = vec2(camera.outline_min.w, camera.outline_max.w);
    let delta = (b.xy/b.w - a.xy/a.w) / pixel;
    let normal = vec2(-delta.y, delta.x) / max(length(delta), 1e-6);
    let vertex = id % 6u; // a-, b-, b+, a-, b+, a+
    let clip = select(a, b, vertex == 1u || vertex == 2u || vertex == 4u);
    let sign = select(-1., 1., vertex == 2u || vertex >= 4u);
    return vec4(clip.xy + normal * pixel * sign * clip.w, clip.zw);
}
@fragment fn fs_outline() -> @location(0) vec4<f32> {
    return vec4(0., 0., 0., 0.4);
}
@fragment fn fs_block_damage(i: Output) -> @location(0) vec4<f32> {
    let texel = textureSample(tiles, tile_sampler, i.uv, i32(i.layer));
    if texel.a * 0.5 <= 0.1 { discard; }
    return vec4(texel.rgb, texel.a * 0.5);
}
