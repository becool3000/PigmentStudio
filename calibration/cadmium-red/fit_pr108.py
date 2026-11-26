import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "phthalo-blue"))
import fit_pb15 as km

# Calibrated PW6 and PY3 anchors
K_PW6 = [0.0327, 0.005, 0.002, 0.001, 0.001, 0.001, 0.0021]
S_PW6 = [19.667, 24.758, 29.26, 28.0, 28.0, 24.074, 21.326]
K_PY3 = [8.7419, 6.1915, 0.452, 0.005, 0.002, 0.002, 0.003]
S_PY3 = [0.466, 0.427, 0.64, 0.833, 0.639, 0.765, 0.71]

K_PR108_BASE = [35.0, 32.0, 26.0, 14.0, 0.03, 0.005, 0.002]
S_PR108_BASE = [0.6, 0.7, 0.8, 1.2, 2.2, 2.4, 2.5]


def eval_pr108_mix(pr108, pw6, py3, k_r, s_r, ground=1.0):
    k31 = [km.interp31(k_r), km.interp31(K_PW6), km.interp31(K_PY3)]
    s31 = [km.interp31(s_r), km.interp31(S_PW6), km.interp31(S_PY3)]
    w = [pr108, pw6, py3]
    xyz = [0.0, 0.0, 0.0]
    spec = []
    for i in range(31):
        k = sum(w[j] * k31[j][i] for j in range(3))
        s = sum(w[j] * s31[j][i] for j in range(3))
        r_corr = km.km_reflectance(k, s, ground)
        spec.append(r_corr)
        for c in range(3):
            xyz[c] += r_corr * km.WEIGHTS[i][c]
    return xyz, spec


def evaluate(ref, k_r, s_r):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_pr108_mix(c["pr108"], c["pw6"], c["py3"], k_r, s_r, c["ground"])
            lab = km.xyz_to_lab(xyz)
            errs.append((c["name"], km.ciede2000(lab, c["target_lab"]), lab, km.xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])

    _, blk_spec = eval_pr108_mix(0.5, 0.0, 0.0, k_r, s_r, 0.0)
    blk_7 = [blk_spec[i * 5] for i in range(7)]
    blk_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(blk_7, ref["target_medium_over_black_7"])) / 7.0)

    yw, _ = eval_pr108_mix(ref["hiding_power_check"]["medium_film_mass"], 0.0, 0.0, k_r, s_r, 1.0)
    yb, _ = eval_pr108_mix(ref["hiding_power_check"]["medium_film_mass"], 0.0, 0.0, k_r, s_r, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, blk_rmse, hiding_pct


def fit_pr108(ref):
    k = list(K_PR108_BASE)
    s = list(S_PR108_BASE)

    def loss(kv, sv):
        fit_errs, _, blk_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_pr108_mix(2.0, 0.0, 0.0, kv, sv, 1.0)
        mast_7 = [mast_spec[i * 5] for i in range(7)]
        mast_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(mast_7, ref["target_masstone_spectrum_7"])) / 7.0)
        de_fit = sum(e[1] ** 2 for e in fit_errs) / len(fit_errs)
        reg = 0.02 * sum((math.log(sv[i + 1]) - math.log(sv[i])) ** 2 for i in range(6))
        hiding_pen = max(0.0, ref["hiding_power_check"]["target_min_hiding_ratio_pct"] - hiding_pct) * 0.5
        return de_fit + 40.0 * blk_rmse + 40.0 * mast_rmse + reg + hiding_pen

    best_loss = loss(k, s)
    for step in [0.35, 0.18, 0.08, 0.03]:
        improved = True
        while improved:
            improved = False
            for idx in range(7):
                k_lo = 8.0 if idx <= 3 else 0.001
                for arr, lo, hi in [(k, k_lo, 45.0), (s, 0.4, 4.5)]:
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
    ref_path = pathlib.Path(__file__).with_name("reference_pr108.json")
    ref = json.loads(ref_path.read_text(encoding="utf-8"))

    print("=== BASELINE PR108 ===")
    _, base_held, base_blk, base_hide = evaluate(ref, K_PR108_BASE, S_PR108_BASE)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in base_held)/len(base_held):.2f}, "
        f"Over-black RMSE: {base_blk:.4f}, Hiding(0.5): {base_hide:.2f}%"
    )
    for name, de, lab, srgb in base_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")

    k_fit, s_fit = fit_pr108(ref)
    print("\n=== FITTED PR108 ===")
    print("ABSORPTION[6]:", k_fit)
    print("SCATTERING[6]:", s_fit)
    _, fit_held, fit_blk, fit_hide = evaluate(ref, k_fit, s_fit)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in fit_held)/len(fit_held):.2f}, "
        f"Over-black RMSE: {fit_blk:.4f}, Hiding(0.5): {fit_hide:.2f}%"
    )
    for name, de, lab, srgb in fit_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")


if __name__ == "__main__":
    main()
