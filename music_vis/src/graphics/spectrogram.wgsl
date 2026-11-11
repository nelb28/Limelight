// Scrolling spectrograms of CWT frames centered on the playback position.
// x: time (center_frame ± half_window_frames), y: frequency bin (low at bottom).
// mode 0: color = magnitude (dB relative to the file's global maximum).
// mode 1: color = instantaneous-frequency deviation from the bin center
//         (blue = flat, white = on pitch, red = sharp), brightness = magnitude.

struct Params {
    center_frame: f32,       // fractional CWT frame at the playhead
    half_window_frames: f32, // frames shown on each side of the playhead
    num_frames: u32,
    num_scales: u32,
    global_max: f32,
    y_bottom: f32,           // NDC bottom edge of this view
    y_top: f32,              // NDC top edge of this view
    center_px: f32,          // framebuffer x of the panel center, for the marker
    db_range: f32,           // dB below global max that maps to black
    dev_range: f32,          // cents of deviation that map to full blue / red
    mode: u32,
    has_phase: u32,          // 0 for CWT1 files without phase data
    scale_mode: u32,         // 0 = dB, 1 = linear, 2 = sqrt
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> mags: array<f32>;      // frame-major
@group(0) @binding(2) var<storage, read> phase_dev: array<f32>; // frame-major, cents

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

const QUAD: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

const BACKGROUND: vec4<f32> = vec4<f32>(0.02, 0.02, 0.04, 1.0);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    let q = QUAD[vi];
    var out: VertOut;
    out.pos = vec4<f32>(-1.0 + 2.0 * q.x, mix(params.y_bottom, params.y_top, q.y), 0.0, 1.0);
    out.uv = q;
    return out;
}

// Approximation of the "inferno" colormap: black -> purple -> orange -> yellow.
fn inferno(v: f32) -> vec3<f32> {
    let c0 = vec3<f32>(0.0, 0.0, 0.02);
    let c1 = vec3<f32>(0.34, 0.06, 0.43);
    let c2 = vec3<f32>(0.73, 0.21, 0.33);
    let c3 = vec3<f32>(0.98, 0.55, 0.04);
    let c4 = vec3<f32>(0.99, 1.0, 0.64);
    if v < 0.25 { return mix(c0, c1, v / 0.25); }
    if v < 0.5  { return mix(c1, c2, (v - 0.25) / 0.25); }
    if v < 0.75 { return mix(c2, c3, (v - 0.5) / 0.25); }
    return mix(c3, c4, (v - 0.75) / 0.25);
}

// Diverging colormap for d in [-1, 1]: blue (flat) -> white (on pitch) -> red (sharp).
fn diverging(d: f32) -> vec3<f32> {
    let neutral = vec3<f32>(0.9, 0.9, 0.9);
    let flat    = vec3<f32>(0.15, 0.4, 1.0);
    let sharp   = vec3<f32>(1.0, 0.3, 0.1);
    if d < 0.0 { return mix(neutral, flat, -d); }
    return mix(neutral, sharp, d);
}

struct Sample {
    f0: u32, f1: u32, s0: u32, s1: u32, ft: f32, st: f32,
}

fn bilinear_mag(p: Sample) -> f32 {
    let n = params.num_scales;
    let a = mix(mags[p.f0 * n + p.s0], mags[p.f1 * n + p.s0], p.ft);
    let b = mix(mags[p.f0 * n + p.s1], mags[p.f1 * n + p.s1], p.ft);
    return mix(a, b, p.st);
}

fn bilinear_dev(p: Sample) -> f32 {
    let n = params.num_scales;
    let a = mix(phase_dev[p.f0 * n + p.s0], phase_dev[p.f1 * n + p.s0], p.ft);
    let b = mix(phase_dev[p.f0 * n + p.s1], phase_dev[p.f1 * n + p.s1], p.ft);
    return mix(a, b, p.st);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // Dashed center marker at the current frame.
    if abs(in.pos.x - params.center_px) < 1.0 && fract(in.pos.y / 12.0) < 0.5 {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }

    let frame = params.center_frame + (in.uv.x * 2.0 - 1.0) * params.half_window_frames;
    let last_frame = f32(params.num_frames - 1u);
    if frame < 0.0 || frame > last_frame {
        return BACKGROUND; // before start / after end of song
    }
    if params.mode == 1u && params.has_phase == 0u {
        return BACKGROUND; // old CWT1 file: no phase data
    }

    let scale = in.uv.y * f32(params.num_scales - 1u);
    var p: Sample;
    p.f0 = u32(floor(frame));
    p.f1 = min(p.f0 + 1u, params.num_frames - 1u);
    p.s0 = u32(floor(scale));
    p.s1 = min(p.s0 + 1u, params.num_scales - 1u);
    p.ft = fract(frame);
    p.st = fract(scale);

    let rel = max(bilinear_mag(p) / params.global_max, 0.0);
    var v: f32;
    if params.scale_mode == 1u {
        v = clamp(rel, 0.0, 1.0);
    } else if params.scale_mode == 2u {
        v = clamp(sqrt(rel), 0.0, 1.0);
    } else {
        let db = 20.0 * log(max(rel, 1e-9)) / log(10.0);
        v = clamp(1.0 + db / params.db_range, 0.0, 1.0);
    }

    if params.mode == 0u {
        return vec4<f32>(inferno(v), 1.0);
    }
    // Deviation is meaningless where there is no energy, so fade it out with magnitude.
    let d = clamp(bilinear_dev(p) / params.dev_range, -1.0, 1.0);
    return vec4<f32>(diverging(d) * v, 1.0);
}
