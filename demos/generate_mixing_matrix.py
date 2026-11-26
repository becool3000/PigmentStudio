import json
import pathlib

# Studio Essentials in rack order:
# (name, channel_index)
# Channel indices: 0=PY3, 1=PB15:2, 2=PR122, 3=PW6, 4=PB29, 5=PBr7, 6=PR108, 7=PBk9
ESSENTIALS = [
    ("Titanium White (PW6)", 3),
    ("Hansa Yellow Light (PY3)", 0),
    ("Cadmium Red Medium (PR108)", 6),
    ("Quinacridone Magenta (PR122)", 2),
    ("Phthalo Blue (PB15:2)", 1),
    ("Ultramarine Blue (PB29)", 4),
    ("Burnt Sienna (PBr7)", 5),
    ("Bone Black (PBk9)", 7),
]


def add_swatch(events, x, y, diameter, recipe, dabs=5):
    events.append({"type": "equip_rack", "slot": 19, "recipe": recipe})
    events.append({"type": "service", "brush": "round", "action": {"kind": "wipe", "fraction": 0}})
    events.append({"type": "service", "brush": "round", "action": {"kind": "dip_rack", "slot": 19}})
    for _ in range(dabs):
        events.append({
            "type": "stroke",
            "surface": "canvas",
            "brush": "round",
            "diameter": diameter,
            "angle_deg": 0,
            "tile": False,
            "samples": [{"x": x, "y": y, "pressure": 1.0, "timestamp_ms": 0}],
        })


def main():
    events = []
    # Grid layout on 1024x1024:
    # Margin headers at x=46, y=46 (34px pure pigment dots)
    # 8x8 matrix in [120..960], pitch=108, swatch diameter=84
    origin = 148
    pitch = 108

    for idx, (_, ch) in enumerate(ESSENTIALS):
        pure = [0.0] * 8
        pure[ch] = 1.0
        pos = origin + idx * pitch
        add_swatch(events, pos, 48, 36, pure, dabs=4)
        add_swatch(events, 48, pos, 36, pure, dabs=4)

    for r, (_, ch_r) in enumerate(ESSENTIALS):
        for c, (_, ch_c) in enumerate(ESSENTIALS):
            recipe = [0.0] * 8
            if r == c:
                # Diagonal: pure masstone
                recipe[ch_r] = 1.0
            elif c > r:
                # Upper triangle: 50/50 direct mix
                recipe[ch_r] += 0.5
                recipe[ch_c] += 0.5
            else:
                # Lower triangle: 1:1 pair cut with 50% Titanium White (PW6 = ch 3)
                recipe[ch_r] += 0.25
                recipe[ch_c] += 0.25
                recipe[3] += 0.50

            x = origin + c * pitch
            y = origin + r * pitch
            add_swatch(events, x, y, 86, recipe, dabs=5)

    recording = {
        "format": "pigment-studio-recording-v1",
        "lighting": {
            "enabled": True,
            "impasto_depth": 1.0,
            "canvas_weave": 0.35,
            "specular_sheen": 0.85,
            "light_angle_deg": 315.0,
            "light_elevation_deg": 45.0,
        },
        "tile": False,
        "events": events,
    }

    out_path = pathlib.Path(__file__).with_name("studio-essentials-mixing-matrix.json")
    out_path.write_text(json.dumps(recording, indent=2), encoding="utf-8")
    print(f"Wrote {out_path} ({len(events)} events)")


if __name__ == "__main__":
    main()
