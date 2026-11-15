# Papier engine (Rust)

Deterministic PDF engine: **document tree in → PDF, PNG previews and warnings out**. No browser, no network, no JavaScript.

* Specification: [`SPEC.md`](./SPEC.md)
* Types: `crates/papier-tree` (source of truth) → `../src/tree.ts` (generated)

| Crate | Role |
|---|---|
| `papier-tree` | Tree types (serde), validation with JSON Pointer paths, warnings |
| `papier-fonts` | Embedded Inter, user fonts (file / base64), face selection and fallback |
| `papier-text` | Shaping (rustybuzz), line breaking, justification, hyphenation (hypher) |
| `papier-layout` | Flexbox (taffy), tables, pagination, header / footer, page numbers |
| `papier-pdf` | PDF writer (krilla): subsetted fonts, deterministic bytes |
| `papier-raster` | PNG previews (tiny-skia) |
| `papier-engine` | Public API: `Engine::render`, `preview`, `render_with_previews`, `layout` |
| `papier-node` | Node.js binding (napi-rs): `render`, `preview`, `version`; runs on the libuv pool; file access limited to `baseDir` |
| `papier-wasm` | WebAssembly build (plain C ABI) + `wasm/papier.mjs` wrapper for Node and browsers |

```bash
cargo test --workspace                    # all Rust tests
cargo test -p papier-tree --features ts   # also regenerates ../src/tree.ts
npm run gen:tree                          # same, from the SDK root

cargo run --release -p papier-engine --example render -- fixtures/invoice.json out/   # PDF + PNG
cargo run --release -p papier-engine --example bench  -- fixtures/invoice_200.json    # timings
cargo run --release -p papier-engine --example phases -- fixtures/invoice_200.json    # per phase

npm run build:native                      # (repo root) Node module → native/papier.<platform>-<arch>.node
cargo build --release -p papier-wasm --target wasm32-unknown-unknown   # → target/wasm32-unknown-unknown/release/papier_wasm.wasm
node wasm/check-determinism.mjs           # native vs wasm: same PDF bytes for every fixture
```

WebAssembly usage (no dependency, no filesystem: use `{ "data": base64 }` sources):

```js
import { load } from './wasm/papier.mjs';
const engine = await load(await fetch('/papier_wasm.wasm'));
const { pdf, pages, warnings, previews } = engine.render(tree, { previews: 2 });
```

The `.wasm` is 7.6 MB (Inter ×5 and the hyphenation dictionaries are embedded); ~24 ms for the 200-row invoice.

## Performance

`fixtures/invoice_200.json`: 3 pages, 200-row table, repeated header, header / footer with `Page {page} / {pages}`.
Apple Silicon, release build.

| | Rust engine | Chromium (papier ≤ 0.1, measured before its removal) |
|---|---|---|
| PDF, warm (median) | **17.5 ms** | 729 ms |
| PDF, cold (new process / browser) | 110 ms (incl. PNG previews) | 911 ms |
| PDF size | 60 KB | 62 KB |

Phases of the Rust render: validate 0.8 ms · fonts 0.2 ms · layout ~10–12 ms · PDF ~6 ms.
PDF + PNG previews at scale 2: ~230 ms (rasterization is not optimized yet).

What made it fast: OpenType shape plans compiled once per face/script, words shaped once
(`ShapeCache`), fonts parsed once per `Engine`, table cells drawn as one PDF path per colour instead of one
graphics block per cell, and deflate through `zlib-rs`.

## Determinism

The same tree gives the same PDF bytes (tested: `same_tree_gives_the_same_bytes`). No clock (the creation
date is written only when `meta.creationDate` is set), no randomness, no system fonts.
PNG previews are pixel-identical on one machine but may differ by antialiasing across CPUs.
Checked: macOS arm64 (native) and wasm32 give byte-identical PDFs for every fixture (`wasm/check-determinism.mjs`).
Linux (x86_64 SIMD paths of the deflate) is not checked yet: run the same fixtures in a Linux container or CI:

```bash
docker run --rm --platform linux/amd64 -v "$PWD":/src:ro -w /src -e CARGO_TARGET_DIR=/tmp/t rust:1.92 \
  sh -c 'cargo build -q --release -p papier-engine --example render && for f in hello invoice invoice_200; do /tmp/t/release/examples/render fixtures/$f.json /tmp/o 1 >/dev/null && md5sum /tmp/o/$f.pdf; done'
```
