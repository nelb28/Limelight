struct Params {
    num_bars: u32,
    global_max: f32,
    max_height: f32,      // NDC height of the bar area
    reference_frac: f32,  // fraction of the area reached at the reference level
    pitch_offset: f32,    // semitones above C of bin 0 (A4 = 440 Hz)
    bins_per_octave: f32,
    color_mode: u32,      // 0 = by position (blue -> red), 1 = by note
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> magnitudes: array<f32>;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
}

// Unit quad: two triangles covering [0,1] x [0,1] (x: segment end, y: bottom/top)
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

// The spectrum is drawn as one continuous filled area: instance `ii` is the segment
// between bins ii and ii+1, a quad whose top edge runs from one bin's height to the
// next. Colors are computed per bin and blend smoothly across each segment.
@vertex
fn vs_main(
    @builtin(vertex_index)   vi: u32,
    @builtin(instance_index) ii: u32,
) -> VertOut {
    let q   = QUAD[vi];
    let bin = ii + u32(q.x);                    // left or right end of the segment

    let rel  = max(magnitudes[bin] / params.global_max, 0.0);
    let norm = min(rel, 1.0);                   // color saturates at the reference level
    // The reference level sits below the top of the area, so louder peaks show their
    // real height in the headroom instead of being cut off flat.
    let height = min(rel * params.reference_frac, 1.0) * params.max_height;

    let x = -1.0 + 2.0 * f32(bin) / f32(params.num_bars - 1u);
    let y = -1.0 + q.y * height;                // bottom-anchored

    // Hue: 0.667 (blue) for low-freq bin 0, 0.0 (red) for the highest bin
    var hue = 0.667 * (1.0 - f32(bin) / f32(params.num_bars - 1u));
    if params.color_mode == 1u {
        // Pitch class: C = 0 (red), around the color wheel by semitone.
        hue = fract((params.pitch_offset + 12.0 * f32(bin) / params.bins_per_octave) / 12.0);
    }
    let value = clamp(norm * 1.5, 0.08, 1.0);  // slight glow even when quiet

    var out: VertOut;
    out.pos   = vec4<f32>(x, y, 0.0, 1.0);
    out.color = hsv_to_rgb(hue, 0.85, value);
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
