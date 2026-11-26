# Changelog

All notable changes to Pigment Studio are documented in this file.

## [Unreleased]

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
