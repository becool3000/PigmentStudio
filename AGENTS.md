# Working on Pigment Studio

- Read [DESIGN.md](DESIGN.md) and [TASKS.md](TASKS.md) before changing the project. Use [DECISIONS.md](DECISIONS.md) for rationale, [CONVENTIONS.md](CONVENTIONS.md) for code organization, and [TESTING.md](TESTING.md) for validation.
- Work on the requested task in small, reviewable changes. Treat [ROADMAP.md](ROADMAP.md) as a list of possibilities. Confirm with the user before changing a settled product or architecture decision.
- Preserve physical and optical fidelity. Spectral Kubelka-Munk mixing, pigment mass conservation, and realistic brush transfer mechanics are central to the workbench. Maintain paint conservation and numerical stability across WGSL compute shaders.
- Protect low-latency input. Keep pen tablet and mouse input handling responsive and stutter-free. Respect the separation between Win32 pointer event processing, WGSL compute dispatch, and egui workspace chrome. Keep heavy processing off the input and render threads.
- Only do one build at a time. If the user has the window open and is testing, never build in the background: Windows locks the running binary (`Access is denied`), failing the link. Either notify the user to close it or terminate the running app process before building.
- Verify what changed with DX12 tests and the 120 Hz benchmark. Report interactive pen tablet and display scaling checks that require hands-on Windows validation. Update task status and decisions when the work changes them.
