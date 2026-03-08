# NRFTW Save Editor

A browser-based save editor for [No Rest for the Wicked](https://store.steampowered.com/app/1371980/No_Rest_for_the_Wicked/). Parse, inspect, and modify `.cerimal` save files entirely in the browser — no server, no uploads.

## Features

- **Save file parsing** — Reads the custom `.cerimal` binary format (ZSTD-compressed, schema-driven) via a Rust WASM module
- **Tree viewer** — Browse the full save structure as an expandable tree with field editing
- **Equipment catalog** — Search items, armaments, and other game assets by name or GUID (requires `catalog.db`)
- **Fully client-side** — All processing happens in the browser using WebAssembly

## Quick Start

```bash
npm install
npm run dev
```

Open http://localhost:5173 and drop a `.cerimal` save file to get started. Example saves are included in `examples/`.

> **Note:** The `prebuild`/`predev` scripts automatically rebuild the WASM module from `wasm/`. If you don't have Rust/wasm-pack installed, the pre-built `src/wasm-pkg/` will work as-is — just run `vite` directly instead of `npm run dev`.

## Building WASM (optional)

Only needed if you're modifying the Rust parser in `wasm/`.

```bash
# Install wasm-pack: https://rustwasm.github.io/wasm-pack/installer/
cargo install wasm-pack

# Build (also runs automatically via npm predev/prebuild)
cd wasm && wasm-pack build --target web --out-dir ../src/wasm-pkg
```

## Asset Catalog

The equipment catalog tab requires `public/catalog.db` (~42MB, not included in the repo). To generate it you need the game installed plus Python with UnityPy:

```bash
pip install UnityPy Pillow
python scripts/bundle_catalog.py --bundle /path/to/game/StreamingAssets/aa/StandaloneWindows64/qdb_assets_all_*.bundle
```

This creates `public/catalog.db` with item names, icons, and metadata extracted from the game's asset bundles.

## Deployment

The app requires [COEP/COOP headers](https://web.dev/articles/cross-origin-isolation-guide) for SharedArrayBuffer (used by SQLite WASM):

```
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Opener-Policy: same-origin
```

These are configured for:
- **Dev server** — `vite.config.ts`
- **Netlify / Cloudflare Pages** — `public/_headers`

GitHub Pages does **not** support custom response headers and will not work.

## Project Structure

```
├── src/              # Svelte app + TypeScript
│   ├── lib/          # Components, bridge, catalog
│   └── wasm-pkg/     # Pre-built WASM (committed)
├── wasm/             # Rust crate (cerimal parser)
├── public/           # Static assets (zstd dict, headers)
├── docs/             # Format documentation
├── examples/         # Sample .cerimal save files
├── scripts/          # Utility scripts (catalog builder)
└── e2e/              # Playwright tests
```

## Documentation

- [Cerimal binary format](docs/cerimal_binary.md)
- [Asset catalog](docs/asset_catalog.md)
- [Asset architecture](docs/asset_architecture.md)
- [Quantum database](docs/quantum_database.md)

## License

MIT
