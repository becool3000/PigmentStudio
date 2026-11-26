struct Dab {
    bounds: vec4<u32>,
    brush: vec4<f32>,
    axis: vec4<f32>,
    mode: vec4<u32>, // operation, brush index, pigment or source x, source y
    tubes: vec4<u32>, // one-based palette recipe
    sweep: vec4<f32>, // brush displacement, end radius and end pressure
}
struct Pigments { lo: vec4<f32>, hi: vec4<f32> }
struct Band { k0: vec4<f32>, k1: vec4<f32>, s0: vec4<f32>, s1: vec4<f32>, xyz: vec4<f32> }
struct BrushState {
    loads: array<Pigments, 14>,
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
@group(0) @binding(6) var<storage, read_write> ink_canvas: array<vec2<f32>>;
@group(0) @binding(7) var<storage, read_write> dry_canvas: array<Pigments>;

struct DrySubstrate {
    r0: vec4<u32>,
    r1: vec4<u32>,
    r2: vec4<u32>,
    r3: vec4<u32>,
};

@group(0) @binding(8) var<storage, read_write> dry_substrate: array<DrySubstrate>;
@group(0) @binding(9) var<storage, read_write> reference_layer: array<u32>;
@group(0) @binding(10) var<storage, read_write> sketch_canvas: array<f32>;
@group(0) @binding(11) var<storage, read_write> paint_step: array<Pigments>;
var<workgroup> sampled: array<Pigments, 64>;
var<workgroup> weights: array<f32, 64>;
var<workgroup> occupied: array<atomic<u32>, 8>;

fn zero() -> Pigments { return Pigments(vec4<f32>(0.0), vec4<f32>(0.0)); }
fn add(a: Pigments, b: Pigments) -> Pigments { return Pigments(a.lo + b.lo, a.hi + b.hi); }
fn scale(a: Pigments, s: f32) -> Pigments { return Pigments(a.lo * s, a.hi * s); }
fn amount(a: Pigments) -> f32 { return dot(a.lo, vec4<f32>(1.0)) + dot(a.hi, vec4<f32>(1.0)); }
fn pure(id: u32, mass: f32) -> Pigments {
    let ids = vec4<u32>(id);
    return Pigments(
        select(vec4<f32>(0.0), vec4<f32>(mass), ids == vec4<u32>(0u, 1u, 2u, 3u)),
        select(vec4<f32>(0.0), vec4<f32>(mass), ids == vec4<u32>(4u, 5u, 6u, 7u))
    );
}
fn index(pixel: vec2<u32>) -> u32 { return pixel.y * 1024u + pixel.x; }

fn wrap_canvas_pixel(raw: vec2<u32>, wrap: bool) -> vec2<u32> {
    return select(raw, raw % 1024u, wrap);
}

fn tip_delta(pixel: vec2<u32>) -> vec2<f32> {
    var delta = vec2<f32>(pixel) + vec2<f32>(0.5) - dab.brush.xy;
    if (dab.mode.w != 0u) {
        delta = delta - round(delta / 1024.0) * 1024.0;
    }
    return delta;
}

fn filbert_contact(across: f32, along: f32, pressure: f32, tooth: f32) -> f32 {
    let oval = vec2<f32>(across, along / 0.52);
    let edge = 1.0 - smoothstep(0.22, 1.0, dot(oval, oval));
    let rhythm = (across + 1.0) * 4.2 + 0.18 * sin(across * 8.0);
    let groove = smoothstep(0.07, 0.29, abs(fract(rhythm) - 0.5));
    let bristles = mix(mix(0.43, 0.76, pressure), 1.0, groove);
    let light_contact = mix(0.28, 1.0, tooth);
    let contact = mix(light_contact * 0.26, 1.0, smoothstep(0.06, 0.56, pressure));
    return edge * bristles * contact;
}

fn fan_contact(across: f32, along: f32, pressure: f32, tooth: f32) -> f32 {
    let spread = smoothstep(0.10, 0.88, pressure);
    let tuft_phase = (across + 1.0) * 4.5 + 0.11 * sin(across * 6.0);
    let gap = abs(fract(tuft_phase) - 0.5);
    let half_width = mix(0.16, 0.51, spread);
    let tuft = 1.0 - smoothstep(half_width - 0.12, half_width + 0.02, gap);
    let overlap = mix(0.0, 0.62, spread);
    let arc = along + 0.22 * across * across;
    let depth = mix(0.20, 0.29, pressure);
    let edge = (1.0 - smoothstep(0.84, 1.0, abs(across))) * (1.0 - smoothstep(0.16, 1.0, abs(arc) / depth));
    let contact = mix(0.24, 1.0, smoothstep(0.04, 0.64, pressure));
    let grain = mix(mix(0.64, 1.0, tooth), 1.0, spread);
    return edge * mix(tuft, 1.0, overlap) * contact * grain;
}

fn tip(pixel: vec2<u32>) -> vec2<f32> {
    let p = tip_delta(pixel) / dab.brush.z;
    let profile = dab.mode.z;
    if (profile == 0u) {
        let contact = select(1.0, dab.axis.w, dab.axis.w > 0.0);
        return vec2<f32>((1.0 - smoothstep(0.3, 1.0, length(p))) * contact, 0.0);
    }
    if (profile == 5u) {
        // Soft Mop: wide, feathered radial blender
        return vec2<f32>(1.0 - smoothstep(0.15, 1.0, length(p)), 0.0);
    }

    let local = vec2<f32>(dot(p, dab.axis.xy), dot(p, vec2<f32>(-dab.axis.y, dab.axis.x)));
    let width = mix(select(0.83, 0.70, profile == 2u && dab.axis.w > 0.0), 1.0, dab.brush.w);
    let across = local.x / width;
    let group = clamp(floor((across + 1.0) * 1.5), 0.0, 2.0);
    let furrow = smoothstep(0.08, 0.28, min(fract((across + 1.0) * 4.5), 1.0 - fract((across + 1.0) * 4.5)));

    let weave_tooth = clamp((cos(f32(pixel.x) * 1.9635) - cos(f32(pixel.y) * 1.9635)) * 0.5 + 0.5, 0.0, 1.0);
    let dry_skip = smoothstep(0.0, 0.28, dab.axis.z);
    let tooth_mask = mix(smoothstep(0.35, 0.72, weave_tooth), 1.0, dry_skip);

    if (profile == 4u) {
        // Palette Knife: crisp steel blade edge with trailing ridge bias
        let along = abs(local.y) / 0.22;
        let across_edge = 1.0 - smoothstep(0.86, 1.0, abs(across));
        let square = min(across_edge, 1.0 - smoothstep(0.86, 1.0, along));
        // Give consecutive moving dabs a gradual overlap along the travel axis.
        let continuous = across_edge * (1.0 - smoothstep(0.18, 1.0, along));
        let blade = select(square, continuous, dab.axis.w > 0.0);
        let ridge = 0.85 + 0.15 * smoothstep(-0.22, 0.22, -local.y);
        return vec2<f32>(blade * ridge, group);
    }

    if (profile == 2u) {
        // Filbert: almond/oval tip with soft tapered corners and bristle grooves
        if (dab.axis.w > 0.0) {
            return vec2<f32>(filbert_contact(across, local.y, dab.brush.w, smoothstep(0.3, 0.83, weave_tooth)), group);
        }
        let edge = length(vec2<f32>(across, local.y / 0.52));
        let shape = 1.0 - smoothstep(0.55, 1.0, edge);
        let bristles = 0.58 + 0.42 * furrow;
        return vec2<f32>(shape * bristles * tooth_mask, group);
    }

    if (profile == 3u) {
        // Fan Brush: curved arc of 9 separated bristle tufts
        if (dab.axis.w > 0.0) {
            return vec2<f32>(fan_contact(across, local.y, dab.brush.w, smoothstep(0.3, 0.83, weave_tooth)), group);
        }
        let arc_y = local.y + 0.22 * across * across;
        let edge = max(abs(across), abs(arc_y) / 0.26);
        let shape = 1.0 - smoothstep(0.65, 1.0, edge);
        let tuft = pow(abs(sin(across * 14.137)), mix(2.2, 0.75, dab.brush.w));
        return vec2<f32>(shape * tuft * tooth_mask, group);
    }

    // Upgraded Hog-Bristle Flat: crisp corners + persistent deep bristle furrows
    let edge = max(abs(across), abs(local.y) / 0.36);
    let ridge = mix(0.42 + 0.58 * furrow, 0.72 + 0.28 * furrow, dab.brush.w);
    return vec2<f32>((1.0 - smoothstep(0.76, 1.0, edge)) * ridge * tooth_mask, group);
}

fn sweep_distance() -> f32 {
    return length(dab.sweep.xy) / (0.125 * (dab.brush.z + dab.sweep.z));
}

fn hog_bristle_contact(across: f32, along: f32, pressure: f32, tooth: f32) -> f32 {
    let clumps = (across + 1.0) * 4.6 + 0.23 * sin(across * 8.7);
    let gap = abs(fract(clumps) - 0.5);
    let bundle = 1.0 - smoothstep(0.22, 0.48, gap);
    let stiffness = 0.07 + 0.17 * (0.5 + 0.5 * sin(floor(clumps) * 2.4));
    let light_touch = bundle * smoothstep(stiffness, stiffness + 0.24, pressure + 0.10 * tooth);
    let full_contact = mix(0.68, 1.0, bundle);
    let bristles = mix(light_touch * mix(0.20, 1.0, tooth), full_contact, smoothstep(0.20, 0.68, pressure));
    let edge = (1.0 - smoothstep(0.82, 1.0, abs(across))) * (1.0 - smoothstep(0.18, 1.0, abs(along) / 0.36));
    return edge * bristles;
}

// Integrate directional contact over a short segment. Each quadrature
// point contributes to the lanes it crosses, so turning keeps the split load.
fn moving_contact(delta: vec2<f32>, t: f32, tooth: f32) -> vec3<f32> {
    let p = (delta - (t - 0.5) * dab.sweep.xy) / mix(dab.brush.z, dab.sweep.z, t);
    let axis = normalize(mix(dab.axis.xy, dab.axis.zw, t));
    let pressure = mix(dab.brush.w, dab.sweep.w, t);
    let filbert = dab.mode.z == 2u;
    let across = dot(p, axis) / mix(select(0.83, 0.70, filbert), 1.0, pressure);
    let along = dot(p, vec2<f32>(-axis.y, axis.x));
    let flat = dab.mode.z == 1u;
    let fan = dab.mode.z == 3u;
    var contact: f32;
    if (flat) {
        contact = hog_bristle_contact(across, along, pressure, tooth);
    } else if (filbert) {
        contact = filbert_contact(across, along, pressure, tooth);
    } else if (fan) {
        contact = fan_contact(across, along, pressure, tooth);
    } else {
        let sides = 1.0 - smoothstep(0.86, 1.0, abs(across));
        let travel = 1.0 - smoothstep(0.18, 1.0, abs(along) / 0.22);
        let ridge = 0.85 + 0.15 * smoothstep(-0.22, 0.22, -along);
        contact = sides * travel * ridge;
    }
    let blend = select(0.05, 0.10, flat || filbert || fan);
    let center = smoothstep(-0.33 - blend, -0.33 + blend, across);
    let far = smoothstep(0.33 - blend, 0.33 + blend, across);
    return vec3<f32>(1.0 - center, center - far, far) * contact;
}

fn swept_exposure(pixel: vec2<u32>) -> vec3<f32> {
    let delta = tip_delta(pixel);
    var tooth = 1.0;
    if (dab.mode.z == 1u || dab.mode.z == 2u || dab.mode.z == 3u) {
        let weave = (cos(f32(pixel.x) * 1.9635) - cos(f32(pixel.y) * 1.9635)) * 0.5 + 0.5;
        tooth = smoothstep(0.3, 0.83, weave);
    }
    let ends = moving_contact(delta, 0.0694318, tooth) + moving_contact(delta, 0.9305682, tooth);
    let middle = moving_contact(delta, 0.3300095, tooth) + moving_contact(delta, 0.6699905, tooth);
    return (ends * 0.1739274 + middle * 0.3260726) * sweep_distance();
}

fn pickup_rate(rate: f32, exposure: f32) -> f32 {
    if (dab.sweep.z > 0.0) {
        return 1.0 - pow(1.0 - rate, exposure);
    }
    return rate * exposure;
}

fn pickup(w: f32) -> f32 {
    let pressure = select(dab.brush.w, 0.5 * (dab.brush.w + dab.sweep.w), dab.sweep.z > 0.0);
    var canvas_rate = mix(0.075, 0.23, pressure);
    if (dab.mode.z == 4u) {
        canvas_rate = mix(0.45, 0.80, pressure);
    } else if (dab.mode.z == 5u) {
        canvas_rate = mix(0.28, 0.52, dab.brush.w);
    }
    let palette_rate = mix(0.16, 0.43, pressure);
    return pickup_rate(select(canvas_rate, palette_rate, dab.brush.y >= 1024.0), w);
}

fn swept_transfer_weights(pixel: vec2<u32>) -> vec3<f32> {
    let exposure = swept_exposure(pixel);
    let total = dot(exposure, vec3<f32>(1.0));
    // Apply the same distance-dependent factor to pickup and replenishment.
    // For a fully loaded brush, subdividing a stroke then gives the same film.
    return exposure * (pickup(total) / (max(total, 0.00000001) * pickup(1.0)));
}

@compute @workgroup_size(64)
fn gather(@builtin(local_invocation_index) i: u32, @builtin(workgroup_id) work: vec3<u32>) {
    let group = work.x;
    let count = dab.bounds.z * dab.bounds.w;
    var paint = zero();
    var area = 0.0;
    for (var k = i; k < count; k += 64u) {
        let pixel = wrap_canvas_pixel(
            dab.bounds.xy + vec2<u32>(k % dab.bounds.z, k / dab.bounds.z),
            dab.mode.w != 0u
        );
        if (dab.sweep.z > 0.0) {
            let lane = swept_transfer_weights(pixel)[group];
            if (lane > 0.0) {
                paint = add(paint, scale(canvas[index(pixel)], pickup(1.0) * lane));
                area += lane;
            }
        } else {
            let footprint = tip(pixel);
            if (u32(footprint.y) == group && footprint.x > 0.0) {
                paint = add(paint, scale(canvas[index(pixel)], pickup(footprint.x)));
                area += footprint.x;
            }
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
        let pressure = select(dab.brush.w, 0.5 * (dab.brush.w + dab.sweep.w), dab.sweep.z > 0.0);
        let deposit_rate = select(
            mix(0.085, 0.25, pressure),
            mix(0.14, 0.38, pressure),
            dab.mode.z == 4u
        );
        var load_fraction = mix(0.045, 0.12, pressure);
        if (dab.sweep.z > 0.0) {
            load_fraction = 1.0 - pow(1.0 - load_fraction, sweep_distance());
        }
        let deposit = min(area_sum * deposit_rate, amount(mixed) * load_fraction);
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

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)),
                  c / 12.92, c <= vec3<f32>(0.04045));
}

fn diffuse_reflectance(r: f32) -> f32 {
    return 0.4 * r / max(1.0 - 0.60 * r, 0.00001);
}

fn xyz_to_linear_rgb(xyz: vec3<f32>) -> vec3<f32> {
    return clamp(vec3<f32>(
        dot(xyz, vec3<f32>(3.2404542, -1.5371385, -0.4985314)),
        dot(xyz, vec3<f32>(-0.969266, 1.8760108, 0.041556)),
        dot(xyz, vec3<f32>(0.0556434, -0.2040259, 1.0572252))
    ), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The wet film follows K-M over black. For a stack with dried paint, use the
// combined pigment as an estimate because the dry substrate stores one backing.
fn paint_over_black(paint: Pigments) -> vec3<f32> {
    if (amount(paint) <= 0.00001) { return vec3<f32>(0.0); }
    var xyz = vec3<f32>(0.0);
    for (var i = 0u; i < 31u; i++) {
        let band = bands[i];
        let k_tot = dot(paint.lo, band.k0) + dot(paint.hi, band.k1);
        let s_tot = dot(paint.lo, band.s0) + dot(paint.hi, band.s1);
        let s_safe = max(s_tot, 0.000001);
        let r = k_tot / s_safe;
        let a = 1.0 + r;
        let b = sqrt(r * (r + 2.0));
        let r_inf = 1.0 / (a + b);
        let u = sqrt(k_tot * (k_tot + 2.0 * s_tot));
        let e2 = exp(-2.0 * u);

        let r_int_b = clamp((r_inf * (1.0 - e2)) / max(1.0 - e2 * r_inf * r_inf, 0.00001), 0.0, 1.0);
        xyz += diffuse_reflectance(r_int_b) * band.xyz.xyz;
    }
    return xyz_to_linear_rgb(xyz);
}

fn composite_linear_color(wet: Pigments, at: u32, is_canvas: bool) -> vec3<f32> {
    let wet_total = amount(wet);
    if (!is_canvas && wet_total <= 0.00001) { return vec3<f32>(1.0); }

    var sub: DrySubstrate;
    var words: array<u32, 16>;
    if (is_canvas) {
        sub = dry_substrate[at];
        if (wet_total <= 0.00001
            && all(sub.r0 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r1 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r2 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r3 == vec4<u32>(0xFFFFFFFFu))) {
            return vec3<f32>(1.0);
        }
        words = array<u32, 16>(
            sub.r0.x, sub.r0.y, sub.r0.z, sub.r0.w,
            sub.r1.x, sub.r1.y, sub.r1.z, sub.r1.w,
            sub.r2.x, sub.r2.y, sub.r2.z, sub.r2.w,
            sub.r3.x, sub.r3.y, sub.r3.z, sub.r3.w
        );
    }

    var xyz = vec3<f32>(0.0);
    for (var w = 0u; w < 16u; w++) {
        var r_sub2 = vec2<f32>(1.0);
        if (is_canvas) {
            r_sub2 = unpack2x16unorm(words[w]);
        }
        for (var c = 0u; c < 2u; c++) {
            let i = w * 2u + c;
            if (i >= 31u) { break; }
            let band = bands[i];
            let r_sub = r_sub2[c];
            var r_int = r_sub;
            if (wet_total > 0.00001) {
                let k_wet = dot(wet.lo, band.k0) + dot(wet.hi, band.k1);
                let s_wet = dot(wet.lo, band.s0) + dot(wet.hi, band.s1);
                let s_safe = max(s_wet, 0.000001);
                let r = k_wet / s_safe;
                let a = 1.0 + r;
                let b = sqrt(r * (r + 2.0));
                let r_inf = 1.0 / (a + b);
                let u = sqrt(k_wet * (k_wet + 2.0 * s_wet));
                let e2 = exp(-2.0 * u);

                let denom = max(1.0 - r_sub * r_inf, 0.00001);
                let d_val = (r_inf - r_sub) / denom;
                let d_denom = max(1.0 - e2 * r_inf * d_val, 0.00001);
                r_int = clamp((r_inf - e2 * d_val) / d_denom, 0.0, 1.0);
            }

            // Saunderson surface boundary correction (SPEX diffuse body reflectance)
            xyz += diffuse_reflectance(r_int) * band.xyz.xyz;
        }
    }
    return xyz_to_linear_rgb(xyz);
}

@compute @workgroup_size(8, 8)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let wrap = dab.mode.w != 0u && (dab.mode.x == 0u || dab.mode.x == 9u || dab.mode.x == 15u || dab.mode.x == 18u);
    let pixel = wrap_canvas_pixel(dab.bounds.xy + id.xy, wrap);
    let at = index(pixel);
    var paint = canvas[at];
    if (dab.mode.x == 0u) {
        if (dab.sweep.z > 0.0) {
            let weights = swept_transfer_weights(pixel);
            let w = dot(weights, vec3<f32>(1.0));
            if (w <= 0.0) { return; }
            let transferred = add(scale(state.transfer[0], weights.x), scale(state.transfer[1], weights.y));
            let deposited = add(transferred, scale(state.transfer[2], weights.z));
            paint = add(scale(paint, max(0.0, 1.0 - pickup(1.0) * w)), deposited);
        } else {
            let footprint = tip(pixel);
            let w = footprint.x;
            if (w <= 0.0) { return; }
            paint = add(scale(paint, 1.0 - pickup(w)), scale(state.transfer[u32(footprint.y)], w));
        }
        canvas[at] = paint;
    } else if (dab.mode.x == 2u) {
            let tube = state.slots[pixel.x / 64u];
            if (tube == 0u) { return; }
            let delta = vec2<f32>(vec2<u32>(pixel.x % 64u, pixel.y - 1024u)) + vec2<f32>(0.5) - vec2<f32>(32.0);
            let weight = 1.0 - smoothstep(11.0, 19.0, length(delta));
            if (weight == 0.0) { return; }
            if (tube < 100u) {
                paint = add(paint, pure(tube - 1u, 45.0 * weight));
            } else if (tube < 200u) {
                let custom_slot = tube - 100u;
                let source_paint = canvas[index(vec2<u32>(544u + custom_slot, 1024u))];
                let src_amt = amount(source_paint);
                if (src_amt > 0.0001) {
                    paint = add(paint, scale(source_paint, (45.0 * weight) / src_amt));
                }
            } else {
                let rack_slot = tube - 200u;
                let source_paint = canvas[index(vec2<u32>(608u + rack_slot, 1024u))];
                let src_amt = amount(source_paint);
                if (src_amt > 0.0001) {
                    paint = add(paint, scale(source_paint, (45.0 * weight) / src_amt));
                }
            }
            canvas[at] = paint;
        } else if (dab.mode.x == 3u) {
            paint = canvas[index(dab.mode.zw)];
            if (dab.mode.w < 1024u) {
                paint = add(paint, dry_canvas[index(dab.mode.zw)]);
            }
            canvas[at] = paint;
        } else if (dab.mode.x == 4u || dab.mode.x == 1u) {
            paint = zero();
            canvas[at] = paint;
            if (pixel.y < 1024u) {
                ink_canvas[at] = vec2<f32>(0.0);
                dry_canvas[at] = zero();
                dry_substrate[at] = DrySubstrate(
                    vec4<u32>(0xFFFFFFFFu),
                    vec4<u32>(0xFFFFFFFFu),
                    vec4<u32>(0xFFFFFFFFu),
                    vec4<u32>(0xFFFFFFFFu)
                );
            }
        } else if (dab.mode.x == 14u) {
            if (pixel.y < 1024u) {
                let wet = canvas[at];
                let wet_total = amount(wet);
                if (wet_total > 0.00001) {
                    let sub = dry_substrate[at];
                    let words = array<u32, 16>(
                        sub.r0.x, sub.r0.y, sub.r0.z, sub.r0.w,
                        sub.r1.x, sub.r1.y, sub.r1.z, sub.r1.w,
                        sub.r2.x, sub.r2.y, sub.r2.z, sub.r2.w,
                        sub.r3.x, sub.r3.y, sub.r3.z, sub.r3.w
                    );
                    var new_words: array<u32, 16>;
                    for (var w = 0u; w < 16u; w++) {
                        let r_sub2 = unpack2x16unorm(words[w]);
                        var r_new2 = vec2<f32>(1.0);
                        for (var c = 0u; c < 2u; c++) {
                            let i = w * 2u + c;
                            if (i >= 31u) {
                                r_new2[c] = 1.0;
                                continue;
                            }
                            let band = bands[i];
                            let r_sub = r_sub2[c];
                            let k_wet = dot(wet.lo, band.k0) + dot(wet.hi, band.k1);
                            let s_wet = dot(wet.lo, band.s0) + dot(wet.hi, band.s1);
                            let s_safe = max(s_wet, 0.000001);
                            let r = k_wet / s_safe;
                            let a = 1.0 + r;
                            let b = sqrt(r * (r + 2.0));
                            let r_inf = 1.0 / (a + b);
                            let u = sqrt(k_wet * (k_wet + 2.0 * s_wet));
                            let e2 = exp(-2.0 * u);

                            let denom = max(1.0 - r_sub * r_inf, 0.00001);
                            let d_val = (r_inf - r_sub) / denom;
                            let d_denom = max(1.0 - e2 * r_inf * d_val, 0.00001);
                            r_new2[c] = clamp((r_inf - e2 * d_val) / d_denom, 0.0, 1.0);
                        }
                        new_words[w] = pack2x16unorm(r_new2);
                    }
                    dry_substrate[at] = DrySubstrate(
                        vec4<u32>(new_words[0], new_words[1], new_words[2], new_words[3]),
                        vec4<u32>(new_words[4], new_words[5], new_words[6], new_words[7]),
                        vec4<u32>(new_words[8], new_words[9], new_words[10], new_words[11]),
                        vec4<u32>(new_words[12], new_words[13], new_words[14], new_words[15])
                    );
                }
                dry_canvas[at] = add(dry_canvas[at], canvas[at]);
                canvas[at] = zero();
            }
        } else if (dab.mode.x == 13u) {
            paint = Pigments(
                vec4<f32>(dab.brush.x, dab.brush.y, dab.brush.z, dab.brush.w),
                vec4<f32>(dab.axis.x, dab.axis.y, dab.axis.z, dab.axis.w)
            );
            canvas[at] = paint;
        } else if (dab.mode.x == 15u) {
            if (pixel.y < 1024u) {
                var delta = vec2<f32>(pixel) + vec2<f32>(0.5) - dab.brush.xy;
                if (dab.mode.w != 0u) {
                    delta = delta - round(delta / 1024.0) * 1024.0;
                }
                let dist = length(delta) / max(dab.brush.z, 0.5);
                let falloff = 1.0 - smoothstep(0.25, 1.0, dist);
                if (falloff > 0.0) {
                    let fx = f32(pixel.x) * 1.9635;
                    let fy = f32(pixel.y) * 1.9635;
                    let tooth = 0.55 + 0.45 * clamp((cos(fx) - cos(fy)) * 0.5 + 0.5, 0.0, 1.0);
                    let pressure = dab.brush.w;
                    let grain = mix(tooth, 1.0, pressure * 0.65);
                    if (dab.axis.x >= 0.0) {
                        let deposit = falloff * grain * pressure * 0.45;
                        sketch_canvas[at] = clamp(sketch_canvas[at] + deposit * (1.0 - sketch_canvas[at] * 0.6), 0.0, 1.0);
                    } else {
                        let erase = falloff * (0.35 + 0.65 * pressure);
                        sketch_canvas[at] = max(0.0, sketch_canvas[at] * (1.0 - erase));
                    }
                }
            }
        } else if (dab.mode.x == 16u) {
            if (pixel.y < 1024u) {
                sketch_canvas[at] = 0.0;
            }
        } else if (dab.mode.x == 17u) {
            if (pixel.y < 1024u) {
                reference_layer[at] = 0u;
            }
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
        ink_canvas[at] = ink_canvas[at] + vec2<f32>(dark, wet);
    } else if (dab.mode.x == 10u) {
        ink_canvas[at] = ink_step[at];
    } else if (dab.mode.x == 18u) {
        canvas[at] = paint_step[at];
    }
}

fn gravity_micro_noise(pos: vec2<i32>) -> f32 {
    let n = u32(pos.x) * 374761393u + u32(pos.y) * 668265263u;
    let h = (n ^ (n >> 13u)) * 1274126177u;
    return f32((h ^ (h >> 16u)) & 1023u) / 1023.0;
}

fn paint_outflow_frac(pos: vec2<i32>, strength: f32, viscosity: f32) -> f32 {
    if (pos.x < 0 || pos.x >= 1024 || pos.y < 0 || pos.y >= 1024) {
        return 0.0;
    }
    let p_at = index(vec2<u32>(pos));
    let mass = amount(canvas[p_at]);
    if (mass <= 1e-6) {
        return 0.0;
    }
    // Self-arresting Bingham-plastic yield stress:
    // Viscosity = 1.0 (Heavy Oil) holds normal strokes (~0.55); lower values drip and arrest as they thin.
    let v = clamp(viscosity, 0.0, 1.0);
    let yield_thresh = 0.002 + 0.55 * v * v;
    let excess = max(mass - yield_thresh, 0.0);
    if (excess <= 0.0) {
        return 0.0;
    }
    // Subtle non-periodic grain (±3%) prevents robotic symmetry without periodic waffle ripples.
    let micro = 0.97 + 0.06 * gravity_micro_noise(pos);
    let flow_rate = clamp(strength * (1.0 - v * 0.85) * 0.48 * micro, 0.0, 0.65);
    return clamp((excess / mass) * flow_rate, 0.0, 0.55);
}

@compute @workgroup_size(8, 8)
fn flow_paint(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    if (pixel.x >= 1024u || pixel.y >= 1024u) { return; }
    let pos = vec2<i32>(pixel);
    let at = index(pixel);

    let gx = dab.axis.x;
    let gy = dab.axis.y;
    let strength = dab.brush.x;
    let viscosity = dab.brush.y;

    let dx = select(-1, 1, gx >= 0.0);
    let dy = select(-1, 1, gy >= 0.0);
    let wx = abs(gx);
    let wy = abs(gy);
    let w_sum = max(wx + wy, 1e-5);
    let fx = wx / w_sum;
    let fy = wy / w_sum;

    let cur = canvas[at];
    let out_frac = paint_outflow_frac(pos, strength, viscosity);
    let can_x = select(0.0, 1.0, (pos.x + dx) >= 0 && (pos.x + dx) < 1024);
    let can_y = select(0.0, 1.0, (pos.y + dy) >= 0 && (pos.y + dy) < 1024);
    var next_paint = scale(cur, max(1.0 - out_frac * (fx * can_x + fy * can_y), 0.0));

    let src_x = pos - vec2<i32>(dx, 0);
    if (src_x.x >= 0 && src_x.x < 1024 && fx > 0.0) {
        let f = paint_outflow_frac(src_x, strength, viscosity) * fx;
        if (f > 0.0) {
            next_paint = add(next_paint, scale(canvas[index(vec2<u32>(src_x))], f));
        }
    }
    let src_y = pos - vec2<i32>(0, dy);
    if (src_y.y >= 0 && src_y.y < 1024 && fy > 0.0) {
        let f = paint_outflow_frac(src_y, strength, viscosity) * fy;
        if (f > 0.0) {
            next_paint = add(next_paint, scale(canvas[index(vec2<u32>(src_y))], f));
        }
    }
    paint_step[at] = next_paint;
}

fn runoff(wet: f32, y: u32) -> f32 {
    return select(max(wet - 0.11, 0.0) * 0.82, 0.0, y == 1023u);
}

@compute @workgroup_size(8, 8)
fn flow_ink(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let at = index(pixel);
    let previous = ink_canvas[at];
    var wet = previous.y - runoff(previous.y, pixel.y);
    if (pixel.y > 0u) {
        wet += runoff(ink_canvas[at - 1024u].y, pixel.y - 1u);
    }
    let absorb = min(wet, wet * 0.035 + 0.006);
    ink_step[at] = vec2<f32>(previous.x + absorb, max(wet - absorb, 0.0));
}

fn surface_height(pos: vec2<u32>, impasto_scale: f32, weave_scale: f32) -> f32 {
    let p_at = index(pos);
    let wet_mass = amount(canvas[p_at]);
    let dry_mass = amount(dry_canvas[p_at]);
    let total_mass = wet_mass + dry_mass;

    // 1. Primed linen canvas weave texture
    // Sinusoidal interlaced warp & weft threads (period ~3.2 px)
    let fx = f32(pos.x) * 1.9635;
    let fy = f32(pos.y) * 1.9635;
    let warp = cos(fx);
    let weft = cos(fy);
    let cross = sin(fx) * sin(fy);
    let weave_pattern = (warp - weft + cross * 0.5) * 0.5;

    // Weave is prominent in unpainted gesso ground, and diminishes as paint fills the weave
    let weave_suppression = exp(-2.2 * total_mass);
    let h_weave = weave_pattern * weave_scale * 0.12 * weave_suppression;

    // 2. Physical paint volume & bristle ridges
    let h_paint = sqrt(total_mass * 1.2) * impasto_scale * 0.35;

    return h_weave + h_paint;
}

fn wrap_coord(v: i32, wrap: bool) -> u32 {
    if (wrap) {
        return u32(((v % 1024) + 1024) % 1024);
    }
    return u32(clamp(v, 0, 1023));
}

fn smoothed_surface_height(pixel: vec2<u32>, impasto_scale: f32, wrap: bool) -> f32 {
    var acc = 0.0;
    for (var dy: i32 = -2; dy <= 2; dy = dy + 1) {
        let sy = wrap_coord(i32(pixel.y) + dy, wrap);
        for (var dx: i32 = -2; dx <= 2; dx = dx + 1) {
            let sx = wrap_coord(i32(pixel.x) + dx, wrap);
            acc = acc + surface_height(vec2<u32>(sx, sy), impasto_scale, 0.0);
        }
    }
    return acc / 25.0;
}

fn sample_export_height(pixel: vec2<u32>, impasto_scale: f32, weave_scale: f32, wrap: bool, smooth_h: bool) -> f32 {
    if (smooth_h) {
        return smoothed_surface_height(pixel, impasto_scale, wrap);
    }
    return surface_height(pixel, impasto_scale, weave_scale);
}

fn compute_surface_normal(pixel: vec2<u32>, impasto_scale: f32, weave_scale: f32, wrap: bool, smooth_h: bool) -> vec3<f32> {
    let px = pixel.x;
    let py = pixel.y;
    let left_x = select(select(px - 1u, 0u, px == 0u), (px + 1023u) % 1024u, wrap);
    let right_x = select(select(px + 1u, 1023u, px >= 1023u), (px + 1u) % 1024u, wrap);
    let up_y = select(select(py - 1u, 0u, py == 0u), (py + 1023u) % 1024u, wrap);
    let down_y = select(select(py + 1u, 1023u, py >= 1023u), (py + 1u) % 1024u, wrap);

    let h_L = sample_export_height(vec2<u32>(left_x, py), impasto_scale, weave_scale, wrap, smooth_h);
    let h_R = sample_export_height(vec2<u32>(right_x, py), impasto_scale, weave_scale, wrap, smooth_h);
    let h_U = sample_export_height(vec2<u32>(px, up_y), impasto_scale, weave_scale, wrap, smooth_h);
    let h_D = sample_export_height(vec2<u32>(px, down_y), impasto_scale, weave_scale, wrap, smooth_h);

    let dh_dx = (h_R - h_L) * 3.5;
    let dh_dy = (h_D - h_U) * 3.5;

    return normalize(vec3<f32>(-dh_dx, -dh_dy, 1.0));
}

@compute @workgroup_size(8, 8)
fn display(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let at = index(pixel);
    let wet_paint = canvas[at];
    let is_canvas = pixel.y < 1024u;
    let export_mode = select(0u, dab.mode.z, is_canvas);
    let wrap = (dab.mode.w & 1u) != 0u;
    let smooth_h = (dab.mode.w & 4u) != 0u;
    let gl_normals = (dab.mode.w & 8u) != 0u;

    if (export_mode == 2u) {
        let h = sample_export_height(pixel, dab.brush.x, dab.brush.y, wrap, smooth_h);
        let h_norm = clamp((h + 0.15) / 1.15, 0.0, 1.0);
        textureStore(output, vec2<i32>(pixel), vec4<f32>(vec3<f32>(h_norm), 1.0));
        return;
    }
    if (export_mode == 3u) {
        let N = compute_surface_normal(pixel, dab.brush.x, dab.brush.y, wrap, smooth_h);
        let n_out = select(N, vec3<f32>(N.x, -N.y, N.z), gl_normals);
        textureStore(output, vec2<i32>(pixel), vec4<f32>(n_out * 0.5 + vec3<f32>(0.5), 1.0));
        return;
    }

    var color = composite_linear_color(wet_paint, at, is_canvas);
    if (is_canvas) {
        let ink_data = ink_canvas[at];
        let ink = 1.0 - exp(-(ink_data.x + ink_data.y) * 3.8);
        let ink_color = srgb_to_linear(vec3<f32>(0.024, 0.031, 0.048));
        var diffuse_factor = 1.0;
        var sheen = vec3<f32>(0.0);

        // 3D Impasto Relief & Studio Raking Light
        if (dab.mode.y != 0u) {
            let impasto_scale = dab.brush.x;
            let weave_scale = dab.brush.y;
            let sheen_scale = dab.brush.z;
            let L = normalize(dab.axis.xyz);
            let V = vec3<f32>(0.0, 0.0, 1.0);
            let N = compute_surface_normal(pixel, impasto_scale, weave_scale, (dab.mode.w & 1u) != 0u, false);

            // Diffuse illumination under raking light
            let NdotL = dot(N, L);
            let ambient = 0.65;
            let direct = 0.45;
            diffuse_factor = ambient + direct * clamp(NdotL, -0.2, 1.0);

            // Specular bristle sheen (Fresnel Blinn-Phong)
            let H = normalize(L + V);
            let NdotH = max(dot(N, H), 0.0);
            let VdotH = max(dot(V, H), 0.0);
            let F0 = 0.04;
            let fresnel = F0 + (1.0 - F0) * pow(1.0 - VdotH, 5.0);

            let wet_amt = amount(wet_paint);
            let dry_amt = amount(dry_canvas[at]);

            var gloss = 0.0;
            var shininess = 16.0;
            if (wet_amt > 0.002) {
                gloss = min(wet_amt * 1.5, 1.0);
                shininess = 52.0;
            } else if (dry_amt > 0.002) {
                gloss = min(dry_amt * 0.4, 0.28);
                shininess = 16.0;
            }

            let spec_term = pow(NdotH, shininess) * fresnel * gloss * sheen_scale;
            let light_color = vec3<f32>(1.0, 0.98, 0.95);

            sheen = spec_term * light_color;
        }
        let over_white = mix(color, ink_color, ink);
        color = clamp(over_white * diffuse_factor + sheen, vec3<f32>(0.0), vec3<f32>(1.0));

        // Composite 3-layer stack: Reference -> Sketch -> Paint
        let sketch_opacity = dab.brush.w;
        let ref_opacity = dab.axis.w;
        let hide_paint = (dab.mode.w & 2u) != 0u;
        let sketch_val = select(0.0, clamp(sketch_canvas[at] * sketch_opacity, 0.0, 1.0), sketch_opacity > 0.001);
        let ref_rgba = select(vec4<f32>(0.0), unpack4x8unorm(reference_layer[at]), ref_opacity > 0.001);
        let ref_val = ref_rgba.a * ref_opacity;
        if (export_mode == 1u || ref_val > 0.001 || sketch_val > 0.001 || hide_paint) {
            let combined = add(wet_paint, dry_canvas[at]);
            let has_paint = amount(combined) > 0.00001 || ink > 0.0001;
            if (export_mode == 1u && !has_paint) {
                textureStore(output, vec2<i32>(pixel), vec4<f32>(0.0));
                return;
            }
            // Keep the existing reference and charcoal opacity response. Once
            // composed, their backdrop enters the paint optics in linear RGB.
            let reference = mix(vec3<f32>(1.0), ref_rgba.rgb, ref_val);
            let bg = srgb_to_linear(mix(reference, vec3<f32>(0.11, 0.105, 0.10), sketch_val));
            if (export_mode != 1u && (hide_paint || !has_paint)) {
                color = bg;
            } else {
                let over_black = min(mix(paint_over_black(combined), ink_color, ink), over_white);
                let response = over_white - over_black;
                if (export_mode == 1u) {
                    // Preserve the white-backed appearance. A single PNG alpha
                    // cannot transmit each channel independently or subtract light.
                    // At very low alpha (<0.05), stabilize un-premultiply so edge
                    // pixels don't lift warm/bright over dark backgrounds.
                    let white_headroom = min(color.r, min(color.g, color.b));
                    let transmission = min(dot(response, vec3<f32>(1.0 / 3.0)), white_headroom);
                    let alpha = 1.0 - transmission;
                    let fg_unpremul = clamp((color - transmission) / max(alpha, 0.04), vec3<f32>(0.0), vec3<f32>(1.0));
                    let edge_t = smoothstep(0.02, 0.12, alpha);
                    let foreground = mix(over_black, fg_unpremul, edge_t);
                    textureStore(output, vec2<i32>(pixel), vec4<f32>(srgb(foreground), alpha));
                    return;
                }
                let backed = over_black + response * bg;
                color = clamp(backed * diffuse_factor + sheen, vec3<f32>(0.0), vec3<f32>(1.0));
            }
        }
    }
    textureStore(output, vec2<i32>(pixel), vec4<f32>(srgb(color), 1.0));
}

@compute @workgroup_size(4)
fn service(@builtin(local_invocation_index) lane: u32) {
    let first = dab.mode.y;
    let single = first == 0u || first == 13u;
    let count = select(3u, 1u, single);
    if (lane >= count) { return; }
    if ((dab.mode.x == 11u || dab.mode.x == 12u) && dab.mode.w != 0u && lane + 1u != dab.mode.w) { return; }
    let i = first + lane;
    if (dab.mode.x == 5u) {
        state.loads[i] = add(state.loads[i], pure(dab.mode.z, dab.brush.w / f32(count)));
        let capacity = select(61333.3, 184000.0, single);
        let cur = amount(state.loads[i]);
        if (cur > capacity) {
            state.loads[i] = scale(state.loads[i], capacity / cur);
        }
    } else if (dab.mode.x == 11u) {
        let source_paint = canvas[index(vec2<u32>(544u + dab.mode.z, 1024u))];
        let src_amt = amount(source_paint);
        if (src_amt > 0.0001) {
            let added = scale(source_paint, (dab.brush.w / f32(count)) / src_amt);
            state.loads[i] = add(state.loads[i], added);
        }
        let capacity = select(61333.3, 184000.0, single);
        let cur = amount(state.loads[i]);
        if (cur > capacity) {
            state.loads[i] = scale(state.loads[i], capacity / cur);
        }
    } else if (dab.mode.x == 12u) {
        let source_paint = canvas[index(vec2<u32>(608u + dab.mode.z, 1024u))];
        let src_amt = amount(source_paint);
        if (src_amt > 0.0001) {
            let added = scale(source_paint, (dab.brush.w / f32(count)) / src_amt);
            state.loads[i] = add(state.loads[i], added);
        }
        let capacity = select(61333.3, 184000.0, single);
        let cur = amount(state.loads[i]);
        if (cur > capacity) {
            state.loads[i] = scale(state.loads[i], capacity / cur);
        }
    } else {
        state.loads[i] = scale(state.loads[i], dab.brush.w);
    }
}

@compute @workgroup_size(16)
fn preview(@builtin(local_invocation_index) lane: u32) {
    if (lane >= 14u) { return; }
    let paint = state.loads[lane];
    let mass = amount(paint);
    var swatch = vec4<f32>(0.0);
    if (mass > 0.00001) {
        // The UI shows load by the size of the painted area. Its colour sample is opaque.
        swatch = vec4<f32>(srgb(composite_linear_color(scale(paint, 2.0 / mass), 0u, false)), 1.0);
    }
    textureStore(output, vec2<i32>(512 + i32(lane), 1024), swatch);
    if (lane == 0u && dab.mode.x == 2u) {
        textureStore(output, vec2<i32>(528, 1024), vec4<f32>(f32(state.placement_ok), 0.0, 0.0, 1.0));
    }
}
