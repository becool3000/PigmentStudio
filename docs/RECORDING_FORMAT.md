# Pigment Studio Recording & Headless CLI Reference

Use `pigment-studio-recording-v2` for new Direct3D 12 recordings. Each oil brush keeps its own paint. The [recording schema](recording-v1.schema.json) also covers v1, which replays with shared Round and Flat reservoirs. When Studio extends a v1 recording, it saves the imported strokes and service actions with `legacy_reservoir: true` so they still replay with their original paint routing.

## Headless CLI Commands

### Single or Multi-Mode Render
```powershell
pigment-studio-ink.exe --render strokes.json --out out_{mode}.png --mode color,height,normal --normal-convention gl --smooth-height --underlayer render_pass.png
```
* `--mode`: One or more comma-separated modes: `color` (default), `transparent`, `height`, `normal`. When multiple modes are passed, `{mode}` in `--out` is replaced (or `_{mode}` is appended before `.png`).
* `--normal-convention`: `dx` (default, DirectX Y-down) or `gl` (OpenGL Y-up, for Godot/Blender/Unity URP).
* `--smooth-height`: Smooths the ~3.2 px deposit tooth grid in `height` and `normal` exports.
* `--underlayer`: Optional PNG loaded onto `dry_substrate` before events run (overrides `underlayer_path` in JSON).

### Palette Export
```powershell
pigment-studio-ink.exe --export-palette palette.json
pigment-studio-ink.exe --export-palette palette.gpl
```

### Ergonomic Agent Events (`v2`)
* `dip_rack` accepts `"clean": true` to wipe the brush clean before dipping.
* `set_mix` loads a custom 8-channel recipe onto a brush in one event:
  ```json
  { "type": "set_mix", "brush": "round", "recipe": [0.5, 0.5, 0, 0, 0, 0, 0, 0], "clean": true }
  ```
* `stroke` events accept optional `"id"` and `"group"` string tags for non-destructive stroke tracking.

### Deterministic Batch Render (Animation / Overpaint)
Initializes Direct3D 12 and `PaintGpu` **once**, then renders every job deterministically (`Command::Initialize` + `equip_cabinet` between frames):
```powershell
pigment-studio-ink.exe --batch manifest.json
```

`manifest.json` format (`pigment-studio-batch-v1`):
```json
{
  "format": "pigment-studio-batch-v1",
  "shared_recording": "glaze_pass.json",
  "jobs": [
    { "underlayer": "frames/frame_0001.png", "out": "out/frame_0001.png", "mode": "color" },
    { "underlayer": "frames/frame_0002.png", "out": "out/frame_0002.png", "mode": "color" }
  ]
}
```
Each job may specify its own `"recording"`, or inherit `"shared_recording"`. Paths are resolved relative to the manifest file.

---

## Tube Indices (`load` / `dip` `tube: 0..7`)

| `tube` | Pigment | Code | Role |
| --- | --- | --- | --- |
| `0` | Hansa Yellow Light | PY74 / PY3 | Bright primary yellow |
| `1` | Phthalo Blue | PB15 | High-tinting transparent cyan-blue |
| `2` | Quinacridone Magenta | PR122 | Transparent cool violet-red |
| `3` | Titanium White | PW6 | High-hiding opaque white |
| `4` | Ultramarine Blue | PB29 | Semi-transparent warm blue |
| `5` | Burnt Sienna | PBr7 | Warm transparent earth brown |
| `6` | Cadmium Red / Vermilion | PR108 / PR106 | Opaque warm scarlet red |
| `7` | Bone Black / Sepia | PBk9 | Deep neutral black |

---

## Atelier Rack Slots (`dip_rack` `slot: 0..19`)

The workbench shows Studio Essentials (slots 0–7). Slots 8–19 are reserved for existing recordings and future calibration.

| `slot` | Name | Code | Opacity |
| --- | --- | --- | --- |
| `0` | Titanium White | PW6 | Opaque (High hiding) |
| `1` | Hansa Yellow Light | PY3 | Semi-transparent |
| `2` | Cadmium Red Medium | PR108 | Opaque (High covering) |
| `3` | Quinacridone Magenta | PR122 | Transparent (Glaze) |
| `4` | Phthalo Blue | PB15 | Transparent (High staining) |
| `5` | Ultramarine Blue | PB29 | Semi-transparent |
| `6` | Burnt Sienna | PBr7 | Semi-transparent |
| `7` | Bone Black | PBk9 | Semi-transparent |
| `8` | Genuine Sepia (Squid Ink) | Natural Marine Melanin | Semi-transparent (Marine glaze) |
| `9` | Genuine Vermilion (Cinnabar) | PR106 | Opaque (Heavy mineral) |
| `10` | French Yellow Ochre | PY43 | Semi-opaque |
| `11` | Cyprus Raw Umber | PBr7 | Semi-transparent |
| `12` | Rose Madder Genuine | NR9 | Transparent (Gem-like glaze) |
| `13` | Italian Terra Verte (Green Earth) | PG23 | Transparent (Delicate glaze) |
| `14` | Lapis Lazuli (Fra Angelico Blue) | Natural Lazurite | Semi-transparent (Crystalline) |
| `15` | 24K Shell Gold in Oil | Genuine Gold Leaf (Au) | Opaque (Metallic reflectance) |
| `16` | Tyrian Imperial Purple | Natural Murex | Transparent (Royal crimson glaze) |
| `17` | Cobalt Violet Deep | PV14 | Semi-transparent (Crystalline) |
| `18` | Mummy Brown (Caput Mortuum) | Pitch & Asphaltum | Transparent (Rich asphaltum glaze) |
| `19` | Dragon's Blood Resin | Natural Dracaena | Transparent (Amber crimson glaze) |

---

## Brush Profiles (`BrushKind`)

| `brush` | Directional | Behavior |
| --- | --- | --- |
| `round` | No | Classic soft-edge round oil brush |
| `flat` | Yes | Hog-bristle flat with deep persistent furrows & dry-brush skip |
| `filbert` | Yes | Almond/oval tip with soft tapered corners & bristle grooves |
| `fan` | Yes | Curved arc of 9 separated bristle tufts |
| `palette_knife` | Yes | Sharp steel blade; thick opaque impasto when loaded, 4x scrape/smear when unloaded |
| `soft_mop` | No | Feathered radial blender with high pickup for softening wet edges |

Set `"ink": true` on a `stroke` for fluid dripping ink, or `"charcoal": true` (`"erase": true` to erase) to draw on the Sketch layer.

### Continuous palette knife strokes

Set `"auto_follow": true` on a knife stroke to steer the blade across the path. The first moving pass starts perpendicular to travel. A stationary tap uses the stroke's `angle_deg`.

Set `"knife_sweep": true` to integrate the blade's contact between samples. Pickup and deposit follow the distance traveled. New desktop knife strokes record this flag for both manual and automatic steering. Existing recordings default to their original dab behavior.

For a deliberate turn, set `angle_deg` on individual samples. A sample angle overrides auto-follow at that point and the blade interpolates along the shortest rotation between samples. The angle describes the blade's long edge, with 0° horizontal and 90° vertical. Reduce `pressure` toward the end to taper the mark.

```json
{
  "type": "stroke",
  "surface": "canvas",
  "brush": "palette_knife",
  "knife_sweep": true,
  "diameter": 64,
  "samples": [
    { "x": 300, "y": 500, "pressure": 0.85, "angle_deg": 90 },
    { "x": 440, "y": 540, "pressure": 0.70, "angle_deg": 115 },
    { "x": 520, "y": 610, "pressure": 0.10, "angle_deg": 140 }
  ]
}
```

Use one stroke for the full sweep so the blade carries wet pigment forward across sample points.

### Split-loading the hog-bristle flat

Select **Flat** and choose **All**, **Left**, **Middle**, or **Right** under **Jar dip**. Clicking a built-in or custom jar loads the selected part of the tip. The lanes rotate with the brush.

In a recording, give a flat brush's `dip_rack` or `dip_custom` action an optional `lane`. For example, this loads Hansa Yellow Light onto the left edge:

```json
{"type":"service","brush":"flat","action":{"kind":"dip_rack","slot":1,"lane":"left"}}
```

For a three-color stroke, dip the opposite edge in Phthalo Blue (slot 4) and dip the middle in both jars. Use `"bristle_sweep": true` on the flat stroke. Broad pressure brings the bundles into contact and lighter pressure leaves broken bristle marks. Use Auto-follow or per-sample angles to turn the brush. New desktop flat strokes record the swept contact flag. Older recordings use their original footprint.

### Round brush pressure

Set `"round_pressure": true` on a Round stroke to use the desktop brush's refined pressure curve. Light pressure gives a smaller contact area and deposits less paint. A firm pass opens the tip, and easing pressure while moving tapers the mark. The `diameter` field still sets the brush size at full pressure. Include the last few positive-pressure samples before lifting to describe the taper.

### Filbert rolls and tapers

Set `"filbert_sweep": true` on a Filbert stroke to use the desktop brush's swept oval contact. Start with the long axis across the path for a broad mark. Turn the long axis toward the path with per-sample `angle_deg` to present the narrow edge, then ease `pressure` as you lift. Auto-follow holds the broad orientation through curves. The three paint lanes carry color through the turn.

### Fan brush tufts

Set `"fan_sweep": true` on a Fan stroke to use the desktop brush's nine spreading tufts. A light pass leaves separate streaks. Firm pressure splays the tufts into a continuous mark, and easing pressure separates them again near the lift. Use Auto-follow or per-sample angles to turn the fan through a curve.
