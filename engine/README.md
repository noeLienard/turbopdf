# TurboPDF engine (Rust)

Deterministic PDF engine: **document tree in → PDF, PNG previews and warnings out**. No browser, no network, no JavaScript.

* Specification: [`SPEC.md`](./SPEC.md)
* Types: `crates/turbopdf-tree` (source of truth) → `../src/tree.ts` (generated)

| Crate | Role |
|---|---|
| `turbopdf-tree` | Tree types (serde), validation with JSON Pointer paths, warnings |
| `turbopdf-fonts` | Embedded Inter, user fonts (file / base64), face selection and fallback |
| `turbopdf-text` | Shaping (rustybuzz), line breaking, justification, hyphenation (hypher) |
| `turbopdf-layout` | Flexbox (taffy), tables, pagination, header / footer, page numbers |
| `turbopdf-pdf` | PDF writer (krilla): subsetted fonts, deterministic bytes |
| `turbopdf-raster` | PNG previews (tiny-skia) |
| `turbopdf-engine` | Public API: `Engine::render`, `preview`, `render_with_previews`, `layout` |
| `turbopdf-node` | Node.js binding (napi-rs): `render`, `preview`, `version`; runs on the libuv pool; file access limited to `baseDir` |
| `turbopdf-wasm` | WebAssembly build (plain C ABI) + `wasm/turbopdf.mjs` wrapper for Node and browsers |

```bash
cargo test --workspace                    # all Rust tests
cargo test -p turbopdf-tree --features ts   # also regenerates ../src/tree.ts
npm run gen:tree                          # same, from the SDK root

cargo run --release -p turbopdf-engine --example render -- fixtures/invoice.json out/   # PDF + PNG
cargo run --release -p turbopdf-engine --example bench  -- fixtures/invoice_200.json    # timings
cargo run --release -p turbopdf-engine --example phases -- fixtures/invoice_200.json    # per phase

npm run build:native                      # (repo root) Node module → native/turbopdf.<platform>-<arch>.node
npm run build:wasm                        # (repo root) WebAssembly engine → wasm/turbopdf.wasm
npm run test:wasm                         # (repo root) the whole SDK test suite on the WebAssembly engine
```

The SDK uses the WebAssembly build by itself wherever no native binary exists (`src/rust/wasm.ts` +
`wasm/engine.cjs`, in a worker thread, files read inside `baseDir` like the native module). Standalone
usage, in Node or a browser (no dependency, no filesystem: use `{ "data": base64 }` sources):

```js
import { load } from './wasm/turbopdf.mjs';
const engine = await load(await fetch('/turbopdf_wasm.wasm'));
const { pdf, pages, warnings, previews } = engine.render(tree, { previews: 2 });
```

The `.wasm` is 10.8 MB (Inter ×5, the hyphenation dictionaries and the HTML parser are embedded); 1.3–2× the
native time for a PDF (17 ms instead of 12 ms for the 200-row invoice), ~2.7× for PNGs.

## Performance

`fixtures/invoice_200.json`: 3 pages, 200-row table, repeated header, header / footer with `Page {page} / {pages}`.
Apple Silicon, release build.

| | Rust engine | Chromium (turbopdf ≤ 0.1, measured before its removal) |
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
Checked: native and wasm32 give byte-identical PDFs, PNGs and HTML previews for every fixture
(`test/wasm.test.ts`, run in CI on Linux x86_64 and locally on macOS arm64).
WebAssembly gives the same bytes on every host, so this also ties the native builds together: macOS arm64 =
wasm32 = Linux x86_64. The release workflow runs the same suite on each platform it ships.
