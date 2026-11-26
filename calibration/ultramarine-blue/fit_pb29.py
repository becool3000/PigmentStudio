import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "phthalo-blue"))
import fit_pb15 as km

# Calibrated PW6, PR122, and current PBr7 anchors
K_PW6 = [0.0327, 0.005, 0.002, 0.001, 0.001, 0.001, 0.0021]
S_PW6 = [19.667, 24.758, 29.26, 28.0, 28.0, 24.074, 21.326]
K_PR122 = [2.188, 1.097, 10.781, 31.101, 0.96, 0.22, 0.152]
S_PR122 = [0.159, 0.145, 0.115, 0.133, 0.135, 0.152, 0.203]
K_PBR7 = [4.5, 4.2, 2.8, 1.6, 0.55, 0.21, 0.13]
S_PBR7 = [0.68, 0.75, 0.8, 0.82, 0.81, 0.75, 0.7]

K_PB29_BASE = [0.06, 0.09, 0.55, 2.4, 3.5, 1.6, 0.9]
S_PB29_BASE = [0.95, 1.1, 1.1, 0.91, 0.87, 0.91, 1.0]


def eval_pb29_mix(pb29, pw6, pr122, pbr7, k_u, s_u, ground=1.0):
    k31 = [km.interp31(k_u), km.interp31(K_PW6), km.interp31(K_PR122), km.interp31(K_PBR7)]
    s31 = [km.interp31(s_u), km.interp31(S_PW6), km.interp31(S_PR122), km.interp31(S_PBR7)]
    w = [pb29, pw6, pr122, pbr7]
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


def evaluate(ref, k_u, s_u):
    def run_cases(cases):
        errs = []
        for c in cases:
            xyz, _ = eval_pb29_mix(c["pb29"], c["pw6"], c["pr122"], c["pbr7"], k_u, s_u, c["ground"])
            lab = km.xyz_to_lab(xyz)
            errs.append((c["name"], km.ciede2000(lab, c["target_lab"]), lab, km.xyz_to_srgb(xyz)))
        return errs

    fit_errs = run_cases(ref["fit_cases"])
    held_errs = run_cases(ref["held_out_cases"])

    _, glaze_spec = eval_pb29_mix(0.25, 0.0, 0.0, 0.0, k_u, s_u, 1.0)
    glaze_7 = [glaze_spec[i * 5] for i in range(7)]
    glaze_rmse = math.sqrt(sum((a - b) ** 2 for a, b in zip(glaze_7, ref["target_thin_glaze_white_7"])) / 7.0)

    yw, _ = eval_pb29_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, 0.0, k_u, s_u, 1.0)
    yb, _ = eval_pb29_mix(ref["hiding_power_check"]["thin_glaze_mass"], 0.0, 0.0, 0.0, k_u, s_u, 0.0)
    hiding_pct = (yb[1] / max(yw[1], 1e-5)) * 100.0
    return fit_errs, held_errs, glaze_rmse, hiding_pct


def fit_pb29(ref):
    k = list(K_PB29_BASE)
    s = list(S_PB29_BASE)

    def loss(kv, sv):
        fit_errs, _, glaze_rmse, hiding_pct = evaluate(ref, kv, sv)
        _, mast_spec = eval_pb29_mix(2.0, 0.0, 0.0, 0.0, kv, sv, 1.0)
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
                for arr, lo, hi in [(k, 0.02, 10.0), (s, 0.4, 1.6)]:
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
    ref_path = pathlib.Path(__file__).with_name("reference_pb29.json")
    ref = json.loads(ref_path.read_text(encoding="utf-8"))

    print("=== BASELINE PB29 ===")
    _, base_held, base_glaze, base_hide = evaluate(ref, K_PB29_BASE, S_PB29_BASE)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in base_held)/len(base_held):.2f}, "
        f"Glaze RMSE: {base_glaze:.4f}, Thin hiding: {base_hide:.2f}%"
    )
    for name, de, lab, srgb in base_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")

    k_fit, s_fit = fit_pb29(ref)
    print("\n=== FITTED PB29 ===")
    print("ABSORPTION[4]:", k_fit)
    print("SCATTERING[4]:", s_fit)
    _, fit_held, fit_glaze, fit_hide = evaluate(ref, k_fit, s_fit)
    print(
        f"Held-out mean dE00: {sum(e[1] for e in fit_held)/len(fit_held):.2f}, "
        f"Glaze RMSE: {fit_glaze:.4f}, Thin hiding: {fit_hide:.2f}%"
    )
    for name, de, lab, srgb in fit_held:
        print(f"  {name:38s} dE00={de:5.2f} Lab={[round(v,1) for v in lab]} sRGB={srgb}")


if __name__ == "__main__":
    main()
