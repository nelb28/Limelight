// Difference overlay for a comparison panel's bar graph.
// For each bin, `heights` holds (reference panel, this panel) bar heights as fractions
// of the bar area, exactly as the bar shader draws them. Between the two curves:
//   this panel higher -> white (its excess height)
//   this panel lower  -> black (the reference's excess), with a grey line on top so it
//                        stays visible against the near-black background.

struct Params {
    num_bins: u32,
    max_height: f32,   // NDC height of the bar area (bars start at y = -1)
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> heights: array<vec2<f32>>; // (reference, this)

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) y: f32,          // NDC y of the fragment
    @location(1) ref_top: f32,    // NDC top of the reference curve at this x
    @location(2) this_top: f32,   // NDC top of this panel's curve at this x
}

const QUAD: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

// Instance ii covers the segment between bins ii and ii+1 over the full bar area.
// The curve tops are passed per vertex; they depend only on x, so interpolation across
// the quad reproduces the bar graph's straight segments exactly.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VertOut {
    let q = QUAD[vi];
    let bin = ii + u32(q.x);
    let h = heights[bin];
    let x = -1.0 + 2.0 * f32(bin) / f32(params.num_bins - 1u);
    let y = -1.0 + q.y * params.max_height;

    var out: VertOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.y = y;
    out.ref_top = -1.0 + h.x * params.max_height;
    out.this_top = -1.0 + h.y * params.max_height;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // About one pixel in NDC, for the grey edge line (computed before any branching).
    let px = fwidth(in.y);
    if in.this_top > in.ref_top && in.y > in.ref_top && in.y <= in.this_top {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    if in.ref_top > in.this_top && in.y > in.this_top && in.y <= in.ref_top {
        if in.ref_top - in.y < 1.5 * px {
            return vec4<f32>(0.55, 0.55, 0.55, 1.0);
        }
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    discard;
}
