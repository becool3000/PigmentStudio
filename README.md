# Pigment Studio

A standalone Windows workbench for learning wet oil painting with a pen tablet or mouse. WGSL compute shaders on Direct3D 12 handle pigment transfer and spectral mixing.

From the prototype directory, run:

```powershell
cargo run --release --locked
```

## Paint

1. Click a paint jar to dip the active brush and add a full load. Click **Dab** beside a jar to put paint on the mixing palette, where you can mix colors and pick up the result.
2. Switch between the round and flat in the brush rack. Each keeps its own size and paint load. Brush tips show the pigment they are carrying. Set the flat’s angle to change the edge presented to your stroke. Pressure changes its spread and paint transfer.
3. Swipe across the rag to wipe off most paint. Tap **Rinse cup** and swipe the rag to blot the brush clean. **Clean brush** does both in one action.
4. Click **Save swatch**, then tap a mixture in the palette. Click a saved swatch to remove it.
5. Open a tube’s info button for its pigment code, uses, reference, and a mix to try. **Get to know your blue** sets up a comparison between Ultramarine and Phthalo.

Scroll over the canvas to zoom at the pointer. Hold the right mouse button and drag to pan. **Fit canvas** resets the view.

Select **Ink Pen** in the brush rack to draw with black ink. Draw lightly for a narrow line, or press hard and slow down or pause to pool ink. The pool can keep running toward the bottom of the canvas after you lift the pen. With a mouse, drag for a line or hold still to make a drip.

**Undo** or **Ctrl+Z** restores up to eight workbench actions. **Export PNG** writes the opaque 1024 × 1024 sRGB canvas. The palette, brushes, swatches, and history live for the session. The paint curves are approximate and all paint remains wet.

## Verify

```powershell
cargo test --locked
cargo run --release --locked -- --benchmark
```

The DX12 test checks spectral output, paint conservation, brush loads, palette placement, swatches, undo, and PNG export. The release benchmark paints a wet underpainting with a preloaded 64 px brush, using 1,200 synthetic inputs over 10 seconds.

On a tablet, test canvas and palette alignment at 100% and 150% Windows scaling. Try a fast pressure ramp, draw off a surface and back while pressed, and verify that one Undo removes one stroke. Compare the flat brush at 0° and 90° along the same path.
