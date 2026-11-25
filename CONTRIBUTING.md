# Contributing to Pigment Studio

Reports, tablet testing, and small patches are welcome. For a Windows input issue, include your tablet model, Windows display scaling, and the sequence of actions that reproduces it. Include GPU and driver details for rendering issues. Review diagnostic notes before sharing them.

Start a port with a small build or smoke-test change. The graphics backend is selected in `src/main.rs`, Win32 pen input lives in `src/pen.rs`, and the paint simulation runs in `src/paint.wgsl`. `src/audio.rs` is another platform boundary. Preserve pigment mass conservation, spectral behavior, and responsive input.

Keep pull requests focused and describe what you verified. See [TESTING.md](TESTING.md) for the DX12 tests, 120 Hz benchmark, and tablet checks.

Submit contributions you have the right to license under [GPL version 3 only](LICENSE). Note the origin and license of third-party material. Accepted changes may be included in free and paid official builds distributed under GPLv3. Keep attribution and license notices for material you reuse.
