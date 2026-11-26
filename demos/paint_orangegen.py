import json
import math
import pathlib
import random
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "release" / "pigment-studio-ink.exe"


# Channel indices: 0=PY3, 1=PB15:2, 2=PR122, 3=PW6, 4=PB29, 5=PBr7, 6=PR108, 7=PBk9
def norm_recipe(r):
    s = sum(r)
    return [round(v / s, 4) for v in r] if s > 0 else [0.0] * 8


def mix_event(brush, weights, clean=True):
    return {"type": "set_mix", "brush": brush, "recipe": norm_recipe(weights), "clean": clean}


def stroke_event(
    sid,
    group,
    brush,
    diameter,
    samples,
    angle_deg=0.0,
    auto_follow=True,
    bristle_sweep=False,
    filbert_sweep=False,
    round_pressure=False,
):
    return {
        "type": "stroke",
        "id": sid,
        "group": group,
        "surface": "canvas",
        "brush": brush,
        "diameter": diameter,
        "angle_deg": angle_deg,
        "auto_follow": auto_follow,
        "bristle_sweep": bristle_sweep,
        "filbert_sweep": filbert_sweep,
        "round_pressure": round_pressure,
        "samples": samples,
    }


def pts_line(x0, y0, x1, y1, steps=8, p0=0.88, p1=0.88):
    out = []
    for i in range(steps + 1):
        t = i / steps
        out.append({
            "x": round(x0 + (x1 - x0) * t, 1),
            "y": round(y0 + (y1 - y0) * t, 1),
            "pressure": round(p0 + (p1 - p0) * t, 3),
            "timestamp_ms": i * 16,
        })
    return out


class ServeClient:
    def __init__(self):
        self.proc = subprocess.Popen(
            [str(EXE), "--serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=str(ROOT),
        )

    def call(self, payload):
        self.proc.stdin.write(json.dumps(payload) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            err = self.proc.stderr.read()
            raise RuntimeError(f"--serve closed unexpectedly: {err}")
        resp = json.loads(line)
        if not resp.get("ok"):
            raise RuntimeError(f"--serve error on {payload.get('op')}: {resp.get('error')}")
        return resp["data"]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=15)


def build_background_and_table(rng):
    events = []
    # Warm dark umber wall (left/top) transitioning to warm Burnt Sienna glow (right)
    dark_wall = [0.02, 0.0, 0.0, 0.0, 0.0, 0.44, 0.02, 0.52]
    mid_wall = [0.06, 0.0, 0.0, 0.01, 0.0, 0.72, 0.02, 0.19]
    warm_wall = [0.12, 0.0, 0.0, 0.02, 0.0, 0.78, 0.02, 0.06]

    # Dense horizontal + diagonal base pass so zero bare canvas shows
    for row in range(-20, 660, 52):
        for col in range(-40, 1040, 74):
            t_x = max(0.0, min(1.0, col / 1024.0))
            if t_x < 0.44:
                rec = dark_wall
            elif t_x < 0.72:
                rec = mid_wall if rng.random() < 0.7 else dark_wall
            else:
                rec = warm_wall if rng.random() < 0.75 else mid_wall
            events.append(mix_event("round", rec, clean=True))
            events.append(
                stroke_event(
                    f"bg_base_{row}_{col}",
                    "background",
                    "round",
                    diameter=115.0,
                    samples=pts_line(col, row, col + 95, row + rng.uniform(-25, 25), steps=5, p0=0.95, p1=0.95),
                    round_pressure=True,
                )
            )

    # Expressive diagonal hog-bristle flat strokes over wall
    for row in range(20, 620, 75):
        for col in range(0, 1000, 110):
            t_x = col / 1024.0
            rec = dark_wall if t_x < 0.45 else (mid_wall if t_x < 0.72 else warm_wall)
            events.append(mix_event("flat", rec, clean=True))
            events.append(
                stroke_event(
                    f"bg_flat_{row}_{col}",
                    "background",
                    "flat",
                    diameter=92.0,
                    samples=pts_line(col, row, col + rng.uniform(60, 110), row + rng.uniform(-70, 70), steps=5, p0=0.9, p1=0.85),
                    bristle_sweep=True,
                )
            )

    # Table / stone surface (y = 610..1040)
    table_base = [0.10, 0.0, 0.0, 0.48, 0.0, 0.30, 0.0, 0.12]
    table_light = [0.10, 0.0, 0.0, 0.62, 0.0, 0.22, 0.0, 0.06]
    table_shadow = [0.08, 0.0, 0.0, 0.32, 0.0, 0.40, 0.0, 0.20]

    for row in range(615, 1045, 44):
        for col in range(-30, 1040, 72):
            dist_center = abs(col - 510) / 520.0
            if row < 680 and dist_center > 0.30:
                rec = table_shadow
            elif row > 780 and 220 < col < 760:
                rec = table_light
            else:
                rec = table_base
            events.append(mix_event("filbert", rec, clean=True))
            events.append(
                stroke_event(
                    f"tbl_{row}_{col}",
                    "table",
                    "filbert",
                    diameter=92.0,
                    samples=pts_line(
                        col,
                        row + rng.uniform(-8, 8),
                        col + rng.uniform(75, 105),
                        row + rng.uniform(-14, 14),
                        steps=5,
                        p0=0.92,
                        p1=0.90,
                    ),
                    filbert_sweep=True,
                )
            )

    # Subtle diagonal drapery/stone step highlight & shadow
    events.append(mix_event("filbert", table_light, clean=True))
    events.append(
        stroke_event(
            "tbl_fold_light",
            "table",
            "filbert",
            diameter=62.0,
            samples=pts_line(520, 795, 750, 1020, steps=12, p0=0.65, p1=0.82),
            filbert_sweep=True,
        )
    )
    events.append({"type": "dry_canvas"})
    return events


def build_cast_shadow_and_orange(rng):
    events = []
    # Elliptical cast shadow tucked right under orange base (520, 730) stretching to (910, 725)
    shadow_dark = [0.02, 0.0, 0.0, 0.02, 0.02, 0.42, 0.0, 0.52]
    shadow_soft = [0.04, 0.0, 0.0, 0.14, 0.02, 0.46, 0.0, 0.34]
    for i in range(9):
        t = i / 8.0
        yc = 688.0 + t * 76.0
        half_w = 185.0 * math.sin(max(0.15, min(math.pi - 0.15, t * math.pi)))
        x_start = 485.0 + (1.0 - t) * 40.0
        x_end = 560.0 + half_w * 1.9
        rec = shadow_dark if 0.25 < t < 0.78 else shadow_soft
        events.append(mix_event("filbert", rec, clean=True))
        events.append(
            stroke_event(
                f"cast_sh_{i}",
                "cast_shadow",
                "filbert",
                diameter=42.0,
                samples=pts_line(x_start, yc, x_end, yc + 4.0, steps=10, p0=0.88, p1=0.45),
                filbert_sweep=True,
            )
        )
    events.append({"type": "dry_canvas"})

    # Orange sphere: center (525, 485), rx=262, ry=252
    cx, cy, rx, ry = 525.0, 485.0, 262.0, 252.0
    lx, ly = -0.62, -0.58

    # Note: PY3 needs ~75-88% weight to balance PR108's high tinting strength into true orange!
    # First: opaque warm Titanium White + PY3 underpainting inside the orange so dark wall doesn't dull the orange!
    under_orange = [0.42, 0.0, 0.0, 0.50, 0.0, 0.0, 0.08, 0.0]
    for gy in range(int(cy - ry + 32), int(cy + ry - 28), 44):
        for gx in range(int(cx - rx + 32), int(cx + rx - 28), 44):
            nx = (gx - cx) / rx
            ny = (gy - cy) / ry
            if nx * nx + ny * ny > 0.84:
                continue
            events.append(mix_event("round", under_orange, clean=True))
            events.append(
                stroke_event(
                    f"org_under_{gy}_{gx}",
                    "orange_underpaint",
                    "round",
                    diameter=72.0,
                    samples=pts_line(gx - 12, gy, gx + 12, gy, steps=2, p0=0.95, p1=0.95),
                    round_pressure=True,
                )
            )
    events.append({"type": "dry_canvas"})

    orange_deep_sh = [0.42, 0.0, 0.02, 0.0, 0.0, 0.26, 0.30, 0.0]
    orange_core_sh = [0.66, 0.0, 0.0, 0.0, 0.0, 0.08, 0.26, 0.0]
    orange_mid = [0.82, 0.0, 0.0, 0.0, 0.0, 0.01, 0.17, 0.0]
    orange_lit = [0.87, 0.0, 0.0, 0.03, 0.0, 0.0, 0.10, 0.0]
    orange_high = [0.78, 0.0, 0.0, 0.15, 0.0, 0.0, 0.07, 0.0]

    # Smooth overlapping grid inside ellipse (no concentric rings!)
    for gy in range(int(cy - ry + 24), int(cy + ry - 18), 38):
        for gx in range(int(cx - rx + 24), int(cx + rx - 18), 38):
            nx = (gx - cx) / rx
            ny = (gy - cy) / ry
            if nx * nx + ny * ny > 0.86:
                continue
            ndotl = nx * lx + ny * ly
            if ndotl < -0.22:
                rec = orange_deep_sh
            elif ndotl < 0.04:
                rec = orange_core_sh
            elif ndotl < 0.34:
                rec = orange_mid
            elif ndotl < 0.58:
                rec = orange_lit
            else:
                rec = orange_high

            # Stroke tangent / along latitude of sphere
            tx = -ny * 26.0 + 18.0
            ty = nx * 22.0 + 8.0
            events.append(mix_event("round", rec, clean=True))
            events.append(
                stroke_event(
                    f"org_fill_{gy}_{gx}",
                    "orange_body",
                    "round",
                    diameter=74.0,
                    samples=pts_line(gx - tx * 0.5, gy - ty * 0.5, gx + tx * 0.5, gy + ty * 0.5, steps=3, p0=0.92, p1=0.92),
                    round_pressure=True,
                )
            )

    # Clean smooth contour pass around the perimeter
    n_edge = 64
    for k in range(n_edge):
        a0 = (k / n_edge) * math.tau
        a1 = ((k + 1.1) / n_edge) * math.tau
        am = 0.5 * (a0 + a1)
        nx, ny = math.cos(am), math.sin(am)
        ndotl = nx * lx + ny * ly
        rec = orange_deep_sh if ndotl < -0.2 else (orange_core_sh if ndotl < 0.05 else (orange_mid if ndotl < 0.38 else orange_lit))
        events.append(mix_event("round", rec, clean=True))
        events.append(
            stroke_event(
                f"org_edge_{k}",
                "orange_body",
                "round",
                diameter=56.0,
                samples=pts_line(
                    cx + (rx - 22) * math.cos(a0),
                    cy + (ry - 22) * math.sin(a0),
                    cx + (rx - 22) * math.cos(a1),
                    cy + (ry - 22) * math.sin(a1),
                    steps=3,
                    p0=0.9,
                    p1=0.9,
                ),
                round_pressure=True,
            )
        )

    events.append({"type": "dry_canvas"})

    # Subtle peel impasto dabs (smaller, lower pressure, clustered naturally)
    for i in range(60):
        ang = rng.uniform(0, math.tau)
        rad = math.sqrt(rng.uniform(0.05, 0.86))
        px = cx + rx * rad * math.cos(ang)
        py = cy + ry * rad * math.sin(ang)
        ndotl = (rad * math.cos(ang)) * lx + (rad * math.sin(ang)) * ly
        rec = orange_lit if ndotl > 0.25 else (orange_mid if ndotl > -0.1 else orange_core_sh)
        events.append(mix_event("round", rec, clean=True))
        events.append(
            stroke_event(
                f"peel_{i}",
                "orange_peel",
                "round",
                diameter=rng.uniform(10.0, 17.0),
                samples=pts_line(px, py, px + rng.uniform(-6, 6), py + rng.uniform(-4, 8), steps=2, p0=0.55, p1=0.40),
                round_pressure=True,
            )
        )

    # Soft golden halo + crisp dimpled specular highlight around (452, 372)
    spec_halo = [0.70, 0.0, 0.0, 0.24, 0.0, 0.0, 0.06, 0.0]
    spec_bright = [0.35, 0.0, 0.0, 0.61, 0.0, 0.0, 0.04, 0.0]
    events.append(mix_event("soft_mop", spec_halo, clean=True))
    events.append(
        stroke_event(
            "spec_halo_0",
            "orange_highlight",
            "soft_mop",
            diameter=92.0,
            samples=pts_line(438, 352, 466, 392, steps=4, p0=0.45, p1=0.40),
            auto_follow=False,
        )
    )
    for i in range(18):
        hx = 452.0 + rng.uniform(-26.0, 26.0)
        hy = 372.0 + rng.uniform(-34.0, 34.0)
        dist = math.hypot((hx - 452.0) / 26.0, (hy - 372.0) / 34.0)
        rec = spec_bright if dist < 0.65 else spec_halo
        events.append(mix_event("round", rec, clean=True))
        events.append(
            stroke_event(
                f"spec_{i}",
                "orange_highlight",
                "round",
                diameter=rng.uniform(9.0, 15.0),
                samples=pts_line(hx, hy, hx + rng.uniform(-3, 4), hy + rng.uniform(2, 7), steps=2, p0=0.75, p1=0.50),
                round_pressure=True,
            )
        )

    # Calyx dimple creases & short olive stem at (548, 250)
    calyx_creases = [0.35, 0.0, 0.0, 0.0, 0.0, 0.40, 0.25, 0.0]
    for a_deg in [-145, -100, -45, 10, 65]:
        rad = math.radians(a_deg)
        events.append(mix_event("round", calyx_creases, clean=True))
        events.append(
            stroke_event(
                f"crease_{a_deg}",
                "stem",
                "round",
                diameter=8.0,
                samples=pts_line(548, 252, 548 + 22 * math.cos(rad), 252 + 14 * math.sin(rad), steps=3, p0=0.65, p1=0.35),
                round_pressure=True,
            )
        )

    stem_olive = [0.55, 0.0, 0.0, 0.08, 0.0, 0.15, 0.0, 0.22]
    stem_tip = [0.45, 0.0, 0.0, 0.28, 0.0, 0.15, 0.0, 0.12]
    events.append(mix_event("round", stem_olive, clean=True))
    events.append(
        stroke_event(
            "stem_body",
            "stem",
            "round",
            diameter=15.0,
            samples=pts_line(548, 252, 545, 238, steps=3, p0=0.90, p1=0.85),
            round_pressure=True,
        )
    )
    events.append(mix_event("round", stem_tip, clean=True))
    events.append(
        stroke_event(
            "stem_top",
            "stem",
            "round",
            diameter=10.0,
            samples=pts_line(544, 238, 548, 236, steps=1, p0=0.85, p1=0.85),
            round_pressure=True,
        )
    )
    return events


def main():
    rng = random.Random(42)
    client = ServeClient()
    try:
        ping = client.call({"op": "ping"})
        print("Connected to --serve:", ping)

        rec_init = {
            "format": "pigment-studio-recording-v2",
            "lighting": {
                "enabled": True,
                "impasto_depth": 1.0,
                "canvas_weave": 0.35,
                "specular_sheen": 0.85,
                "light_angle_deg": 315.0,
                "light_elevation_deg": 45.0,
            },
            "tile": False,
            "events": [],
        }
        client.call({"op": "load_recording", "recording": rec_init})

        bg_events = build_background_and_table(rng)
        res1 = client.call({"op": "apply", "events": bg_events})
        print("Applied background & table:", res1["events"], "events")

        fg_events = build_cast_shadow_and_orange(rng)
        res2 = client.call({"op": "apply", "events": fg_events})
        print("Applied orange & shadow:", res2["events"], "total events")

        # Demonstrate Tier A live inspection + edit_stroke refinement
        org_mid = client.call({"op": "inspect_region", "x": 470, "y": 460, "w": 80, "h": 80})
        print("Orange mid sRGB:", [round(v, 1) for v in org_mid["mean_srgb"]])

        # Nudge stem_top slightly into alignment via move_stroke
        client.call({"op": "move_stroke", "id": "stem_top", "dx": -1.0, "dy": 1.0})

        rec_out = ROOT / "demos" / "orangegen-painting.json"
        pigment_out = ROOT / "demos" / "orangegen-painting.pigment"
        png_out = ROOT / "demos" / "orangegen-painting.png"

        client.call({"op": "save_recording", "path": str(rec_out)})
        client.call({"op": "render", "out": str(pigment_out)})
        client.call({"op": "render", "out": str(png_out), "mode": "color"})
        print("Saved:", rec_out.name, pigment_out.name, png_out.name)
    finally:
        client.close()


if __name__ == "__main__":
    main()
