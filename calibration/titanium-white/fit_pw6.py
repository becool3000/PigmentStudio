import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "phthalo-blue"))
import fit_pb15 as km

# Calibrated PB15:2 + current PY3 anchors from src/pigment.rs
K_PB15 = [1.802, 0.648, 1.053, 4.175, 22.372, 39.472, 40.064]
S_PB15 = [0.122, 0.106, 0.093, 0.076, 0.055, 0.143, 0.16]

K_PW6_BASE = [0.008, 0.004, 0.002, 0.001, 0.001, 0.001, 0.002]
S_PW6_BASE = [18.0, 22.0, 26.0, 28.0, 28.0, 26.0, 24.0]


def eval_pw6_mix(pb15, pw6, py3, k_w, s_w, ground=1.0):
    km.K_PW6[:] = k_w
    km.S_PW6[:] = s_w
    return km.eval_mix(pb15, pw6, py3, K_PB15, S_PB15, ground)


def evaluate(ref, k_w, s_w):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_pw6_mix(c["pb15"], c["pw6"], c["py3"], k_w, s_w, c["ground"])
            lab = km.xyz_to_lab(xyz)
            errs.append((c["name"], km.ciede2000(lab, c["target_lab"]), lab, km.xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])

    _, blk_spec = eval_pw6_mix(0.0, 0.5, 0.0, k_w, s_w, 0.0)
    blk_7 = [blk_spec[i * 5] for i in range(7)]
    blk_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(blk_7, ref["target_medium_over_black_7"])) / 7.0)

    yw, _ = eval_pw6_mix(0.0, ref["hiding_power_check"]["medium_film_mass"], 0.0, k_w, s_w, 1.0)
    yb, _ = eval_pw6_mix(0.0, ref["hiding_power_check"]["medium_film_mass"], 0.0, k_w, s_w, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, blk_rmse, hiding_pct


def fit_pw6(ref):
    k = list(K_PW6_BASE)
    s = list(S_PW6_BASE)

    def loss(kv, sv):
        fit_errs, _, blk_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_pw6_mix(0.0, 2.0, 0.0, kv, sv, 1.0)
        mast_7 = [mast_spec[i * 5] for i in range(7)]
        mast_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(mast_7, ref["target_masstone_spectrum_7"])) / 7.0)
        de_fit = sum(e[1] ** 2 for e in fit_errs) / len(fit_errs)
        reg = 0.05 * sum((math.log(sv[i + 1]) - math.log(sv[i])) ** 2 for i in range(6))
        hiding_pen = max(0.0, ref["hiding_power_check"]["target_min_hiding_ratio_pct"] - hiding_pct) * 0.5
        return de_fit + 40.0 * blk_rmse + 40.0 * mast_rmse + reg + hiding_pen

    best_loss = loss(k, s)
    for step in [0.35, 0.18, 0.08, 0.03]:
        improved = True
        while improved:
            improved = False
            for idx in range(7):
                for arr, lo, hi in [(k, 0.0005, 0.10), (s, 14.0, 36.0)]:
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
    return [round(v, 4) for v in k], [round(v, 3) for v in s]


def main():
    ref_path = pathlib.Path(__file__).with_name("reference_pw6.json")
    ref = json.loads(ref_path.read_text(encoding="utf-8"))

    print("=== BASELINE PW6 ===")
    _, base_held, base_blk, base_hide = evaluate(ref, K_PW6_BASE, S_PW6_BASE)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in base_held)/len(base_held):.2f}, "
        f"Over-black RMSE: {base_blk:.4f}, Hiding(0.5): {base_hide:.2f}%"
    )
    for name, de, lab, srgb in base_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")

    k_fit, s_fit = fit_pw6(ref)
    print("\n=== FITTED PW6 ===")
    print("ABSORPTION[3]:", k_fit)
    print("SCATTERING[3]:", s_fit)
    _, fit_held, fit_blk, fit_hide = evaluate(ref, k_fit, s_fit)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in fit_held)/len(fit_held):.2f}, "
        f"Over-black RMSE: {fit_blk:.4f}, Hiding(0.5): {fit_hide:.2f}%"
    )
    for name, de, lab, srgb in fit_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")


if __name__ == "__main__":
    main()
