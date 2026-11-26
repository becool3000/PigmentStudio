# Current tasks

Status: v0.1 prototype is implemented and passes all automated tests and benchmarks. Ready for hands-on pen tablet validation and new feature implementation.

## First priority — Windows validation & regression verification

- [x] Implement DX12 compute shader paint pipeline and Kubelka-Munk spectral mixing.
- [x] Wire up Win32 pointer subclassing for stylus pressure, position, and palm rejection.
- [x] Implement round brush, flat brush with 3-lane transfer and angle control, and ink pen dripping.
- [x] Build mixing palette, jar dipping/dabbing, wipe rag, rinse cup, swatch strip, and 8-level undo.
- [x] Implement PNG export and synthetic 120 Hz benchmark.
- [x] Run automated test suite (`cargo test --locked`) and verify clean clippy/build.
- [x] Eliminate extra console window on startup with Windows GUI subsystem.
- [x] Open Studio with the captured first-launch layout and restore the last normal window within the available Windows work area.
- [x] Implement 4-dip (25% step) brush capacity with tip-inward reservoir fill.
- [x] Add brush card mini progress bar, live percentage readout, and multi-lane paint hue rendering.
- [x] Streamline brush maintenance: Wipe rag knocks off 10% per swipe; Clean brush clears to 0%; remove Rinse cup.
- [x] Give Clean brush a 44-point, full-width target below the wipe rag.
- [x] Always send a GPU clear from Clean brush and keep separate pigment reservoirs and rack previews for the six oil brushes.
- [x] Scaled brush capacity 10x (184,000 max load) for long, multi-stroke painting runs.
- [x] Scaled mixing palette dab volume 10x (45.0 mass per pixel) for deep, reusable paint puddles.
- [x] Implemented "Fill Jar" and functional Custom Jars (up to 64 slots) with dipping, dabbing, and safe deletion.
- [x] Expanded workbench to an 8-slot Atelier Active Rack with live optical GPU jar rendering.
- [x] Implemented The Paint Cabinet with 3 drawers (Studio Essentials, Natural & Historic, Precious & Mythic) containing 20 curated historical, scientific, and precious pigments.
- [x] Built tactile game-inventory drag-and-drop system and 1-click slot equipping with golden drop-target indicators.
- [x] Added Curator's Dossiers showing real-world valuations, pigment codes, opacities, and historical/scientific lore.
- [x] Replaced the Paint Cabinet and equip slots with one scrollable, 20-pigment Atelier Rack. Removed Dab buttons from built-in and custom jar rows so artists place paint on the palette with the loaded brush.
- [x] Enlarged Atelier Rack jars, names, and tap targets in a two-column layout that adapts to narrow windows.
- [x] Redraw all six brush profiles and the built-in and custom jars. Show the active tool and layer at a glance and keep brush paint colour paired with a separate load meter.
- [x] Correct the extra sRGB conversion in the desktop canvas and atlas previews so the displayed paint and PNG export use the same colour values.
- [x] Integrated asynchronous procedural Win32 audio feedback (drawer slides, jar seating clacks, brass latch toggles).
- [x] Implemented 8-channel physical Kubelka-Munk spectral curves for Genuine Vermilion / Cadmium Red (PR106/PR108) and Bone Black / Genuine Sepia (PBk9/Melanin).
- [x] Eliminated rack jar pale pastel wash by rendering authentic crisp pigment silhouettes with glass borders.
- [x] Dedicated fluid ink simulation to independent `ink_canvas` buffer with undo history snapshotting, freeing all 8 atlas floats for 100% pure oil pigment mass conservation.
- [x] Phase 1: Implement closed-form finite-thickness Kubelka-Munk film equations across 31 spectral bands in WGSL and CPU colorimetry.
- [x] Phase 1: Implement Saunderson surface boundary optics (k2=0.60) modeling linseed oil internal reflection and diffuse body reflectance.
- [x] Phase 1: Eliminate chalky pastel wash on canvas strokes, achieving pure saturated Cadmium Red [253, 0, 0], Vermilion [254, 10, 0], Rose Madder [253, 55, 16], and Dragon's Blood [246, 53, 0].
- [x] Phase 1: Verify 120 Hz benchmark passes with 2.10 ms p95 frame latency (470+ FPS) on Direct3D 12.
- [x] Phase 2: Implement dedicated `dry_canvas` GPU storage buffer (32 MB) and `Command::DryCanvas` (mode 14) to lock wet canvas paint into an immovable dry substrate while keeping the palette wet.
- [x] Phase 2: Implement two-layer finite-thickness Kubelka-Munk substrate transfer optics ($D = (R_{\infty,\text{wet}} - R_{\text{sub}}) / (1 - R_{\text{sub}} R_{\infty,\text{wet}})$) in WGSL display shaders and CPU colorimetry.
- [x] Phase 2: Calibrate Titanium White scattering ($S \in [18, 28]$) for physical hiding power when painting opaque clouds over dark dried underpaintings.
- [x] Phase 2: Add "☀️ Dry canvas" UI button, `Ctrl+D` shortcut, tactile brass audio feedback, and full 8-level undo/redo support.
- [x] Phase 2: Verify layer separation, non-bleed overpainting, and undo restoration with hardware DX12 unit tests and 120 Hz benchmark (p95 11.16 ms).
- [x] Step 1: Implement Pinned Notes & GPU Diagnostic Inspection system (`src/notes.rs`, `notes.json`).
- [x] Step 1: Add Note Tool (`N` key) with canvas pin badges (`①`, `②`, `✓`) and interactive egui popovers.
- [x] Step 1: Implement direct GPU diagnostic sampling (`PaintGpu::sample_diagnostics`) capturing wet/dry 8-channel pigment masses, bristle loads, rendered sRGB, and intimate mixture optical comparison.
- [x] Step 1: Add quick F12 diagnostic dump and CLI inspection commands (`--notes`, `--note-done <id>`, `--inspect-pixel <x> <y>`, `--clear-done-notes`).
- [x] Step 1: Fix Note popover text focus by guarding shortcut triggers with `!ctx.wants_keyboard_input()`, wrapping popup in `egui::Window` with stable ID, and preventing outside canvas clicks from discarding composed note text.
- [x] Step 1: Implement Notes Manager (`📋 Notes (open/total) ▾`) toolbar window for viewing note lists, toggling status, 1-click panning to pins, inspecting GPU layer diagnostics, and deleting notes.
- [x] Phase 2.1: Fix multi-layer drying optical collapse: implement 16-bit packed spectral substrate reflectance buffer (`dry_substrate`, 64 MB) and Kubelka 1954 optical strata transfer in WGSL compute shaders.
- [x] Phase 2.1: Eliminate muddy darkening and color shifts on repeated drying, enabling infinite glaze stratification (e.g. Cadmium Red + Hansa Yellow over Phthalo Blue) with bit-for-bit optical fidelity and 1.96 ms p95 120 Hz latency (510+ FPS).
- [x] Phase 2.2: Implement "Fill Jar" from the main canvas as well as the palette, capturing combined wet (`canvas`) and cured dry (`dry_canvas`) pigment recipes into custom jars with 100% reproduction fidelity.
- [x] Phase 2.2: Calibrate diagnostic optical opacity calculation (`top_layer_opacity_pct`) to true ASTM D2805 physical contrast ratio ($R_0 / R_\infty$), accurately differentiating transparent glazes (Quinacridone Magenta) from opaque mass tones (Titanium White, Cadmium Red).
- [x] Phase 2.3: Calibrate multi-coat layer accumulation & mass tone coverage for Phthalo Blue (PB15) and Quinacridone Magenta (PR122). Replaced artificial asymptotic $R_\infty$ saturation ceiling with realistic low-scattering, high-absorption spectral anchors ($S \in [0.10, 0.25]$, $K_{\text{red}} \ge 30.0$). Glazes remain luminous and transparent at low thickness ($X < 0.5$) over white gesso, while multiple dried coats ($X \ge 1.5$) build authentic opaque mass tone (midnight navy [0, 52, 120] and deep ruby-carmine [129, 0, 78]). Verified with automated multi-coat hardware test suite and 7.81 ms p95 120 Hz benchmark.
- [x] Phase 3: 3D Impasto Relief & Studio Lighting. Real-time physical paint thickness and height tracking (wet + dry pigment mass), procedural primed linen canvas weave texture ($H_{\text{weave}}$), central-difference surface normal gradient computation, classical studio raking directional light (315° NW, 45° elevation), and wet-vs-satin Blinn-Phong specular bristle sheen ($F_0=0.04$). Applied strictly to the canvas while keeping the palette flat optical colorimetry. Interactive workbench controls under brush maintenance (Impasto Depth, Canvas Weave, Bristle Sheen, Azimuth, Elevation, Reset to North Light). Verified with 11 Direct3D 12 hardware tests and 1.53 ms p95 120 Hz benchmark (650+ FPS).
- [x] Fix pen release, hover tracking, and event loop backlog unresponsiveness: enforce `POINTER_FLAG_INCONTACT` and `POINTER_FLAG_UP` in Win32 pointer subclassing (`src/pen.rs`), clamp micro-dab spacing to $\ge 0.75\text{ px}$ and reject zero-pressure samples in stroke interpolator (`src/stroke.rs`), terminate zero-pressure strokes in `move_stroke` (`src/main.rs`), post `WM_NULL` on pointer events to wake winit with zero latency, and include active strokes and pending commands in event loop polling so command queues never starve. Verified with 11 DX12 unit tests and 4.99 ms p95 120 Hz benchmark.
- [x] End pen strokes at the last contact sample and let light contact start painting when pressure arrives. Apply pending rack clicks before the next pen stroke so rapid color changes preserve input order.
- [ ] Validate pen tablet drawing and pressure curves at 100% and 150% Windows DPI scaling, including rapid jar-to-canvas taps and lifting while moving the pen.
- [ ] Verify flat brush mark consistency between 0° and 90° across physical tablet strokes.
- [ ] Confirm wipe rag swipe gesture (-10% load) and Clean brush responsiveness on tablet.

## Upcoming feature preparation (ready for implementation)

- [x] Phthalo Blue calibration phase 1. Capture and reproduce the [Studio Essentials PB15 baseline](calibration/phthalo-blue/README.md) with lit and unlit swatches, center sRGB samples, spectral anchors, and a 120 Hz benchmark.
- [x] Phthalo Blue calibration phase 2. Apply studio lighting in linear RGB, report same-film black/white luminance hiding, and check the canvas and white-matched PNG alpha over multiple grounds.
- [x] Limit the built-in workbench rack to Studio Essentials during calibration and retain legacy pigment slots for replay.
- [x] Palette knife tool for crisp scraping, blending, and applying thick opaque patches.
- [x] Interpolate palette knife turns, soften the overlap between steered knife dabs, record desktop Auto-follow for replay, and expose blade angles per sample.
- [x] Sweep the moving palette knife between samples, with distance-based transfer and DX12 checks for paint-height uniformity and wet pigment conservation.
- [ ] Validate palette knife placement, turning, and lift on a physical pen tablet at 100% and 150% Windows DPI scaling.
- [x] Robust oil brush pack (Hog-Bristle Flat, Filbert, Fan, Palette Knife, Soft Mop, Dry-brush skip, and Auto-follow stroke rotation).
- [x] Add split jar dips and swept hog-bristle flat contact with pressure-dependent bundle breakup.
- [ ] Validate flat-brush split loading, pressure, and lift with a physical tablet at 100% and 150% Windows DPI scaling.
- [x] Give Round a finer light-pressure tip and continuous paint transfer through pressure ramps.
- [ ] Validate Round touchdown, broad strokes, and lift with a physical tablet at 100% and 150% Windows DPI scaling.
- [x] Sweep Filbert contact through turns, with soft oval edges and pressure-dependent bristle texture.
- [ ] Validate Filbert rolls and lift with a physical tablet at 100% and 150% Windows DPI scaling.
- [ ] Additional pigments / customizable palette slots (e.g. Raw Umber, Cadmium Red, Viridian).
- [x] Project persistence / session save-and-load (saving canvas, palette state, and swatches to disk).

## Community feedback roadmap (Discord)

### Phase A — Game-Asset Exports
- [x] Transparent PNG export: map paint coverage / ASTM D2805 opacity to alpha channel without white gesso background.
- [x] Height & Normal map export: export Phase 3 Impasto thickness buffer and central-difference surface normals as PNGs.
- [x] Palette export: export custom jar recipes (pigment weights + sRGB preview) to JSON / `.gpl`.

### Phase B — Texture & Underpainting Workflow
- [x] Seamless tiling / wraparound mode: toroidal coordinate wrapping in brush/dab stamping and Phase 3 normal gradient sampling.
- [x] Import image as dried underlayer: uplift sRGB image into `dry_substrate` so wet paint can glaze and layer over sketches or external renders.
- [x] 3-Layer Studio Stack (Reference → Sketch → Paint): non-destructive Reference Layer (`Import Reference…`, `👁 Ref` toggle, opacity), Charcoal Pencil & Eraser on a dedicated Sketch Layer with linen tooth grain, and a `🗂 Layers` panel in the workbench dock.

### Phase C — Stroke Recording, Headless CLI & Deep Undo
- [x] Stroke recording, replay & headless CLI rendering: JSON stroke/command schema for deterministic replay, AI/agent control, and headless PNG export.
- [x] Deeper undo: expand beyond 8 steps via compressed/CPU-offloaded snapshots or checkpoints + JSON stroke replay.

### Phase D — Standalone Pigment Engine Crate
- [ ] Extract core Kubelka-Munk spectral math and WGSL mixing shaders into a standalone workspace crate under `MIT`, keeping the desktop app `GPL-3.0-only`.

### Phase E — Reachable Underlayer, Batch Animation & Agent Docs
- [x] Reachable underlayer import: expose `ImportUnderlayer` in `File → Import Dried Underlayer…`, support `underlayer_path` in `pigment-studio-recording-v1` JSON, and add `--underlayer <img.png>` to the `--render` CLI (keeping `ImportUnderlayer` active in non-test builds) for headless glazing over 3D renders.
- [x] Deterministic batch rendering: add `--batch <manifest.json>` (or multi-frame CLI) reusing one DX12 `PaintGpu` instance across many recordings or per-frame underlayer PNGs with deterministic reset (`Command::Initialize`) between frames.
- [x] Documented recording format: publish a formal JSON Schema for `pigment-studio-recording-v1` plus a reference table of `tube` indices (`0..7`), `dip_rack` cabinet slots (`0..19`), and brush profiles so agents can author strokes from scratch.
