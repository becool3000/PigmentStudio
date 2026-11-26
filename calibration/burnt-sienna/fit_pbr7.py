import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "phthalo-blue"))
import fit_pb15 as km

# Calibrated PB29, PW6, and PY3 anchors
K_PB29 = [0.051, 0.081, 0.611, 2.832, 4.587, 1.51, 0.556]
S_PB29 = [0.962, 1.037, 0.943, 0.848, 0.725, 0.823, 0.954]
K_PW6 = [0.0327, 0.005, 0.002, 0.001, 0.001, 0.001, 0.0021]
S_PW6 = [19.667, 24.758, 29.26, 28.0, 28.0, 24.074, 21.326]
K_PY3 = [8.7419, 6.1915, 0.452, 0.005, 0.002, 0.002, 0.003]
S_PY3 = [0.466, 0.427, 0.64, 0.833, 0.639, 0.765, 0.71]

K_PBR7_BASE = [4.5, 4.2, 2.8, 1.6, 0.55, 0.21, 0.13]
S_PBR7_BASE = [0.68, 0.75, 0.8, 0.82, 0.81, 0.75, 0.7]


def eval_pbr7_mix(pbr7, pb29, pw6, py3, k_s, s_s, ground=1.0):
    k31 = [km.interp31(k_s), km.interp31(K_PB29), km.interp31(K_PW6), km.interp31(K_PY3)]
    s31 = [km.interp31(s_s), km.interp31(S_PB29), km.interp31(S_PW6), km.interp31(S_PY3)]
    w = [pbr7, pb29, pw6, py3]
    xyz = [0.0, 0.0, 0.0]
    spec = []
    for i in range(31):
        k = sum(w[j] * k31[j][i] for j in range(4))
        s = sum(w[j] * s31[j][i] for j in range(4))
        r_corr = km.km_reflectance(k, s, ground)
        spec.append(r_corr)
        for c in range(3):
            xyz[c] += r_corr * km.WEIGHTS[i][c]
    return xyz, spec


def evaluate(ref, k_s, s_s):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_pbr7_mix(c["pbr7"], c["pb29"], c["pw6"], c["py3"], k_s, s_s, c["ground"])
            lab = km.xyz_to_lab(xyz)
            errs.append((c["name"], km.ciede2000(lab, c["target_lab"]), lab, km.xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])

    _, glaze_spec = eval_pbr7_mix(0.25, 0.0, 0.0, 0.0, k_s, s_s, 1.0)
    glaze_7 = [glaze_spec[i * 5] for i in range(7)]
    glaze_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(glaze_7, ref["target_thin_glaze_white_7"])) / 7.0)

    yw, _ = eval_pbr7_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, 0.0, k_s, s_s, 1.0)
    yb, _ = eval_pbr7_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, 0.0, k_s, s_s, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, glaze_rmse, hiding_pct


def fit_pbr7(ref):
    k = list(K_PBR7_BASE)
    s = list(S_PBR7_BASE)

    def loss(kv, sv):
        fit_errs, _, glaze_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_pbr7_mix(2.0, 0.0, 0.0, 0.0, kv, sv, 1.0)
        mast_7 = [mast_spec[i * 5] for i in range(7)]
        mast_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(mast_7, ref["target_masstone_spectrum_7"])) / 7.0)
        de_fit = sum(e[1] ** 2 for e in fit_errs) / len(fit_errs)
        reg = 0.02 * sum((math.log(sv[i + 1]) - math.log(sv[i])) ** 2 for i in range(6))
        hiding_pen = max(0.0, hiding_pct - ref["hiding_power_check"]["target_max_hiding_ratio_pct"]) * 0.5
        return de_fit + 35.0 * glaze_rmse + 40.0 * mast_rmse + reg + hiding_pen

    best_loss = loss(k, s)
    for step in [0.35, 0.18, 0.08, 0.03]:
        improved = True
        while improved:
            improved = False
            for idx in range(7):
                for arr, lo, hi in [(k, 0.05, 12.0), (s, 0.35, 1.2)]:
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
    ref_path = pathlib.Path(__file__).with_name("reference_pbr7.json")
    ref = json.loads(ref_path.read_text(encoding="utf-8"))

    print("=== BASELINE PBR7 ===")
    _, base_held, base_glaze, base_hide = evaluate(ref, K_PBR7_BASE, S_PBR7_BASE)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in base_held)/len(base_held):.2f}, "
        f"Glaze RMSE: {base_glaze:.4f}, Thin hiding: {base_hide:.2f}%"
    )
    for name, de, lab, srgb in base_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")

    k_fit, s_fit = fit_pbr7(ref)
    print("\n=== FITTED PBR7 ===")
    print("ABSORPTION[5]:", k_fit)
    print("SCATTERING[5]:", s_fit)
    _, fit_held, fit_glaze, fit_hide = evaluate(ref, k_fit, s_fit)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in fit_held)/len(fit_held):.2f}, "
        f"Glaze RMSE: {fit_glaze:.4f}, Thin hiding: {fit_hide:.2f}%"
    )
    for name, de, lab, srgb in fit_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")


if __name__ == "__main__":
    main()
