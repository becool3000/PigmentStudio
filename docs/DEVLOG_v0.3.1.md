# Pigment Studio v0.3.1 — Studio Essentials Calibration, Brush Refinements & Agent Pipeline

**v0.3.1** is a major release focused on physically calibrated oil pigments, refined 6-brush handling, and a full headless/agent-ready asset & stroke-editing pipeline (covering all changes since `v0.2.0`).

## Calibrated Studio Essentials (8/8) & Mixing Matrix Demo
- **All 8 Studio Essentials calibrated** against published spectral/colorimetric data with CPU/GPU DX12 parity:
  - **Phthalo Blue (`PB15:2`)**, **Titanium White (`PW6`)**, **Hansa Yellow Light (`PY3`)**, **Quinacridone Magenta (`PR122`)**, **Cadmium Red Medium (`PR108`)**, **Ultramarine Blue (`PB29`)**, **Burnt Sienna (`PBr7`)**, and **Bone Black (`PBk9`)**.
- **True Kubelka-Munk jar swatches:** Built-in rack jars now compute their colors directly from the spectral engine so jars match painted swatches.
- **Studio Essentials Mixing Matrix demo:** Open **File → Load Mixing Matrix Demo** to explore an 8×8 grid of pure masstones, 50/50 mixes, and Titanium White-cut undertones (and save your own copy with **Save / Save As**).

## Brush Physics & Desktop Polish
- **6-brush oil pack refinements:**
  - Independent brush reservoirs (cleaning or dipping one brush no longer overwrites the others).
  - Swept **Palette Knife** & **Flat** bristle contact, **Round** pressure/taper curves, **Filbert** turns and tapers, and **Fan** tuft spread and lift.
  - Legacy `v1` recordings preserve original shared-reservoir routing via `legacy_reservoir`.
- **Window placement:** Automatically remembers and restores the last normal Studio window position and size across launches.

## Game-Asset Exports & Headless Pipeline (`Phase E` + `v0.3.x` Upgrades)
- **Reachable dried underlayers:** `File → Import Dried Underlayer…`, `underlayer_path` / `import_underlayer` in recording JSON, and `--underlayer <img.png>` on `--render`.
- **Deterministic batch rendering:** `--batch <manifest.json>` reuses a single DX12 `PaintGpu` instance across frame sequences and per-frame underlayers.
- **Multi-mode `--render` & normal convention:**
  - `--mode color,height,normal,transparent` exports multiple maps in one pass (with `{mode}` pattern support in `--out`).
  - `--normal-convention gl|dx` (`gl` flips the green channel for Godot, Blender, and Unity URP).
  - `--smooth-height` removes the ~3.2 px deposit tooth grid in height and normal exports.
  - `--export-palette <out.json|out.gpl>` exports the pigment palette headlessly.
- **Transparent export edge fix:** Stabilized low-alpha un-premultiplying so dark strokes no longer show a warm/light edge hairline over black backgrounds.
- **Per-stroke `set_tile` fix:** `set_tile` events now apply per-stroke during replay when a stroke omits `"tile"`.

## Persistent Agent Daemon (`--serve`) & Non-Destructive Stroke Editing
- **`--serve` (JSON-RPC over `stdin`/`stdout`):** Keeps one DX12 `PaintGpu` instance and GPU checkpoints warm in memory for fast iterative painting.
- **Tier A stroke editing:** Assigns/preserves stroke `id` and `group` tags and supports `move_stroke`, `edit_stroke`, `delete_stroke`, and `reorder_stroke` with checkpointed GPU replay.
- **Spatial & optical inspection:** `strokes_at`, `inspect_pixel`, `inspect_region`, and `brush_state`.
- **Ergonomic recording events:** `dip_rack { slot, clean: true }`, `set_mix { brush, recipe, clean }`, and documented schema in [RECORDING_FORMAT.md](RECORDING_FORMAT.md) + [recording-v1.schema.json](recording-v1.schema.json).

## Package Verification
- **Windows x64 ZIP:** `pigment-studio-v0.3.1-windows-x64.zip`
- **SHA-256:** `3bf55579749391680b6dd8b0cccb8633d622df46e21bdf8a8fe4a2fa51c34fa1`
