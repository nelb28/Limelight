struct Params {
    num_points: u32,
    amplitude_scale: f32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> samples: array<f32>;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) alpha: f32,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    let t = f32(vi) / f32(params.num_points - 1u);
    let x = -1.0 + 2.0 * t;
    let y = samples[vi] * params.amplitude_scale;

    var out: VertOut;
    out.pos   = vec4<f32>(x, y, 0.0, 1.0);
    out.alpha = 0.65;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 1.0, 1.0, in.alpha);
}
