# Changelog

All notable changes to Pigment Studio are documented in this file.

## [0.3.2] - 2026-11-27

### Added
- **4K UHD & Custom Canvas Dimensions (`128 × 128` to `4096 × 4096`):** Dynamic `Canvas → Canvas Size…` dialog, adaptive undo history, `--benchmark-size WxH`, and full `.pigment` / recording support.
- **Jackson Pollock Splatter / Sling Tool:** Velocity- and whip-driven ballistic paint splatter and poured enamel strings with 5 one-click presets (*Pollock Whip*, *Fine Mist*, *Heavy Enamel*, *Dry Flick*, *Matrix Plow*), inline **Gravity Drip** toggle, and up to `128 px` droplet size.
- **GPU Paint Gravity & Viscosity Simulation:** Mass-conserving real-time WGSL drip and flow with 6 viscosity presets and directional tilt.
- **Live Diagnostic FPS / Perf HUD:** Viewport overlay (`⚡ FPS · ms · W×H · Tool`) toggleable via `View → Show FPS / Perf HUD` or the toolbar `⚡ FPS` pill.
- **4K UHD Mixing Matrix Demo:** Added `3840 × 2160` 8×8 Kubelka-Munk swatch graph (`uhd_mixing_matrix_4k.pigment`).
- **MIT `pigment-engine` Crate & Blender Extension:** Extracted core Kubelka-Munk / WGSL library (`crates/pigment-engine`) and native C-ABI Blender 4.2+ bridge (`crates/pigment-blender`).

### Changed
- **280–450 FPS Across Every Brush at 4K UHD:** Extended single-dispatch swept-capsule optimization to **Soft Mop** (`3.37 ms / ~297 FPS`) and tuned directional sweeps for **Palette Knife** (`2.22 ms / ~450 FPS`), **Round** (`2.54 ms / ~394 FPS`), **Fan** (`3.38 ms / ~296 FPS`), **Filbert** (`3.50 ms / ~286 FPS`), and **Flat** (`3.52 ms / ~284 FPS`).
- **Larger Oil Brush Sizes:** Expanded oil brush maximum diameter from `96 px` to `300 px`.

## [0.3.1] - 2026-11-26

### Added
- **Swept Brush Physics & `--serve` Tier A Agent Pipeline:** Continuous swept-capsule strokes for Round, Flat, Filbert, Fan, and Palette Knife, plus persistent `--serve` JSON-RPC editing, spatial inspection, and multi-mode `--render` (`color`, `height`, `normal`, `transparent`). See [docs/DEVLOG_v0.3.1.md](docs/DEVLOG_v0.3.1.md).

## [0.2.0] - 2026-11-25

### Added
- **Game-Asset Exports:**
  - **Transparent PNG:** Exports the Paint layer with coverage/mass-derived alpha and hue-preserving linear un-premultiplication (excluding white gesso and reference/sketch layers).
  - **Height & Normal Maps:** Exports Phase 3 Impasto surface height (grayscale PNG) and central-difference tangent-space normal maps (PNG).
  - **Palette Export:** Exports occupied custom jars (pigment masses, ratios, sRGB, and hex) to JSON or GIMP/Krita `.gpl` palettes.
- **Seamless Tiling Mode (`T`):** Toroidal coordinate wrapping for oil, ink, and charcoal strokes as well as 3D Impasto surface normals across the 1024×1024 canvas edges.
- **3-Layer Studio Stack (Reference → Sketch → Paint):**
  - **Reference Layer:** Non-destructive PNG reference import (`File → Import Reference Image…`), `👁 Ref` visibility toggle (`R`), and opacity control.
  - **Sketch Layer:** Dedicated charcoal/graphite buffer with **Charcoal Pencil** (pressure-sensitive linen-tooth grain), **Charcoal Eraser**, opacity control, and undo support.
  - **Layers Panel:** Workbench dock panel to switch active target (**Paint** vs. **Sketch**), toggle visibility, adjust opacity, and clear individual layers.
- **JSON Stroke Recording, Replay & Headless CLI:**
  - Live `pigment-studio-recording-v1` JSON recording with **Save/Replay Recording (JSON)…**.
  - Headless DX12 CLI rendering: `--render <recording.json> --out <output.png> [--mode color|transparent|height|normal]`.
- **64-Step Deep Undo (`Ctrl+Z`):** Combines the 8-slot GPU snapshot ring (with lazy buffer allocation) and deterministic recording replay for up to 64 undo steps.
- **`.pigment` Project Save & Load (`Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S`):** Lossless `PIGMENT1` binary container with `u32` PackBits RLE compression for all GPU buffers, workbench state, layers, lighting, and stroke recording.
- **Traditional Menu Bar & Quick-Bar:** `File`, `Edit`, `Canvas`, `Layer`, `Tools`, `View`, and `Help` menus, `Keyboard Shortcuts…` and `About` dialogs, and a compact quick-bar for frequent painting actions.

### Changed
- Updated [ROADMAP.md](ROADMAP.md) and [TASKS.md](TASKS.md) with the community feedback roadmap and completed milestones.

## [0.1.0]

### Added
- Initial open-source release under `GPL-3.0-only`.
- 31-band Kubelka-Munk spectral mixing, Saunderson surface boundary optics, and 20-pigment Atelier Rack.
- `Dry canvas` (`Ctrl+D`) optical strata transfer, 3D Impasto relief & studio lighting, fluid ink simulation, Win32 `WM_POINTER` stylus input, and pinned diagnostic notes.
