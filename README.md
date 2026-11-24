# OpenDesktopDoors

An ultra-lightweight, open-source desktop portal & folder organizer for **Windows 11** built natively in **Rust + Win32 / Direct2D / DirectComposition**.

## Highlights

- **Instant Cold Startup & Tiny Footprint**: Pure native Win32/Direct2D with zero WebView/Electron/runtime overhead.
- **iOS-Inspired Frosted Glass**: Native Windows 11 DWM Acrylic backdrop, `24px` DirectComposition rounded corners, specular highlights, and critically-damped spring animations.
- **Survives `Win+D`**: Desktop-pinned Z-order (`HWND_BOTTOM` + `SetWinEventHook` guard) keeps portals on your desktop even when showing the desktop.
- **Real Folder & Virtual Portals**: Live folder sync via `ReadDirectoryChangesW` or JSON-persisted shortcut portals.
- **Auto-Collapse & Global Hotkey**: Portals smoothly roll up into a compact floating pill when idle, and `Ctrl+Alt+D` toggles bringing all portals to the foreground.
- **Native Drag & Drop**: Drop files onto portals or drag items out directly to Windows Explorer / Desktop.

## Requirements

- Windows 11 (22H2+)
- Rust (`x86_64-pc-windows-msvc`) + Visual Studio 2022 Build Tools (MSVC + Windows 11 SDK)

## Building

```powershell
cargo build --release
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
