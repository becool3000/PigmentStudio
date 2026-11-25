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
@group(0) @binding(6) var<storage, read_write> ink_canvas: array<vec2<f32>>;
@group(0) @binding(7) var<storage, read_write> dry_canvas: array<Pigments>;

struct DrySubstrate {
    r0: vec4<u32>,
    r1: vec4<u32>,
    r2: vec4<u32>,
    r3: vec4<u32>,
};

@group(0) @binding(8) var<storage, read_write> dry_substrate: array<DrySubstrate>;
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
            paint = add(paint, scale(pixel_paint, pickup(footprint.x)));
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

fn composite_display_color(wet: Pigments, at: u32, is_canvas: bool) -> vec4<f32> {
    let wet_total = amount(wet);
    if (!is_canvas && wet_total <= 0.00001) { return vec4<f32>(1.0); }

    var sub: DrySubstrate;
    var words: array<u32, 16>;
    if (is_canvas) {
        sub = dry_substrate[at];
        if (wet_total <= 0.00001
            && all(sub.r0 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r1 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r2 == vec4<u32>(0xFFFFFFFFu))
            && all(sub.r3 == vec4<u32>(0xFFFFFFFFu))) {
            return vec4<f32>(1.0);
        }
        words = array<u32, 16>(
            sub.r0.x, sub.r0.y, sub.r0.z, sub.r0.w,
            sub.r1.x, sub.r1.y, sub.r1.z, sub.r1.w,
            sub.r2.x, sub.r2.y, sub.r2.z, sub.r2.w,
            sub.r3.x, sub.r3.y, sub.r3.z, sub.r3.w
        );
    }

    let k2 = 0.60;
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
            let r_corrected = (r_int * (1.0 - k2)) / max(1.0 - k2 * r_int, 0.00001);

            xyz += r_corrected * band.xyz.xyz;
        }
    }
    let linear = clamp(vec3<f32>(
        dot(xyz, vec3<f32>(3.2404542, -1.5371385, -0.4985314)),
        dot(xyz, vec3<f32>(-0.969266, 1.8760108, 0.041556)),
        dot(xyz, vec3<f32>(0.0556434, -0.2040259, 1.0572252))
    ), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(srgb(linear), 1.0);
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
            paint = add(scale(paint, 1.0 - pickup(w)), scale(state.transfer[u32(footprint.y)], w));
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

@compute @workgroup_size(8, 8)
fn display(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= dab.bounds.zw)) { return; }
    let pixel = dab.bounds.xy + id.xy;
    let at = index(pixel);
    let wet_paint = canvas[at];
    let is_canvas = pixel.y < 1024u;
    let base = composite_display_color(wet_paint, at, is_canvas);
    var color = base.rgb;
    if (is_canvas) {
        let ink_data = ink_canvas[at];
        let ink = 1.0 - exp(-(ink_data.x + ink_data.y) * 3.8);
        color = mix(base.rgb, vec3<f32>(0.024, 0.031, 0.048), ink);

        // 3D Impasto Relief & Studio Raking Light
        if (dab.mode.y != 0u) {
            let impasto_scale = dab.brush.x;
            let weave_scale = dab.brush.y;
            let sheen_scale = dab.brush.z;
            let L = normalize(dab.axis.xyz);
            let V = vec3<f32>(0.0, 0.0, 1.0);

            let px = pixel.x;
            let py = pixel.y;
            let left_x = select(px - 1u, 0u, px == 0u);
            let right_x = select(px + 1u, 1023u, px >= 1023u);
            let up_y = select(py - 1u, 0u, py == 0u);
            let down_y = select(py + 1u, 1023u, py >= 1023u);

            let h_L = surface_height(vec2<u32>(left_x, py), impasto_scale, weave_scale);
            let h_R = surface_height(vec2<u32>(right_x, py), impasto_scale, weave_scale);
            let h_U = surface_height(vec2<u32>(px, up_y), impasto_scale, weave_scale);
            let h_D = surface_height(vec2<u32>(px, down_y), impasto_scale, weave_scale);

            let dh_dx = (h_R - h_L) * 3.5;
            let dh_dy = (h_D - h_U) * 3.5;

            let N = normalize(vec3<f32>(-dh_dx, -dh_dy, 1.0));

            // Diffuse illumination under raking light
            let NdotL = dot(N, L);
            let ambient = 0.65;
            let direct = 0.45;
            let diffuse_factor = ambient + direct * clamp(NdotL, -0.2, 1.0);

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

            color = clamp(color * diffuse_factor + spec_term * light_color, vec3<f32>(0.0), vec3<f32>(1.0));
        }
    }
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
        let capacity = select(61333.3, 184000.0, first == 0u);
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
        let capacity = select(61333.3, 184000.0, first == 0u);
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
        let capacity = select(61333.3, 184000.0, first == 0u);
        let cur = amount(state.loads[i]);
        if (cur > capacity) {
            state.loads[i] = scale(state.loads[i], capacity / cur);
        }
    } else {
        state.loads[i] = scale(state.loads[i], dab.brush.w);
    }
}

@compute @workgroup_size(4)
fn preview(@builtin(local_invocation_index) lane: u32) {
    let paint = state.loads[lane];
    let capacity = select(61333.3, 184000.0, lane == 0u);
    let fill = clamp(amount(paint) / capacity, 0.0, 1.0);
    let color = composite_display_color(scale(paint, 2.0 / max(amount(paint), 0.00001)), 0u, false);
    textureStore(output, vec2<i32>(512 + i32(lane), 1024), vec4<f32>(color.rgb, fill));
    if (lane == 0u && dab.mode.x == 2u) {
        textureStore(output, vec2<i32>(528, 1024), vec4<f32>(f32(state.placement_ok), 0.0, 0.0, 1.0));
    }
}
