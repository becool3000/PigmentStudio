# Pigment Studio Recording & Headless CLI Reference

Pigment Studio can be driven headlessly on Direct3D 12 using `pigment-studio-recording-v1` JSON files (schema: [recording-v1.schema.json](recording-v1.schema.json)).

## Headless CLI Commands

### Single Render
```powershell
pigment-studio-ink.exe --render strokes.json --out render.png --mode color --underlayer render_pass.png
```
* `--mode`: `color` (default), `transparent`, `height`, or `normal`.
* `--underlayer`: Optional PNG loaded onto `dry_substrate` before events run (overrides `underlayer_path` in JSON).

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
