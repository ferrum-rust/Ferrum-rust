struct Panorama { crop: vec4<f32>, rotation: vec4<f32>, a: vec4<u32>, b: vec4<u32> };
@group(0) @binding(0) var<uniform> pan: Panorama;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var linear: sampler;
@group(0) @binding(3) var previous: texture_2d<f32>;
struct Screen { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vertex(@builtin(vertex_index) i:u32)->Screen {
    let uv=vec2(f32((i<<1u)&2u),f32(i&2u));
    var o:Screen; o.pos=vec4(uv.x*2.-1.,1.-uv.y*2.,0.,1.); o.uv=uv; return o;
}
fn inverse_camera(v:vec3<f32>)->vec3<f32> {
    let x=v.x;
    let y=pan.rotation.y*v.y+pan.rotation.x*v.z;
    let z=-pan.rotation.x*v.y+pan.rotation.y*v.z;
    return vec3(pan.rotation.w*x-pan.rotation.z*z,y,pan.rotation.z*x+pan.rotation.w*z);
}
fn sample_face(pos:vec3<f32>,axis:u32)->vec3<f32> {
    var uv:vec2<f32>;
    var layer:u32;
    if axis==2u {
        layer=select(pan.a.z,pan.a.x,pos.z>0.);
        uv=vec2(select(-pos.x,pos.x,pos.z>0.),pos.y);
    } else if axis==0u {
        layer=select(pan.a.w,pan.a.y,pos.x>0.);
        uv=vec2(select(pos.z,-pos.z,pos.x>0.),pos.y);
    } else {
        layer=select(pan.b.x,pan.b.y,pos.y>0.);
        uv=vec2(pos.x,select(pos.z,-pos.z,pos.y>0.));
    }
    let xy=vec2<i32>(floor((uv*0.5+0.5)*256.)) & vec2<i32>(255);
    return textureLoad(atlas,xy,i32(layer),0).rgb;
}
fn quantize_color(value:vec3<f32>)->vec3<f32> { return round(clamp(value,vec3(0.),vec3(1.))*255.)/255.; }
@fragment fn cube(s:Screen)->@location(0) vec4<f32> {
    // The intermediate's row zero stores the original GL framebuffer's bottom row.
    let ndc=s.uv*2.-1.;
    let ray=inverse_camera(vec3(-ndc.y,-ndc.x,0.57735027));
    var rgb=vec3(0.);
    for(var i=0u; i<64u; i++) {
        let jitter=(vec2(f32(i%8u),f32(i/8u))/8.-0.5)/64.;
        let origin=inverse_camera(vec3(-jitter,0.));
        let t=(select(vec3(-1.),vec3(1.),ray>=vec3(0.))-origin)/ray;
        let depth=min(t.x,min(t.y,t.z));
        let axis=select(select(2u,1u,depth==t.y),0u,depth==t.x);
        let sample=sample_face(origin+ray*depth,axis);
        rgb=quantize_color(mix(rgb,sample,f32(255u/(i+1u))/255.));
    }
    return vec4(rgb,1.);
}
@fragment fn blur(s:Screen)->@location(0) vec4<f32> {
    let uv=s.uv.yx;
    let a=textureSampleLevel(previous,linear,uv-vec2(1./256.,0.),0.).rgb;
    let b=textureSampleLevel(previous,linear,uv,0.).rgb;
    let c=textureSampleLevel(previous,linear,uv+vec2(1./256.,0.),0.).rgb;
    return vec4(quantize_color(mix(quantize_color(mix(a,b,127./255.)),c,85./255.)),1.);
}
@fragment fn blit(s:Screen)->@location(0) vec4<f32> {
    let uv=vec2(0.5)+(vec2(1.)-2.*s.uv.yx)*pan.crop.xy;
    return textureSampleLevel(previous,linear,uv,0.);
}
