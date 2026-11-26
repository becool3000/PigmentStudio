# Phthalo Blue simulation baseline

This captures the current Studio Essentials PB15 model on the default white ground in preparation for calibration against physical PB15:2 oil paint. All measurements here come from rendered simulation output. Dab counts represent relative paint buildup, and recipe percentages are fractions of the simulator's pigment load.

## Reading the sheet

The PNGs show the same 18 swatches. `unlit.png` provides the color baseline and `lit.png` shows the current studio light and surface relief. `measurements.csv` records the center pixel in each image using 8-bit sRGB. `baseline.json` records the swatch coordinates, current absorption and scattering anchors, and the source hashes.

| Row from top | Swatches from left to right |
| --- | --- |
| Wet thickness | 1, 2, 4, 8, and 16 repeated dabs at a single point |
| Dried coats | 1, 2, and 3 separately dried coats, each one dab |
| Titanium White tints | 0%, 1%, 5%, 20%, and 100% Phthalo Blue by recorded recipe fraction |
| Hansa Yellow Light mixes | 0%, 1%, 5%, 20%, and 100% Phthalo Blue by recorded recipe fraction |

All dabs use the same round brush size and pressure. Each mixture gets eight dabs from a fresh brush load. The authored paint events complete the dried coats before placing the wet ramps. The fixture uses rack slot 19 for its temporary mixtures. Replay it in a fresh session when inspecting it in the app.

The unlit center darkens from `#0065B2` at one wet dab to `#002456` at 16. The dried coats move from `#0065B2` to `#002D6C`. At 5% blue, the white mixture is `#B2E4F1` and the yellow mixture is `#569818`.

## Reproduce

Use a release build on Windows with Direct3D 12. From the repository root, in PowerShell, run the following command.

```powershell
.\calibration\phthalo-blue\render.ps1 -OutputDirectory "$env:TEMP\pigment-phthalo-compare"
```

The script uses the same recording for both renders and changes only the lighting flag. Each render starts a fresh process because the current batch path can retain dried underlayers between jobs. Compare the resulting PNGs and center samples with the baseline files in this directory. `benchmark.txt` records the 120 Hz benchmark and the executable used for this baseline.

## Phase 2 validation

The archived images capture the original renderer. Rerendering with the updated build shows the effect of linear-light shading. The `dx12_thin_phthalo_lighting_hiding_and_transparent_export` test compares a thin wet film over black, white, gray, warm ochre, and blue backgrounds. It checks the hiding ratio against the neutral samples and checks the PNG's white-backed match using linear RGB compositing.

Run the specific DX12 test from the repository root with Pigment Studio closed to print the canvas and PNG values for each ground.

```powershell
cargo test --locked dx12_thin_phthalo_lighting_hiding_and_transparent_export -- --nocapture
```

The canvas can transmit different amounts of red, green, and blue through the glaze. A transparent PNG has one alpha channel and is fitted to preserve the white-backed appearance. Highly saturated glazes may therefore export at or near full alpha. Reference and sketch overlays estimate the RGB response from black and white endpoints. For a stack with dried paint, the black endpoint estimates the accumulated pigment as one film. Use a color export over an imported dried underlayer when the final background is known and spectral layering matters.

In the thin-film test over black, the canvas produces approximately `#010102`. Compositing the exported PNG over the same black in linear RGB produces approximately `#00B3C1`. The same PNG reproduces the nearly white cyan swatch over white within the test's quantization tolerance. This difference is worth checking before using a transparent Phthalo glaze as a game asset on a dark surface.

The Phase 2 replay reproduced the archived unlit image byte for byte. All 22 tests passed serially. The 120 Hz benchmark processed 1,200 inputs at 2.13 ms p95 on the RX 5700 XT.
