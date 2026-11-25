# Pigment Studio

Pigment Studio is a Windows workbench for experimenting with wet oil paint. The GPU models how pigments absorb and scatter light, how brushes pick up and lay down paint, and how wet brushwork looks over dried layers.

## Download and paint

Get the Windows x64 ZIP from [GitHub Releases](https://github.com/becool3000/PigmentStudio/releases). Extract it and run `pigment-studio-ink.exe`. You need a Windows PC with a Direct3D 12-capable GPU. A mouse works, and Windows Ink provides pressure input for compatible pen tablets.

- Tap a jar tile in the two-column Atelier Rack to load the brush. There are 20 pigments to try.
- Mix paint directly on the palette and use **Fill Jar** to save a mixture for the current session.
- Paint with a round or flat brush, dry the canvas between layers, and export a 1024 × 1024 PNG.
- Use the Note tool to mark a spot and capture what the simulation is doing there. This idea was inspired by [Klad](https://github.com/klad-app/klad) and conversations with its creator, Quantum Whisker.

This is an early build. The spectral models approximate real pigments. Windows tablet behavior across different devices and display scaling settings is still being tested.

## Build from source

Install Rust with the Windows MSVC toolchain and the Visual Studio C++ build tools. From the repository root:

```powershell
cargo run --release --locked
cargo test --locked -- --test-threads=1
cargo run --release --locked -- --benchmark
```

The DX12 tests require a compatible GPU. Close any running copy of Pigment Studio before building. See [TESTING.md](TESTING.md) for the interactive checks.

## Contribute

Windows tablet testing and macOS and Linux ports are welcome. The current build uses DX12 and Win32 pointer input. For platform work, see the [contribution guide](CONTRIBUTING.md) and [design notes](DESIGN.md).

Pigment Studio and Klad explore different parts of digital art. Klad's editable strokes make it possible for an artist and an agent to work on a drawing together. Pigment Studio explores GPU paint simulation. I'm looking forward to sharing ideas and finding ways to help with Klad too.

## License

Pigment Studio is licensed under [GNU GPL version 3 only](LICENSE). Third-party dependencies and bundled fonts keep their own licenses. Official binary releases include third-party notices.
