# Build and validation

Run these commands from the project root with the Rust MSVC toolchain and Windows C++ build tools installed.

> [!IMPORTANT]
> **Only One Build at a Time (Windows File Locking):**
> Never start a compile or test build while the application window is open. Windows locks active `.exe` binaries on disk, causing `os error 5 (Access is denied)` during linking. Always close the running instance (or ask the user to close it) before running `cargo build` or `cargo test`.

```powershell
cargo fmt --check
cargo clippy --all-targets
cargo test --locked
cargo run --release --locked -- --benchmark
cargo run --release --locked
```

## Automated test suite

The automated suite (`cargo test --locked`) exercises both CPU math and GPU Direct3D 12 compute shaders:

- **`tests::canvas_zoom_anchors_at_the_cursor_and_pan_moves_the_view`**: Verifies viewport projection, zoom cursor stability, and clamped pan limits.
- **`tests::undo_restores_swatches_and_load_state`**: Verifies that CPU session state (swatches and brush load percentages) accurately restores upon undo.
- **`tests::dx12_oil_workbench`**: An exhaustive Direct3D 12 test executing on your GPU adapter. It validates:
  - Spectral Kubelka-Munk reflectance against CIE 1931 D65 reference values.
  - Pigment mass conservation across brush pickup and deposition passes.
  - Palette dab placement and collision checking (`CheckTubes`).
  - Flat brush 3-lane transfer and orientation differences (0° vs 90°).
  - Swatch sampling and extraction.
  - Ink pooling, fluid gravity dripping (`AdvanceInk`), and drying transitions.
  - GPU undo snapshot restoration and sRGB 1024×1024 PNG export.

## Performance benchmark

The release benchmark tests continuous rendering under heavy input load:

```powershell
cargo run --release --locked -- --benchmark
```

- Seeds a wet underpainting with a preloaded 64 px brush.
- Dispatches 1,200 synthetic inputs at 120 Hz over 10 seconds.
- Enforces a **p95 total frame latency target of <= 16.7 ms** (consistent 60+ FPS).
- Reports stage percentiles: acquire, encode, GPU and submit, and present.

## Windows tablet and interactive checks

Run these hands-on checks with a pen tablet or mouse:

1. **Display scaling:** Verify canvas and palette alignment at 100% and 150% Windows DPI scaling.
2. **Pressure ramp:** Draw with a light touch and ramp up to full pressure; confirm smooth bristle spread without stepped jumps.
3. **Boundary handling:** Start drawing on canvas, drag off the window boundary or over docked chrome, and re-enter while pressed. Verify that strokes terminate cleanly without ghost lines or trapped state.
4. **Flat brush angle:** Set angle to 0° and draw a vertical stroke; set to 90° and draw along the same path. Verify that 90° presents the broad edge and creates a wider mark.
5. **Wipe rag and clean brush:** Swipe across the wipe rag to reduce paint load by 10% per swipe; click **Clean brush** to purge all paint back to 0% (Empty). Verify the load bar and percentage text update accordingly.
6. **Ink pen dripping:** Switch to **Ink Pen**, press down firmly (>56% pressure), and pause. Verify that ink pools at the tip and continues dripping downward under gravity after lifting.
7. **Undo:** Perform painting, swatching, and dabbing actions; press `Ctrl+Z` or click **Undo** up to 8 times to confirm step-by-step restoration.
8. **Rapid pen input:** Start a stroke with very light pressure, then press harder without lifting. Lift while moving sideways and hover across the canvas. Confirm that paint stops at the last point of contact. Tap a color jar, make a short stroke immediately, then repeat with other jars and the Clean brush control. Confirm that each new stroke starts with the latest brush load.

## Troubleshooting

- **Direct3D 12 adapter initialization fails:** Check graphics driver updates. Ensure the Windows display adapter supports Direct3D 12 Feature Level 11_0 or higher.
- **Tablet pressure not recognized:** Ensure the tablet driver supports Windows Ink / `WM_POINTER` APIs. If input falls back to mouse mode, strokes will use default mouse pressure (65% for oil, 87% for ink).
- **Shader compilation error:** Inspect WGSL syntax in `src/paint.wgsl`. If validation errors occur, run with `RUST_BACKTRACE=1` to pinpoint the failing pipeline descriptor.
