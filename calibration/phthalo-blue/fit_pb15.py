import json
import math
import pathlib

# Exact CIE 1931 2-deg observer + D65 SPD from src/pigment.rs (400..700 nm, 10 nm step)
OBSERVER = [
    [0.01431, 0.000396, 0.06785, 82.7549],
    [0.04351, 0.00121, 0.2074, 91.486],
    [0.13438, 0.004, 0.6456, 93.4318],
    [0.2839, 0.0116, 1.3856, 86.6823],
    [0.34828, 0.023, 1.74706, 104.865],
    [0.3362, 0.038, 1.77211, 117.008],
    [0.2908, 0.06, 1.6692, 117.812],
    [0.19536, 0.09098, 1.28764, 114.861],
    [0.09564, 0.13902, 0.81295, 115.923],
    [0.03201, 0.20802, 0.46518, 108.811],
    [0.0049, 0.323, 0.272, 109.354],
    [0.0093, 0.503, 0.1582, 107.802],
    [0.06327, 0.71, 0.07825, 104.79],
    [0.1655, 0.862, 0.04216, 107.689],
    [0.2904, 0.954, 0.0203, 104.405],
    [0.43345, 0.99495, 0.00875, 104.046],
    [0.5945, 0.995, 0.0039, 100.0],
    [0.7621, 0.952, 0.0021, 96.3342],
    [0.9163, 0.87, 0.00165, 95.788],
    [1.0263, 0.757, 0.0011, 88.6856],
    [1.0622, 0.631, 0.0008, 90.0062],
    [1.0026, 0.503, 0.00034, 89.5991],
    [0.85445, 0.381, 0.00019, 87.6987],
    [0.6424, 0.265, 0.00005, 83.2886],
    [0.4479, 0.175, 0.00002, 83.6992],
    [0.2835, 0.107, 0.0, 80.0268],
    [0.1649, 0.061, 0.0, 80.2146],
    [0.0874, 0.032, 0.0, 82.2778],
    [0.04677, 0.017, 0.0, 78.2842],
    [0.0227, 0.00821, 0.0, 69.7213],
    [0.011359, 0.004102, 0.0, 71.6091],
]

D65_WHITE = [0.95047, 1.0, 1.08883]
SUMS = [sum(row[c] * row[3] for row in OBSERVER) for c in range(3)]
WEIGHTS = [[OBSERVER[i][c] * OBSERVER[i][3] * D65_WHITE[c] / SUMS[c] for c in range(3)] for i in range(31)]

# Existing Studio Essentials anchors from src/pigment.rs
K_PY3 = [4.0, 3.0, 0.9, 0.006, 0.002, 0.002, 0.003]
S_PY3 = [0.42, 0.45, 0.65, 0.75, 0.67, 0.55, 0.5]
K_PW6 = [0.008, 0.004, 0.002, 0.001, 0.001, 0.001, 0.002]
S_PW6 = [18.0, 22.0, 26.0, 28.0, 28.0, 26.0, 24.0]

K_PB15_BASE = [1.5, 0.8, 1.0, 3.2, 18.0, 38.0, 30.0]
S_PB15_BASE = [0.25, 0.22, 0.20, 0.18, 0.15, 0.12, 0.10]


def interp31(anchors):
    out = []
    for i in range(31):
        a = min(i // 5, 5)
        b = (i - a * 5) / 5.0
        out.append(anchors[a] * (1.0 - b) + anchors[a + 1] * b)
    return out


def km_reflectance(k, s, r_sub=1.0, k2=0.60):
    s_safe = max(s, 1e-6)
    r = k / s_safe
    a = 1.0 + r
    b = math.sqrt(r * (r + 2.0))
    r_inf = 1.0 / (a + b)
    u = math.sqrt(k * (k + 2.0 * s))
    e2 = math.exp(-2.0 * u)
    denom = max(1.0 - r_sub * r_inf, 1e-5)
    d_val = (r_inf - r_sub) / denom
    d_denom = max(1.0 - e2 * r_inf * d_val, 1e-5)
    r_int = min(max((r_inf - e2 * d_val) / d_denom, 0.0), 1.0)
    return (r_int * (1.0 - k2)) / max(1.0 - k2 * r_int, 1e-5)


def eval_mix(pb15, pw6, py3, k_pb, s_pb, ground=1.0):
    k_pb31, s_pb31 = interp31(k_pb), interp31(s_pb)
    k_w31, s_w31 = interp31(K_PW6), interp31(S_PW6)
    k_y31, s_y31 = interp31(K_PY3), interp31(S_PY3)
    xyz = [0.0, 0.0, 0.0]
    spec = []
    for i in range(31):
        k = pb15 * k_pb31[i] + pw6 * k_w31[i] + py3 * k_y31[i]
        s = pb15 * s_pb31[i] + pw6 * s_w31[i] + py3 * s_y31[i]
        r_corr = km_reflectance(k, s, ground)
        spec.append(r_corr)
        for c in range(3):
            xyz[c] += r_corr * WEIGHTS[i][c]
    return xyz, spec


def xyz_to_lab(xyz):
    def f(t):
        return t ** (1.0 / 3.0) if t > 0.008856 else 7.787 * t + 16.0 / 116.0

    fx, fy, fz = f(xyz[0] / D65_WHITE[0]), f(xyz[1] / D65_WHITE[1]), f(xyz[2] / D65_WHITE[2])
    return [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]


def xyz_to_srgb(xyz):
    m = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.969266, 1.8760108, 0.041556],
        [0.0556434, -0.2040259, 1.0572252],
    ]
    out = []
    for row in m:
        lin = min(max(sum(xyz[i] * row[i] for i in range(3)), 0.0), 1.0)
        s = 12.92 * lin if lin <= 0.0031308 else 1.055 * (lin ** (1.0 / 2.4)) - 0.055
        out.append(int(round(s * 255)))
    return out


def ciede2000(lab1, lab2):
    L1, a1, b1 = lab1
    L2, a2, b2 = lab2
    C1, C2 = math.hypot(a1, b1), math.hypot(a2, b2)
    C_bar = 0.5 * (C1 + C2)
    C_bar7 = C_bar ** 7
    G = 0.5 * (1.0 - math.sqrt(C_bar7 / (C_bar7 + 25.0 ** 7)))
    a1p, a2p = (1.0 + G) * a1, (1.0 + G) * a2
    C1p, C2p = math.hypot(a1p, b1), math.hypot(a2p, b2)
    h1p = math.degrees(math.atan2(b1, a1p)) % 360.0 if C1p > 1e-8 else 0.0
    h2p = math.degrees(math.atan2(b2, a2p)) % 360.0 if C2p > 1e-8 else 0.0
    dLp = L2 - L1
    dCp = C2p - C1p
    if C1p * C2p == 0.0:
        dhp = 0.0
    elif abs(h2p - h1p) <= 180.0:
        dhp = h2p - h1p
    elif h2p - h1p > 180.0:
        dhp = (h2p - h1p) - 360.0
    else:
        dhp = (h2p - h1p) + 360.0
    dHp = 2.0 * math.sqrt(C1p * C2p) * math.sin(math.radians(dhp / 2.0))
    L_bar = 0.5 * (L1 + L2)
    Cp_bar = 0.5 * (C1p + C2p)
    if C1p * C2p == 0.0:
        hp_bar = h1p + h2p
    elif abs(h1p - h2p) <= 180.0:
        hp_bar = 0.5 * (h1p + h2p)
    elif h1p + h2p < 360.0:
        hp_bar = 0.5 * (h1p + h2p + 360.0)
    else:
        hp_bar = 0.5 * (h1p + h2p - 360.0)
    T = (
        1.0
        - 0.17 * math.cos(math.radians(hp_bar - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hp_bar))
        + 0.32 * math.cos(math.radians(3.0 * hp_bar + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hp_bar - 63.0))
    )
    d_theta = 30.0 * math.exp(-(((hp_bar - 275.0) / 25.0) ** 2))
    Cp_bar7 = Cp_bar ** 7
    R_C = 2.0 * math.sqrt(Cp_bar7 / (Cp_bar7 + 25.0 ** 7))
    L50 = (L_bar - 50.0) ** 2
    S_L = 1.0 + (0.015 * L50) / math.sqrt(20.0 + L50)
    S_C = 1.0 + 0.045 * Cp_bar
    S_H = 1.0 + 0.015 * Cp_bar * T
    R_T = -math.sin(math.radians(2.0 * d_theta)) * R_C
    return math.sqrt(
        (dLp / S_L) ** 2
        + (dCp / S_C) ** 2
        + (dHp / S_H) ** 2
        + R_T * (dCp / S_C) * (dHp / S_H)
    )


def evaluate(ref, k_pb, s_pb):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_mix(c["pb15"], c["pw6"], c["py3"], k_pb, s_pb, c["ground"])
            lab = xyz_to_lab(xyz)
            errs.append((c["name"], ciede2000(lab, c["target_lab"]), lab, xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])
    _, glaze_spec = eval_mix(0.25, 0.0, 0.0, k_pb, s_pb, 1.0)
    glaze_7 = [glaze_spec[i * 5] for i in range(7)]
    glaze_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(glaze_7, ref["target_thin_glaze_white_7"])) / 7.0)

    yw, _ = eval_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, k_pb, s_pb, 1.0)
    yb, _ = eval_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, k_pb, s_pb, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, glaze_rmse, hiding_pct


def fit_pb15(ref):
    k = list(K_PB15_BASE)
    s = list(S_PB15_BASE)

    def loss(kv, sv):
        fit_errs, held_errs, glaze_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_mix(2.0, 0.0, 0.0, kv, sv, 1.0)
        mast_7 = [mast_spec[i * 5] for i in range(7)]
        mast_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(mast_7, ref["target_masstone_spectrum_7"])) / 7.0)
        de_fit = sum(e[1] ** 2 for e in fit_errs) / len(fit_errs)
        # Mild smoothness regularizer on log(K) and log(S)
        reg = 0.05 * sum((math.log(kv[i + 1]) - math.log(kv[i])) ** 2 for i in range(6))
        hiding_pen = max(0.0, hiding_pct - 18.0) * 0.5
        return de_fit + 35.0 * glaze_rmse + 40.0 * mast_rmse + reg + hiding_pen

    best_loss = loss(k, s)
    for step in [0.35, 0.18, 0.08, 0.03]:
        improved = True
        while improved:
            improved = False
            for idx in range(7):
                k_lo = 20.0 if idx in (4, 5) else (8.0 if idx == 6 else 0.2)
                for arr, lo, hi in [(k, k_lo, 80.0), (s, 0.04, 0.16)]:
                    orig = arr[idx]
                    for mult in (1.0 + step, 1.0 / (1.0 + step)):
                        cand = min(max(orig * mult, lo), hi)
                        arr[idx] = cand
                        l = loss(k, s)
                        if l + 1e-6 < best_loss:
                            best_loss = l
                            orig = cand
                            improved = True
                        else:
                            arr[idx] = orig
                    arr[idx] = orig
    return [round(v, 3) for v in k], [round(v, 3) for v in s]


def main():
    ref_path = pathlib.Path(__file__).with_name("reference_pb15_2.json")
    ref = json.loads(ref_path.read_text())

    print("=== BASELINE PB15 ===")
    f_b, h_b, g_b, hp_b = evaluate(ref, K_PB15_BASE, S_PB15_BASE)
    print(f"Held-out mean dE00: {sum(e[1] for e in h_b)/len(h_b):.2f}, Glaze RMSE: {g_b:.4f}, Thin hiding: {hp_b:.2f}%")
    for name, de, lab, rgb in h_b:
        print(f"  {name:40s} dE00={de:5.2f} Lab={[round(x,1) for x in lab]} sRGB={rgb}")

    k_fit, s_fit = fit_pb15(ref)
    print("\n=== FITTED PB15:2 ===")
    print("ABSORPTION[1]:", k_fit)
    print("SCATTERING[1]:", s_fit)
    f_n, h_n, g_n, hp_n = evaluate(ref, k_fit, s_fit)
    print(f"Held-out mean dE00: {sum(e[1] for e in h_n)/len(h_n):.2f}, Glaze RMSE: {g_n:.4f}, Thin hiding: {hp_n:.2f}%")
    for name, de, lab, rgb in h_n:
        print(f"  {name:40s} dE00={de:5.2f} Lab={[round(x,1) for x in lab]} sRGB={rgb}")


if __name__ == "__main__":
    main()
