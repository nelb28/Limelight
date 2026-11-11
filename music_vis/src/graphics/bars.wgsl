struct Params {
    num_bars: u32,
    global_max: f32,
    max_height: f32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> magnitudes: array<f32>;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
}

// Unit quad: two triangles covering [0,1] x [0,1]
const QUAD: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let hi = u32(floor(h * 6.0)) % 6u;
    let f  = h * 6.0 - floor(h * 6.0);
    let p  = v * (1.0 - s);
    let q  = v * (1.0 - f * s);
    let t  = v * (1.0 - (1.0 - f) * s);
    switch hi {
        case 0u: { return vec3<f32>(v, t, p); }
        case 1u: { return vec3<f32>(q, v, p); }
        case 2u: { return vec3<f32>(p, v, t); }
        case 3u: { return vec3<f32>(p, q, v); }
        case 4u: { return vec3<f32>(t, p, v); }
        default: { return vec3<f32>(v, p, q); }
    }
}

@vertex
fn vs_main(
    @builtin(vertex_index)   vi: u32,
    @builtin(instance_index) ii: u32,
) -> VertOut {
    let mag  = magnitudes[ii];
    let norm = clamp(mag / params.global_max, 0.0, 1.0);

    let bar_w = 2.0 / f32(params.num_bars);
    let gap   = bar_w * 0.08;                  // 8% gap on each side
    let left  = -1.0 + f32(ii) * bar_w + gap;
    let width = bar_w - 2.0 * gap;
    let height = norm * params.max_height;      // stays below the spectrogram

    let q = QUAD[vi];
    let x = left + q.x * width;
    let y = -1.0 + q.y * height;              // bottom-anchored

    // Hue: 0.667 (blue) for low-freq index 0, 0.0 (red) for high-freq
    let hue   = 0.667 * (1.0 - f32(ii) / f32(params.num_bars - 1u));
    let value = clamp(norm * 1.5, 0.08, 1.0); // slight glow even when quiet
    let color = hsv_to_rgb(hue, 0.85, value);

    var out: VertOut;
    out.pos   = vec4<f32>(x, y, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
