# Project conventions

Use standard Rust naming. Files, modules, functions, and fields use `snake_case`. Types and traits use `PascalCase`. Constants use `SCREAMING_SNAKE_CASE`.

The project is structured as a single Cargo package (`pigment-prototype`) producing the binary `pigment-studio-ink`.

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | App entry, winit event loop, egui UI layout, canvas viewport zoom/pan, and stroke dispatch. |
| `src/gpu.rs` | wgpu Direct3D 12 device setup, command encoders, atlas layout, dab structs, and PNG export worker. |
| `src/paint.wgsl` | Compute shaders for Kubelka-Munk spectral evaluation, dab deposition, pigment pickup, and ink flow. |
| `src/pen.rs` | Win32 window subclassing, `WM_POINTER*` message handling, pressure extraction, and palm rejection. |
| `src/pigment.rs` | Optical constants (CIE 1931 2° observer, D65 illuminant, $K$ and $S$ anchor spectra, CPU RGB conversion). |
| `src/stroke.rs` | Stroke state machine, sample interpolation, dab spacing, flat brush orientation, and ink dwell detection. |
| `src/workbench.rs` | Workbench session state, 8-entry undo history, brush sizing/angle, swatch strip, and pigment guide metadata. |
| `src/benchmark.rs` | 120 Hz synthetic stroke benchmark tracking p95 latency across acquire, encode, GPU submit, and present. |
| `src/tests.rs` | Unit and integration tests (viewport math, undo restore, and full DX12 GPU test suite). |

## Coordinate spaces and surfaces

- **Atlas memory:** Canvas occupies `[0, 0, 1024, 1024]`, mixing palette occupies `[0, 1024, 512, 256]`, and swatches occupy `[512 + slot, 1024, 1, 1]`.
- **Display mapping:** Viewport coordinates map to physical canvas pixels via `CanvasView`. Stylus input coordinates from Win32 are converted to virtual canvas or palette space respecting window DPI `scale_factor`.
- **Hit testing:** Always clip interaction areas to visible screen bounds (e.g. `palette_clip`, `viewport`) and check overlay popups to prevent painting underneath floating modal cards.

## Performance and safety rules

- Only run one build at a time. Active Windows processes lock the output executable (`pigment-studio-ink.exe`), causing `os error 5 (Access is denied)` if a build is triggered while testing. Check for running app processes and close them (or notify the user) before invoking `cargo build` or `cargo test`.
- Keep the Win32 pointer subclass callback (`window_proc`) fast and non-blocking; do not allocate heavily inside input handlers.
- Maintain pigment mass conservation in compute passes (verified via `totals` checks in integration tests).
- Never block the UI thread on GPU readback or image encoding; run PNG exports on background threads communicating via `mpsc::channel`.
- Keep memory bounded: snapshot queues must discard old states beyond `HISTORY` (8 steps).
- Follow [TESTING.md](TESTING.md) for local verification before committing changes.
