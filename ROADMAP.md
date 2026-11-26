# Pigment Studio roadmap

## Core workbench prototype (Current milestone)

Validate a native Windows oil painting and ink workbench with real-time Kubelka-Munk spectral mixing, 6 artist pigments, a 1024×1024 canvas, a 512×256 mixing palette, and sub-pixel pen tablet input. See [TASKS.md](TASKS.md) for the active checklist.

## Impasto and canvas texture

Model physical paint thickness and volume on the canvas. Add surface normal mapping and directional relief lighting to reveal bristle ridges, knife marks, paint edges, and subtle canvas weave.

## Extended brush and tool mechanics

- Introduce a **palette knife** for scraping back wet paint, laying down clean flat planes, and creating sharp impasto edges.
- Add filbert and fan brush profiles with customizable stiffness and bristle density.
- Refine dry-brush drag effects when brush load is nearly exhausted.

## Layering, drying, and glazing

- Add a **Dry canvas** / **Glaze layer** feature that sets wet paint into a fixed substrate.
- Enable transparent glazes where wet paint glides over dried passages without lifting or muddying underlying colors.
- Model semi-dry tackiness where fresh paint partially adheres to and lifts semi-set paint.

## Color science, educational sandbox & "The Paint Store"

- **Aladdin's Cave of Paint:** Maintain the core design focus of placing artists in an infinite, authentic paint store where they can freely test rare, astronomical, and historic pigments (from natural lapis lazuli and genuine cinnabar vermilion to marine squid ink sepia and 24K gold leaf) without cost or safety limitations.
- **Museum of Curiosities & Curator's Guides:** Expand the dossier library with interactive spectral absorption graphs, historical trade routes, optical mixing challenges, and manufacturer comparisons.
- Support a native `.pigment` session file format preserving full floating-point pigment distribution, swatches, and history.
- High-resolution export (2K/4K tile rendering) with embedded sRGB / Display P3 color profiles.

## Game-dev, Asset Pipeline & Agent Interoperability

- **Transparent, Height & Normal exports:** Export PNGs with coverage-derived alpha for sprites/decals, plus per-pixel paint height and surface normal maps for 3D materials.
- **Seamless tiling mode:** Wraparound stroke stamping and normal sampling for tileable textures.
- **Dried underlayer import:** Import external sketches or renders (e.g., from Klad) onto the dried substrate for wet overpainting and glazing.
- **Palette export:** Export custom jar recipes and swatches for reuse across projects.
- **JSON stroke recording, replay & headless rendering:** Record and replay strokes deterministically for agent automation, deep undo, and high-res re-rendering.
- **Batch animation & overpaint pipeline:** Single-process deterministic batch rendering across frame sequences or per-frame 3D underlayers (`--underlayer`, `--batch`), backed by a documented JSON Schema and pigment/rack index table.
- **Agent Stroke Editing (Tier A, CLAD-style) & CLI Upgrades:**
  - Add stroke `id` / `group` metadata and non-destructive recording edits (`move_stroke`, `edit_stroke`, `delete_stroke`, `reorder_stroke`) with checkpointed GPU replay.
  - Add persistent `--serve` mode (JSON-RPC over stdin/stdout) keeping `PaintGpu` and checkpoints warm in memory for sub-50 ms agent iteration.
  - Add spatial & optical inspection (`strokes_at`, `inspect_region`, `brush_state`).
  - Add ergonomic agent painting events (`dip_rack { slot, clean: true }`, `set_mix { brush, recipe, load }`).
  - Add multi-mode `--render` (`--mode color,height,normal,transparent` with `{mode}` output pattern), `--normal-convention gl|dx`, and CLI palette export (`--export-palette`).
- **Interactive UI Stroke Selection & Editing (Tier B, Deferred):** Hit-testing, transform gizmos, and incremental GPU re-replay in the desktop app, deferred until after Tier A and CLI upgrades.
- **Permissive pigment engine crate:** Extract the core Kubelka-Munk mixing model and shaders into a standalone crate for runtime use in games and tools.

## Immersive 3D Atelier Studio (Long-Term Vision)

- Model a physical north-light painter's loft in 3D.
- Enable camera rotation/pivots: turning to the easel presents the primary canvas; turning to the taboret presents the mixing palette; turning to the mahogany paint cabinet opens the physical drawer slides and reveals tactile jar racks.
