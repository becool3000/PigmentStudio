import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "phthalo-blue"))
import fit_pb15 as km

# Calibrated PB15:2 + calibrated PW6 anchors
K_PB15 = [1.802, 0.648, 1.053, 4.175, 22.372, 39.472, 40.064]
S_PB15 = [0.122, 0.106, 0.093, 0.076, 0.055, 0.143, 0.16]
K_PW6 = [0.0327, 0.005, 0.002, 0.001, 0.001, 0.001, 0.0021]
S_PW6 = [19.667, 24.758, 29.26, 28.0, 28.0, 24.074, 21.326]

K_PY3_BASE = [4.0, 3.0, 0.9, 0.006, 0.002, 0.002, 0.003]
S_PY3_BASE = [0.42, 0.45, 0.65, 0.75, 0.67, 0.55, 0.5]


def eval_py3_mix(pb15, pw6, py3, k_y, s_y, ground=1.0):
    km.K_PW6[:] = K_PW6
    km.S_PW6[:] = S_PW6
    km.K_PY3[:] = k_y
    km.S_PY3[:] = s_y
    return km.eval_mix(pb15, pw6, py3, K_PB15, S_PB15, ground)


def evaluate(ref, k_y, s_y):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_py3_mix(c["pb15"], c["pw6"], c["py3"], k_y, s_y, c["ground"])
            lab = km.xyz_to_lab(xyz)
            errs.append((c["name"], km.ciede2000(lab, c["target_lab"]), lab, km.xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])

    _, glaze_spec = eval_py3_mix(0.0, 0.0, 0.25, k_y, s_y, 1.0)
    glaze_7 = [glaze_spec[i * 5] for i in range(7)]
    glaze_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(glaze_7, ref["target_thin_glaze_white_7"])) / 7.0)

    yw, _ = eval_py3_mix(0.0, 0.0, ref["hiding_power_check"]["thin_glaze_mass"], k_y, s_y, 1.0)
    yb, _ = eval_py3_mix(0.0, 0.0, ref["hiding_power_check"]["thin_glaze_mass"], k_y, s_y, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, glaze_rmse, hiding_pct


def fit_py3(ref):
    k = list(K_PY3_BASE)
    s = list(S_PY3_BASE)

    def loss(kv, sv):
        fit_errs, _, glaze_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_py3_mix(0.0, 0.0, 2.0, kv, sv, 1.0)
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
                k_lo = 2.0 if idx in (0, 1) else 0.001
                for arr, lo, hi in [(k, k_lo, 20.0), (s, 0.25, 1.20)]:
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
    ref_path = pathlib.Path(__file__).with_name("reference_py3.json")
    ref = json.loads(ref_path.read_text(encoding="utf-8"))

    print("=== BASELINE PY3 ===")
    _, base_held, base_glaze, base_hide = evaluate(ref, K_PY3_BASE, S_PY3_BASE)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in base_held)/len(base_held):.2f}, "
        f"Glaze RMSE: {base_glaze:.4f}, Thin hiding: {base_hide:.2f}%"
    )
    for name, de, lab, srgb in base_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")

    k_fit, s_fit = fit_py3(ref)
    print("\n=== FITTED PY3 ===")
    print("ABSORPTION[0]:", k_fit)
    print("SCATTERING[0]:", s_fit)
    _, fit_held, fit_glaze, fit_hide = evaluate(ref, k_fit, s_fit)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in fit_held)/len(fit_held):.2f}, "
        f"Glaze RMSE: {fit_glaze:.4f}, Thin hiding: {fit_hide:.2f}%"
    )
    for name, de, lab, srgb in fit_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")


if __name__ == "__main__":
    main()
