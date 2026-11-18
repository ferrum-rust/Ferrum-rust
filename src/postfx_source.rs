//! Adapt the resource pack's original 1.20 GLSL stage declarations to Vulkan GLSL.
//! Program expressions, varyings, sample offsets, constants and pass order come from the pack.
use anyhow::{bail, ensure, Context, Result};
use std::{collections::BTreeMap, fmt::Write};

pub(super) const SLOTS: usize = 32;
pub(super) type Values = [[f32; 4]; SLOTS];
pub(super) struct ProgramInputs {
    pub values: Values,
    pub slots: BTreeMap<String, usize>,
}
impl ProgramInputs {
    pub fn new() -> Self {
        Self {
            values: [[0.; 4]; SLOTS],
            slots: BTreeMap::new(),
        }
    }
    pub fn slot(&mut self, name: &str) -> Result<usize> {
        let next = self.slots.len() + 1;
        let slot = *self.slots.entry(name.to_owned()).or_insert(next);
        ensure!(slot < SLOTS, "Too many post-processing uniforms");
        Ok(slot)
    }
}

fn declaration<'a>(line: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let mut words = line
        .strip_prefix(prefix)?
        .split(';')
        .next()?
        .split_whitespace();
    Some((words.next()?, words.next()?))
}

pub(super) fn stage(
    source: &str,
    vertex: bool,
    vars: &mut ProgramInputs,
    varyings: &mut BTreeMap<String, usize>,
    samplers: &[(String, bool)],
) -> Result<String> {
    let mut out = String::from("#version 450\n#define sample original_sample\nlayout(set=0,binding=0,std140) uniform Params { vec4 v[32]; } params;\n");
    if vertex {
        // Match the original GL_QUADS diagonal. The two possible diagonals round
        // some varying sample positions onto opposite texels in the mosaic passes.
        out.push_str("#define Position vec4(vec2((gl_VertexIndex & 1) != 0 ? params.v[0].x : 0.0, gl_VertexIndex < 2 ? params.v[0].y : 0.0), 500.0, 1.0)\n");
    } else {
        out.push_str("layout(location=0) out vec4 frag_color;\n#define gl_FragColor frag_color\n");
        for (i, (name, framebuffer)) in samplers.iter().enumerate() {
            writeln!(
                out,
                "layout(set=0,binding={}) uniform texture2D img{i};",
                i * 2 + 1
            )?;
            writeln!(
                out,
                "layout(set=0,binding={}) uniform sampler smp{i};",
                i * 2 + 2
            )?;
            if *framebuffer {
                // GL_NEAREST selects floor(uv * size) in bottom-up storage. Flip
                // the integer row after selecting it, including exact texel edges.
                writeln!(out, "vec4 {name}(vec2 uv) {{ float h=float(textureSize(sampler2D(img{i},smp{i}),0).y); float y=(h-0.5-floor(uv.y*h))/h; return textureLod(sampler2D(img{i},smp{i}),vec2(uv.x,y),0.0); }}")?;
            } else {
                writeln!(out, "vec4 {name}(vec2 uv) {{ return textureLod(sampler2D(img{i},smp{i}), uv, 0.0); }}")?;
            }
        }
        out.push_str("#define texture2D(tex,uv) tex(uv)\n");
    }
    let fxaa = source.contains("FxaaPixelShader(");
    let mut skip_offset_macros = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version") || trimmed.starts_with("#extension") {
            continue;
        }
        if fxaa && trimmed.starts_with("#if __VERSION__") {
            // Sampling via the UV helper converts GL's bottom-left origin before clamping.
            out.push_str("#define OffsetVec(a,b) vec2(a,b)\n#define FxaaTexOff(t,p,o,r) DiffuseSampler(p+o*r)\n");
            skip_offset_macros = true;
        }
        if skip_offset_macros {
            if trimmed == "#endif" {
                skip_offset_macros = false;
            }
            continue;
        }
        if fxaa && trimmed.starts_with("sampler2D tex,") {
            continue;
        }
        if let Some((ty, name)) = declaration(trimmed, "uniform ") {
            if ty == "sampler2D" {
                continue;
            }
            if ty == "mat4" && name == "ProjMat" {
                out.push_str("#define ProjMat mat4(2.0/params.v[0].x,0,0,0, 0,-2.0/params.v[0].y,0,0, 0,0,-0.0020001999,0, -1,1,-1.0001999,1)\n");
            } else {
                let suffix = match ty {
                    "float" => "x",
                    "vec2" => "xy",
                    "vec3" => "xyz",
                    "vec4" => "xyzw",
                    _ => bail!("Unsupported post uniform {ty} {name}"),
                };
                let slot = vars.slot(name)?;
                writeln!(out, "#define {name} params.v[{slot}].{suffix}")?;
            }
        } else if trimmed.starts_with("attribute ") {
            ensure!(
                trimmed == "attribute vec4 Position;",
                "Unsupported post vertex attribute {trimmed}"
            );
        } else if let Some((ty, name)) = declaration(trimmed, "varying ") {
            let slot = if vertex {
                let n = varyings.len();
                *varyings.entry(name.to_owned()).or_insert(n)
            } else {
                *varyings
                    .get(name)
                    .with_context(|| format!("Missing vertex output {name}"))?
            };
            writeln!(
                out,
                "layout(location={slot}) {} {ty} {name};",
                if vertex { "out" } else { "in" }
            )?;
        } else if fxaa {
            // This one original helper takes a combined sampler as an argument.
            // Its only call uses DiffuseSampler. Vulkan textures and samplers are separate.
            writeln!(
                out,
                "{}",
                line.replace("FxaaTex(tex,", "FxaaTex(DiffuseSampler,")
                    .replace(
                        "FxaaPixelShader(posPos, DiffuseSampler,",
                        "FxaaPixelShader(posPos,"
                    )
            )?;
        } else {
            writeln!(out, "{line}")?;
        }
    }
    Ok(out)
}
