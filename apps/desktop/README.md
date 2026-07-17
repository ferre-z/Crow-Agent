# Crow Desktop

Tauri 2 desktop shell for the [Crow](https://github.com/ferre-z/Crow-Agent) autonomous coding agent.

## Prerequisites

- [Rust](https://rustup.rs/) (1.88+, pinned via `rust-toolchain.toml`)
- [Node.js](https://nodejs.org/) 18+
- System dependencies for Tauri 2 (see [Tauri docs](https://tauri.app/start/prerequisites/))

## Development

```bash
# Install frontend dependencies
npm install

# Build the crow sidecar binary
bash ../../scripts/build-sidecar.sh

# Start the Tauri dev server (Vite + Tauri window)
npm run tauri dev
```

The dev server opens a native window at 1280×800. The `crow serve` sidecar is bundled
via `externalBin` and started when the Tauri app boots.

## Production build

```bash
# Build the crow sidecar (release)
bash ../../scripts/build-sidecar.sh

# Build the Tauri app bundle (.deb, .AppImage, .dmg, etc.)
npm run tauri build
```

The output appears in `src-tauri/target/release/bundle/`.

## Rebuilding the sidecar

The crow binary must be rebuilt whenever the core crate changes:

```bash
bash ../../scripts/build-sidecar.sh
```

Tauri expects the binary at `src-tauri/binaries/crow-<target-triple>`.

## Project layout

```
apps/desktop/
├── src/                    # React + TypeScript frontend
│   ├── App.tsx
│   ├── main.tsx
│   ├── index.css           # Tailwind + design tokens
│   └── ...
├── src-tauri/              # Tauri Rust shell
│   ├── src/
│   │   ├── lib.rs          # Sidecar manager, JSON-RPC client, commands
│   │   └── main.rs         # Entry point
│   ├── tauri.conf.json
│   ├── Cargo.toml
│   └── binaries/           # Crow sidecar binary (gitignored)
├── public/
│   └── fonts/              # Self-hosted fonts (Clash Display)
├── package.json
├── tailwind.config.ts
└── vite.config.ts
```

## Notes

- The sidecar binary is platform-specific; rebuild after switching OS or architecture.
- CSP is locked to `'self'` for scripts and `'unsafe-inline'` for styles (required by Tauri).
- Icons are generated from `public/icon-source.png` via `npx @tauri-apps/cli icon`.
