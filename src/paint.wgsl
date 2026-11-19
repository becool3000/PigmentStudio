struct Dab {
    bounds: vec4<u32>,
    brush: vec4<f32>,
    axis: vec4<f32>,
    mode: vec4<u32>, // operation, brush index, pigment or source x, source y
    tubes: vec4<u32>, // one-based palette recipe
}
struct Pigments { lo: vec4<f32>, hi: vec4<f32> }
struct Band { k0: vec4<f32>, k1: vec4<f32>, s0: vec4<f32>, s1: vec4<f32>, xyz: vec4<f32> }
struct BrushState {
    loads: array<Pigments, 4>,
    transfer: array<Pigments, 3>,
    slots: array<u32, 8>,
    placement_ok: u32,
}
@group(0) @binding(0) var<storage, read_write> canvas: array<Pigments>;
@group(0) @binding(1) var<storage, read_write> state: BrushState;
@group(0) @binding(2) var<uniform> dab: Dab;
@group(0) @binding(3) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(4) var<storage, read> bands: array<Band>;
@group(0) @binding(5) var<storage, read_write> ink_step: array<vec2<f32>>;
var<workgroup> sampled: array<Pigments, 64>;
var<workgroup> weights: array<f32, 64>;
var<workgroup> occupied: array<atomic<u32>, 8>;

fn zero() -> Pigments { return Pigments(vec4<f32>(0.0), vec4<f32>(0.0)); }
fn add(a: Pigments, b: Pigments) -> Pigments { return Pigments(a.lo + b.lo, a.hi + b.hi); }
fn scale(a: Pigments, s: f32) -> Pigments { return Pigments(a.lo * s, a.hi * s); }
fn amount(a: Pigments) -> f32 { return dot(a.lo, vec4<f32>(1.0)) + a.hi.x + a.hi.y; }
fn pure(id: u32, mass: f32) -> Pigments {
    let ids = vec4<u32>(id);
    return Pigments(
        select(vec4<f32>(0.0), vec4<f32>(mass), ids == vec4<u32>(0u, 1u, 2u, 3u)),
        select(vec4<f32>(0.0), vec4<f32>(mass), ids == vec4<u32>(4u, 5u, 6u, 7u))
    );
}
fn index(pixel: vec2<u32>) -> u32 { return pixel.y * 1024u + pixel.x; }

fn tip(pixel: vec2<u32>) -> vec2<f32> {
    let p = (vec2<f32>(pixel) + vec2<f32>(0.5) - dab.brush.xy) / dab.brush.z;
    if (dab.mode.y == 0u) {
        return vec2<f32>(1.0 - smoothstep(0.3, 1.0, length(p)), 0.0);
    }
    let local = vec2<f32>(dot(p, dab.axis.xy), dot(p, vec2<f32>(-dab.axis.y, dab.axis.x)));
    let width = mix(0.83, 1.0, dab.brush.w);
    let across = local.x / width;
    let edge = max(abs(across), abs(local.y) / 0.36);
    let group = clamp(floor((across + 1.0) * 1.5), 0.0, 2.0);
    let ridge = mix(0.54 + 0.46 * smoothstep(0.08, 0.26, min(fract((across + 1.0) * 4.5), 1.0 - fract((across + 1.0) * 4.5))), 1.0, dab.brush.w);
    return vec2<f32>((1.0 - smoothstep(0.68, 1.0, edge)) * ridge, group);
}

fn pickup(w: f32) -> f32 {
    let canvas_rate = mix(0.075, 0.23, dab.brush.w);
    let palette_rate = mix(0.16, 0.43, dab.brush.w);
    return w * select(canvas_rate, palette_rate, dab.brush.y >= 1024.0);
}

@compute @workgroup_size(64)
fn gather(@builtin(local_invocation_index) i: u32, @builtin(workgroup_id) work: vec3<u32>) {
    let group = work.x;
    let count = dab.bounds.z * dab.bounds.w;
    var paint = zero();
    var area = 0.0;
    for (var k = i; k < count; k += 64u) {
        let pixel = dab.bounds.xy + vec2<u32>(k % dab.bounds.z, k / dab.bounds.z);
        let footprint = tip(pixel);
        if (u32(footprint.y) == group && footprint.x > 0.0) {
            let pixel_paint = canvas[index(pixel)];
            let oil = Pigments(pixel_paint.lo, vec4<f32>(pixel_paint.hi.xy, 0.0, 0.0));
            paint = add(paint, scale(oil, pickup(footprint.x)));
            area += footprint.x;
        }
    }
    sampled[i] = paint;
    weights[i] = area;
    workgroupBarrier();
    for (var step = 32u; step > 0u; step >>= 1u) {
        if (i < step) {
            sampled[i] = add(sampled[i], sampled[i + step]);
            weights[i] += weights[i + step];
        }
        workgroupBarrier();
    }
    if (i == 0u) {
        let load_index = dab.mode.y + group;
        let mixed = add(state.loads[load_index], sampled[0]);
        let area_sum = weights[0];
        let deposit = min(area_sum * mix(0.085, 0.25, dab.brush.w), amount(mixed) * mix(0.045, 0.12, dab.brush.w));
        let used_fraction = deposit / max(amount(mixed), 0.00000001);
        state.loads[load_index] = scale(mixed, 1.0 - used_fraction);
        state.transfer[group] = scale(mixed, used_fraction / max(area_sum, 0.00000001));
    }
}

@compute @workgroup_size(64)
fn choose_slots(@builtin(local_invocation_index) lane: u32) {
    if (lane < 8u) { atomicStore(&occupied[lane], 0u); }
    workgroupBarrier();
    for (var i = lane; i < 32768u; i += 64u) {
        let x = i % 512u;
        let y = i / 512u + 1024u;
        let relative = vec2<f32>(vec2<u32>(x % 64u, y - 1024u)) + vec2<f32>(0.5) - vec2<f32>(32.0);
        if (dot(relative, relative) < 361.0 && amount(canvas[index(vec2<u32>(x, y))]) > 0.003) {
            atomicStore(&occupied[x / 64u], 1u);
        }
    }
    workgroupBarrier();
    if (lane == 0u) {
        var required = 0u;
        for (var i = 0u; i < 4u; i++) {
            required += select(0u, 1u, dab.tubes[i] != 0u);
        }
        for (var i = 0u; i < 8u; i++) { state.slots[i] = 0u; }
        state.placement_ok = 0u;
        var span = 0u;
        for (var i = 0u; i < 8u; i++) {
            if (atomicLoad(&occupied[i]) != 0u) {
                span = 0u;
                continue;
            }
            span++;
            if (span == required) {
                for (var j = 0u; j < required; j++) {
                    state.slots[i + 1u - required + j] = dab.tubes[j];
                }
                state.placement_ok = 1u;
                break;
            }
        }
    }
}

fn srgb(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055,
                  12.92 * linear, linear <= vec3<f32>(0.0031308));
}

fn display_color(paint: Pigments) -> vec4<f32> {
    let total = amount(paint);
    if (total <= 0.00001) { return vec4<f32>(1.0); }
    var xyz = vec3<f32>(0.0);
    for (var i = 0u; i < 31u; i++) {
        let band = bands[i];
        let k = dot(paint.lo, band.k0) + dot(paint.hi, band.k1);
        let s = dot(paint.lo, band.s0) + dot(paint.hi, band.s1);
        let ratio = k / s;
        let reflectance = 1.0 / (1.0 + ratio + sqrt(ratio * (ratio + 2.0)));
        xyz += reflectance * band.xyz.xyz;
    }
    let linear = clamp(vec3<f32>(
        dot(xyz, vec3<f32>(3.2404542, -1.5371385, -0.4985314)),
        dot(xyz, vec3<f32>(-0.969266, 1.8760108, 0.041556)),
        dot(xyz, vec3<f32>(0.0556434, -0.2040259, 1.0572252))
    ), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(srgb(mix(vec3<f32>(1.0), linear, 1.0 - exp(-4.0 * total))), 1.0);
}

@compute @workgroup_size(8, 8)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let at = index(pixel);
    var paint = canvas[at];
    if (dab.mode.x == 0u) {
            let footprint = tip(pixel);
            let w = footprint.x;
            if (w <= 0.0) { return; }
            let ink = paint.hi.zw;
            paint = add(scale(paint, 1.0 - pickup(w)), scale(state.transfer[u32(footprint.y)], w));
            paint.hi = vec4<f32>(paint.hi.xy, ink);
            canvas[at] = paint;
        } else if (dab.mode.x == 2u) {
            let tube = state.slots[pixel.x / 64u];
            if (tube == 0u) { return; }
            let delta = vec2<f32>(vec2<u32>(pixel.x % 64u, pixel.y - 1024u)) + vec2<f32>(0.5) - vec2<f32>(32.0);
            let weight = 1.0 - smoothstep(11.0, 19.0, length(delta));
            if (weight == 0.0) { return; }
            paint = add(paint, pure(tube - 1u, 4.5 * weight));
            canvas[at] = paint;
        } else if (dab.mode.x == 3u) {
            paint = canvas[index(dab.mode.zw)];
            canvas[at] = paint;
        } else if (dab.mode.x == 4u) {
            paint = zero();
            canvas[at] = paint;
        } else if (dab.mode.x == 7u) {
            let stripe = (pixel.x / 79u + pixel.y / 103u) % 3u;
            paint = pure(stripe + 1u, 0.23);
            canvas[at] = paint;
        }
    else if (dab.mode.x == 9u) {
        let p = length(vec2<f32>(pixel) + vec2<f32>(0.5) - dab.brush.xy) / dab.brush.z;
        let w = 1.0 - smoothstep(0.26, 1.0, p);
        let pressure = dab.brush.w;
        let flood = smoothstep(0.56, 0.98, pressure);
        let dark = w * mix(0.12, 0.37, pressure);
        let wet = w * flood * select(0.020, 0.37, dab.mode.z != 0u);
        paint.hi = paint.hi + vec4<f32>(0.0, 0.0, dark, wet);
        canvas[at] = paint;
    } else if (dab.mode.x == 10u) {
        paint.hi = vec4<f32>(paint.hi.xy, ink_step[at]);
        canvas[at] = paint;
    }
}

fn runoff(wet: f32, y: u32) -> f32 {
    return select(max(wet - 0.11, 0.0) * 0.82, 0.0, y == 1023u);
}

@compute @workgroup_size(8, 8)
fn flow_ink(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let at = index(pixel);
    let previous = canvas[at].hi.zw;
    var wet = previous.y - runoff(previous.y, pixel.y);
    if (pixel.y > 0u) {
        wet += runoff(canvas[at - 1024u].hi.w, pixel.y - 1u);
    }
    let absorb = min(wet, wet * 0.035 + 0.006);
    ink_step[at] = vec2<f32>(previous.x + absorb, max(wet - absorb, 0.0));
}

@compute @workgroup_size(8, 8)
fn display(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let pigment = canvas[index(pixel)];
    let base = display_color(pigment);
    let ink = 1.0 - exp(-(pigment.hi.z + pigment.hi.w) * 3.8);
    let color = mix(base.rgb, vec3<f32>(0.024, 0.031, 0.048), ink);
    textureStore(output, vec2<i32>(pixel), vec4<f32>(color, 1.0));
}

@compute @workgroup_size(4)
fn service(@builtin(local_invocation_index) lane: u32) {
    let first = dab.mode.y;
    let count = select(3u, 1u, first == 0u);
    if (lane >= count) { return; }
    let i = first + lane;
    if (dab.mode.x == 5u) {
        state.loads[i] = add(state.loads[i], pure(dab.mode.z, dab.brush.w / f32(count)));
    } else {
        state.loads[i] = scale(state.loads[i], dab.brush.w);
    }
}

@compute @workgroup_size(4)
fn preview(@builtin(local_invocation_index) lane: u32) {
    let paint = state.loads[lane];
    let capacity = select(410.0, 1230.0, lane == 0u);
    let fill = 1.0 - exp(-amount(paint) / capacity);
    let color = display_color(scale(paint, 2.0 / max(amount(paint), 0.00001)));
    textureStore(output, vec2<i32>(512 + i32(lane), 1024), vec4<f32>(color.rgb * fill, fill));
    if (lane == 0u && dab.mode.x == 2u) {
        textureStore(output, vec2<i32>(528, 1024), vec4<f32>(f32(state.placement_ok), 0.0, 0.0, 1.0));
    }
}
