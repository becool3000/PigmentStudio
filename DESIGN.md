# Pigment Studio

*Authentic color mixing. Wet paint mechanics. Tactile digital oil painting.*

Pigment Studio is a standalone Windows desktop workbench for learning wet oil painting with a pen tablet or mouse. Instead of blending RGB values like a standard digital paint program, it simulates real pigment interactions—such as tinting strength, optical scattering, and physical transfer—using Kubelka-Munk theory implemented in WGSL compute shaders on Direct3D 12.

## Technical direction

- Build the desktop app natively in Rust using `wgpu` on Direct3D 12 for compute and rendering, `winit` for windowing, and `egui` for the studio chrome.
- Use Kubelka-Munk spectral reflectance math across 31 wavelength bands (CIE 1931 2° observer, 400–700 nm under D65 illumination). Subtractive mixing produces authentic physical results (e.g. Hansa Yellow + Phthalo Blue yields brilliant green rather than dark digital gray).
- Capture pen tablet input via native Win32 pointer window subclassing (`WM_POINTERDOWN`, `WM_POINTERUPDATE`, `WM_POINTERUP`). This bypasses standard event queue latency, delivering sub-pixel precision, high-frequency sampling, and hardware pressure curves.
- Unify paint memory into a single GPU texture atlas holding the 1024×1024 canvas, the 512×256 mixing palette, brush load registers, and swatch slots.
- Simulate dynamic liquid ink alongside wet oil paint: ink marks pool under pressure and dwell time, dripping with gravity down the canvas until drying.

## Workspace layout

- **Top toolbar:** Quick access to 8-step Undo (`Ctrl+Z`), Fit canvas, Clear canvas, and 1024×1024 sRGB Export PNG.
- **Central canvas viewport:** Smoothly pan (right-click drag) and zoom (scroll wheel anchored at cursor) around the 1024×1024 digital canvas.
- **Right workbench dock (304–452 logical pixels):**
  - **Mixing palette:** A dedicated 512×256 surface for tapping or painting with a loaded brush, mixing colors, and saving mixtures into up to 64 custom jars. Guided drills can still set out dabs in clear spots automatically.
  - **Atelier Rack:** The eight Studio Essentials pigments are available in the workbench. Slots for Natural & Historic and Precious & Mythic are reserved for existing recordings and future calibration. The rack uses two columns when there is room and one in narrow windows. Each tile has a 40×36 jar beside its name. Tapping the tile dips the brush by 25%, and a separate Info button opens pigment details. The rack scrolls below the mixing palette and brush controls.
  - **Brush rack:** Round brush, Flat brush (with angle rotation and 3-lane transfer), and Ink Pen. Visual brush tips render active paint loads and multi-color pigment distribution.
  - **Brush maintenance:** Wipe rag (gesture wipe), Rinse cup (animated flick), Clean brush button, and dynamic stylus pressure meter.
  - **Guided drills:** Step-by-step exercises (e.g. "Get to know your blue") to compare pigment tinting strength and optical properties.

## Color and simulation model

- **Kubelka-Munk optical engine:** Pigments are defined by absorption ($K$) and scattering ($S$) anchor spectra interpolated across the visible spectrum. Total reflectance produces realistic body color and undertones.
- **Mass conservation:** Paint transferred from jars to palette, picked up by brush bristles, and deposited onto the canvas strictly conserves pigment mass.
- **Bristle dynamics:** The flat brush models three distinct bristle lanes, allowing multi-pigment streaking along oriented stroke paths.
- **Fluid ink model:** Dwell time with pressure > 0.56 accumulates pooling ink that spreads and flows downward over sequential advance steps.

## First prototype verification

Verify on Windows that:

- Automated test suite (`cargo test --locked`) passes all DX12 compute shader checks: spectral output, paint mass conservation, brush loading, palette dab collision checking, swatch saving, undo recovery, and PNG export.
- Benchmark (`cargo run --release --locked -- --benchmark`) completes 1,200 synthetic inputs over 10 seconds with a p95 total frame latency under 16.7 ms (60 FPS target).
- Stylus drawing on pen tablets feels direct and low-latency, with responsive pressure spreading and clean palm rejection across both 100% and 150% display DPI scaling.

## Future directions

Let hands-on painting guide subsequent features:
- Impasto paint thickness modeling with surface normal mapping and directional relief lighting.
- Layer drying and glazing mode to paint over set paint without disturbing underlying work.
- Palette knife tool for scraping and spreading flat opaque patches.
- Native `.pigment` session file persistence (preserving pigment mass maps, swatches, and history).
