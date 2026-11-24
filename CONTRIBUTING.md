# Contributing to OpenDesktopDoors

Thank you for contributing to **OpenDesktopDoors**!

## Development Setup

1. Install **Rust** (`x86_64-pc-windows-msvc`) via `rustup`.
2. Install **Visual Studio 2022 Build Tools** with the **Desktop development with C++** workload and **Windows 11 SDK**.
3. Clone and verify:
   ```powershell
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

## Architecture Principles

- **Zero heavy dependencies**: Keep the binary small, fast, and free of WebView/Electron/browser runtimes.
- **0% idle CPU**: Never poll with busy timers when portals are idle or collapsed.
- **Safe Win32 wrappers**: Isolate `unsafe` Win32/COM calls in focused modules with clear invariants and RAII cleanup.
