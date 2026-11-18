@group(0) @binding(0) var atlas:texture_2d_array<f32>;
@group(0) @binding(1) var sprites:sampler;
@group(0) @binding(2) var tiles:texture_2d_array<f32>;
@group(0) @binding(3) var blocks:sampler;
struct Out{@builtin(position) pos:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) @interpolate(flat) layer:u32,@location(3) glint:vec4<f32>};
@vertex fn vs_main(@location(0) pos:vec2<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) layer:u32,@location(4) glint:vec4<f32>)->Out {var o:Out;o.pos=vec4(pos,0.0,1.0);o.uv=uv;o.color=color;o.layer=layer;o.glint=glint;return o;}
@fragment fn fs_main(i:Out)->@location(0) vec4<f32>{let index=i.layer & 65535u;let a=textureSample(atlas,sprites,i.uv,i32(min(index,textureNumLayers(atlas)-1u)));let b=textureSampleLevel(tiles,blocks,i.uv,i32(min(index,textureNumLayers(tiles)-1u)),0.0);var texel=select(a,b,(i.layer & 65536u)!=0u);if i.uv.x<0.0{texel=vec4(1.0);}var result=texel*i.color;
    if (i.layer & 2147483648u)!=0u {
        result=vec4(mix(result.rgb,vec3(1.0,0.0,0.0),0.3),result.a);
    }
    let glint=(i.layer & 2147483647u)>>17u;
    if glint!=0u{
        let a=textureSampleLevel(atlas,sprites,fract(i.glint.xy),i32(glint),0.0).rgb;
        let b=textureSampleLevel(atlas,sprites,fract(i.glint.zw),i32(glint),0.0).rgb;
        let color=vec3<f32>(128.0,64.0,204.0)/255.0;
        result=vec4<f32>(result.rgb+(a*a+b*b)*color*color,result.a);
    }
    return result;
}
