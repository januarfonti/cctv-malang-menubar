# CCTV Menubar - macOS Menu Bar App

A lightweight macOS menu bar application for quick-glance CCTV monitoring of Kota Malang cameras. Lives in the menu bar with no dock icon — click the camera icon to view a live stream in a small popup.

## Download

Pre-built `.dmg` files are available on the [Releases](https://github.com/januarfonti/cctv-malang-menubar/releases) page. Download the latest `.dmg`, open it, and drag the app to your Applications folder.

> **Note:** The DMG is built for Apple Silicon (M1/M2/M3/M4). Intel Macs can run it natively via Rosetta 2.

**macOS Gatekeeper:** Since the app is not code-signed, macOS will show "app is damaged" on first open. Run this once to fix it:

```bash
xattr -cr /Applications/CCTV\ Menubar.app
```

## Features

- **Menu bar tray icon** — camera icon, adapts to light/dark mode (template image)
- **Popup window** — 400x380px, frosted glass vibrancy (macOS Popover effect), rounded corners, no title bar
- **Auto-hide** — popup dismisses when clicking away (focus loss)
- **No dock icon** — uses `ActivationPolicy::Accessory`
- **Camera selector** — custom dropdown with search filter and 220px max-height scroll
- **HLS streaming** — uses HLS.js for reliable playback in WKWebView
- **Screenshot capture** — saves current frame as PNG
- **10s video recording** — canvas-based MediaRecorder capture, downloads as MP4/WebM
- **Camera persistence** — last selected camera saved to localStorage
- **Local proxy** — Rust proxy on `localhost:9877` handles CORS and upstream authentication

## Architecture

```
┌──────────────┐     ┌───────────────────┐     ┌────────────────────┐
│  Tray Icon   │────▶│  Popup Window     │────▶│  HLS.js Player     │
│  (click)     │     │  (vibrancy glass) │     │  (canvas capture)  │
└──────────────┘     └───────────────────┘     └────────┬───────────┘
                                                        │
                     ┌───────────────────┐              │
                     │  Rust Proxy       │◀─────────────┘
                     │  localhost:9877   │
                     │  /cameras         │
                     │  /stream/*        │
                     └────────┬──────────┘
                              │
                     ┌────────▼──────────┐
                     │  Upstream CCTV    │
                     │  103.135.14.67    │
                     │  (direct IP)      │
                     └───────────────────┘
```

### Why a Local Proxy?

The upstream CCTV server (`cctv.malangkota.go.id`) requires specific `Host` and `Referer` headers and uses a self-signed SSL certificate. The Rust proxy on `localhost:9877`:

1. Adds required `Host`, `Referer`, and `User-Agent` headers
2. Accepts invalid TLS certificates (`danger_accept_invalid_certs`)
3. Provides CORS headers for the webview
4. Falls back to static camera data if the API is unreachable

### Why HLS.js Instead of Native Video?

Tauri loads the frontend from `tauri://localhost` (a secure context). The native `<video>` element blocks HTTP sources (`http://127.0.0.1:9877`) as mixed content. HLS.js fetches segments via JavaScript `fetch()`, which bypasses this restriction in the webview.

### Why Canvas-Based Recording?

WKWebView doesn't support `HTMLMediaElement.captureStream()` on HLS streams. Recording works by:
1. Drawing video frames onto an offscreen `<canvas>` at 25fps
2. Calling `canvas.captureStream()` to get a MediaStream
3. Using `MediaRecorder` to encode as MP4 (preferred) or WebM

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Runtime | Tauri v2 |
| Backend | Rust (axum, reqwest, tower-http) |
| Frontend | Vanilla HTML/JS/CSS (no build step) |
| Streaming | HLS.js (CDN) |
| Proxy | axum on `localhost:9877` |
| Positioning | `tauri-plugin-positioner` (tray anchor) |
| Effects | macOS Popover vibrancy, 12px corner radius |

## Project Structure

```
├── ui/
│   └── index.html          # Single-file frontend (HTML + CSS + JS)
└── src-tauri/
    ├── tauri.conf.json      # Tauri config (no windows, tray only)
    ├── Cargo.toml           # Rust dependencies
    ├── build.rs             # Tauri build script
    ├── capabilities/
    │   └── default.json     # Permissions (core + positioner)
    ├── icons/
    │   ├── tray-icon.png    # 22x22 menu bar icon (template)
    │   ├── tray-icon@2x.png # 44x44 retina menu bar icon
    │   └── ...              # App bundle icons
    └── src/
        ├── main.rs          # Entry point
        ├── lib.rs           # Tray icon, popup window, vibrancy setup
        ├── proxy.rs         # HTTP proxy (port 9877)
        └── data/
            └── cameras.json # Static fallback camera data
```

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)

```bash
cargo install tauri-cli --version "^2"
```

## Development

```bash
cargo tauri dev
```

## Build

```bash
cargo tauri build
```

### Build Output

```
src-tauri/target/release/bundle/
├── macos/CCTV Menubar.app
└── dmg/CCTV Menubar_1.0.0_aarch64.dmg
```

## Related

This is the standalone menubar companion app. The main web application (Next.js) lives at [januarfonti/cctv-malang](https://github.com/januarfonti/cctv-malang).

## License

MIT
