# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com), and the project follows [Semantic Versioning](https://semver.org).

## Unreleased

### Fixed

- The Linux glibc engines need glibc 2.17 instead of 2.35 (built with zig against glibc 2.17): they load on
  Amazon Linux 2023, so on AWS Lambda and Vercel, which ran the 5× slower WebAssembly engine instead. The
  release fails if a Linux engine needs a newer glibc (`scripts/check-glibc.mjs`) and renders a PDF with the
  native engine in `amazonlinux:2023`, `public.ecr.aws/lambda/nodejs:22` and `node:22-bullseye`.
- The WebAssembly engine and its worker are found by file tracers too: Vercel, Netlify and Next.js
  `output: 'standalone'` deploy them with no `outputFileTracingIncludes`. Checked by `npm run check:trace`, and
  by the release with a Next.js standalone app serving a PDF from Amazon Linux 2023.
- A native engine that fails to load is no longer silent: one `[turbopdf]` warning gives the loader's message
  (hide it with `TURBOPDF_NO_FALLBACK_WARNING=1`), and with `TURBOPDF_ENGINE=native` the `engine_unavailable`
  error has it as its `cause`. `TURBOPDF_NATIVE`, when set, is the only native engine tried.

### Added

- `turbopdf.engineInfo()`: `{ kind: 'native' | 'wasm', target, version, fallbackReason? }`.
- `warmup()` runs once: later calls return the same promise, so it can be called at module scope.
- `bench/cold.mjs`: cold start (new process → first PDF) and warm render, native and WebAssembly.

### Changed

- The embedded fonts are parsed once per process instead of once per render: a warm native render goes from
  2.1 ms to 1.6 ms (Apple M-series, `node bench/cold.mjs`). Cold start: 25 ms native, 125 ms WebAssembly,
  unchanged; `wasm-opt -O3` and SIMD were measured and brought nothing, so the build is unchanged.

## 0.2.1 — 2026-11-16

### Fixed

- Vercel, Netlify and Next.js `output: 'standalone'` deployments now include the native engine: each engine
  package is loaded by a literal `require` that file tracers follow (before, they copied none, and the app
  failed with `engine_unavailable` or fell back to WebAssembly). Checked by `npm run check:trace`.

## 0.2.0 — 2026-11-16

### Added

- CSS selectors: `+` and `~` combinators, `:not()`, `:is()`, `:where()`, `:first-of-type`, `:last-of-type`,
  `:only-child`, `:only-of-type`, `:nth-of-type()`, `:nth-last-child()`, `:nth-last-of-type()`, `:empty`, the
  attribute operators `~=` `|=` `^=` `$=` `*=` and the `i` flag.
- `unsupported_css` warning: each CSS property or selector that is ignored, with how often it is used.
- CSS typography: `text-decoration` (underline, line-through, overline), `letter-spacing`, `text-transform`,
  `vertical-align: super / sub`, `font-variant-numeric: tabular-nums`, `white-space` and `text-indent`, in the
  PDF, the PNGs and the HTML preview. The tree gets the matching `textDecoration`, `letterSpacing`,
  `verticalAlign`, `fontVariantNumeric` and `textIndent` fields.
- `allowRemote`: origins or URL prefixes that remote images may be downloaded from. The SDK downloads them
  (redirects checked against the list, images only, 20 MB and 10 s each); the engine still never uses the network.
- Built-in symbols font (a subset of Noto Sans Symbols 2): check boxes, arrows, dingbats and shapes Inter lacks
  (☐ ☑ ✔ ✘ ✉ ➜ ♠ …) are drawn instead of missing, so `pdfua` no longer fails on them. Color emoji are not included.
- WOFF and WOFF2 fonts (`fonts` option and `@font-face`), e.g. the files of `@fontsource/*` packages.

### Changed

- Links, `<u>` / `<ins>` are underlined and `<s>` / `<del>` struck through, as in a browser; `<sup>` / `<sub>`
  are raised / lowered and smaller. `text-decoration: none` removes the line.

## 0.1.0 — 2026-11-16

TurboPDF no longer uses a browser: every source is rendered by a Rust engine.

### Added

- `pdf.preview({ scaleToFit: true })`: each page shrinks as a whole to fit its container (same layout as
  the PDF, never larger than the real size). Pure CSS, works inside `dangerouslySetInnerHTML`.
- `turbopdf/edge`: the engine in WebAssembly with no Node API, for Cloudflare Workers, Deno Deploy, Netlify
  / Vercel Edge and browsers (`init(wasm)` once, then `create()`). Checked in workerd (Cloudflare's runtime).
  The package exports the engine as `turbopdf/turbopdf.wasm`. It replaces `engine/wasm/turbopdf.mjs`, which
  rendered only document trees.
- `pdf.toUint8Array()` and `page.toUint8Array()` next to `toBuffer()`.
- CSS borders per side: `border-top` / `-right` / `-bottom` / `-left`, their `-width` / `-style` / `-color`, and
  `border-width` / `border-style` / `border-color` with one to four values, resolved in declaration order
  (tree: `borderTop`… next to `border`). Drawn in the PDF, the PNGs and the HTML preview.
- `font-family` lists: the first loaded family is used and the next ones draw the characters it lacks, before
  Inter (tree and components accept the same list).
- CSS `float: left / right` and `clear`, laid out as a row (columns, a logo on the right, an image with text
  beside it).
- **Runs everywhere**: prebuilt engines for macOS arm64/x64, Linux x64/arm64 (glibc and musl) and Windows x64,
  installed as optional `@turbopdf/engine-<target>` packages; on any other platform the SDK falls back by itself
  to the same engine compiled to WebAssembly (`wasm/turbopdf.wasm`, run in a worker thread, files read inside
  `baseDir`). Native and WebAssembly give byte-identical PDFs, PNGs and previews. `TURBOPDF_ENGINE=native|wasm`
  forces one. `npm run build:wasm`, `npm run test:wasm`; a release workflow builds, installs and smoke-tests
  each package on its own OS (and on Bun and Deno).
- **Rust engine** (`engine/`): document tree → PDF and PNG previews, no browser, no network, no JavaScript.
  taffy (flexbox), rustybuzz (shaping), hypher (hyphenation), krilla (PDF, subsetted fonts), tiny-skia (PNG).
  A 3-page invoice with a 200-row table renders in ~17.5 ms (Chromium, before: ~730 ms warm). Same input →
  same bytes on macOS, Linux and WebAssembly. Inter is built in.
- HTML → document tree conversion with a CSS cascade: `<style>`, local `<link rel="stylesheet">` (read inside
  `baseDir`), the `css` option and inline styles; type / class / id / attribute / descendant / child selectors,
  `:first-child`, `:last-child`, `:nth-child()`, specificity, `!important`, `@media print`, `@page` size and margin.
  `<title>` and `<html lang>` fill the PDF metadata.
- `tree` source: render a document tree directly (types exported from `turbopdf/tree`).
- `turbopdf/components`: `Document`, `Page`, `View`, `Text`, `Span`, `Heading`, `Image`, `Table`, `Row`, `Cell`,
  `PageBreak`, `PageNumber`, `KeepTogether`, `Spacer`, `Divider`.
- Clickable links: `<a href>` (URL or `#id`, around text or a whole block), `<Link>`, `link` on `Span` / `View` /
  `Image`, Markdown links with GitHub-style heading ids. Only `http(s):`, `mailto:`, `tel:`, `ftp:` are clickable.
- PDF bookmarks from h1–h3 (`bookmarks: false` to disable), `<TableOfContents>` / `<!-- toc -->` with page
  numbers, `<PageNumber target>` ("see page 4").
- Images: SVG (vector in the PDF, inline `<svg>` too), WebP, GIF (first frame).
- `position: absolute` / `fixed` with `top` / `right` / `bottom` / `left` / `inset`.
- `watermark` option and `<Page watermark>`: text stamped diagonally on every page.
- `@font-face` with local `.ttf` / `.otf` files, read inside `baseDir`.
- Accessibility: every PDF is tagged (headings, paragraphs, lists, tables with header cells, links, figures
  with `alt`, reading order; headers, footers, backgrounds and repeated table headers are artifacts), from
  plain HTML / Markdown semantics or a `role` on `View` / `Text`. `tagged: false` to opt out.
- **`pdf.preview()` now returns the document as HTML/CSS** (`{ style, pages: [{ page, width, height, html }],
  toHtml(), toResponse(), save() }`): real text and fonts, working links, same layout as the PDF, for every
  source. **Breaking:** the PNG images moved to `pdf.toPng({ scale })` (same `PngPage` objects as before;
  `PreviewOptions` is now `PngOptions`). New engine crate `turbopdf-html`. Its fonts are light WOFF files
  (only the glyphs used keep their outlines; kerning and ligatures kept): ~1.2 MB → ~250 KB per document.
- **QR codes**, drawn as vector shapes: `<img src="qr:…">` (HTML), `![alt](qr:…)` (Markdown), `<QR value />`
  (React), `{ "src": { "qr": "…", "ecc", "color", "background" } }` (tree). New `ImageSource` type in the tree.
- HTML: in a `display: flex` container every child (also `<span>`, `<b>`…) is its own flex item, with its box
  styles, as in CSS (they used to be merged into one line of text).
- Markdown links to headings with accents (`[x](#résumé)`) work (markdown-it percent-encoded them).
- An absolutely positioned child may cover its parent's padding without an `overflow` warning.
- An absolutely positioned child anchored with `right` / `bottom` inside a `flexGrow` box is placed in that box
  (it used to be pushed out of it and disappear).
- `margin` accepts numbers (CSS pixels): `margin: 0` used to fall back to 10mm.
- CSS cascade: rules are indexed by id, class and tag like in browsers, so an element only tests the selectors
  that can match it. A 9,000-rule stylesheet over 4,000 elements: 9.8 s → 0.12 s.
- `tsx` is no longer installed with turbopdf (it pulled esbuild, ~10 MB). It is an optional peer dependency, only
  needed by `turbopdf dev` for `.tsx` / `.jsx` files.
- `pdf.preview()` and `pdf.toPng()` reuse the layout computed by `create()`: the engine keeps the laid-out
  document (`Engine::prepare` in Rust, a `NativeDocument` in the Node binding) instead of parsing and laying
  the tree out again for every output.
- `fonts: [{ data }]` bytes go to the engine next to the tree (`{ "buffer": n }` sources), without base64.
- `fonts: [{ path }]` is read by the engine, inside `baseDir`, like images: a missing file, or one outside
  `baseDir`, now throws `font_failed` (was `invalid_source`).
- **HTML and CSS are handled by the Rust engine** (new crate `turbopdf-markup`), not by TypeScript: HTML5
  parsing with html5ever (the WHATWG algorithm: unclosed `<p>` / `<li>`, tables without `<tbody>`, misnested
  tags and every named entity behave as in a browser), the CSS cascade with a rule index and an ancestor
  Bloom filter (18,000 rules over 4,000 elements: ~0.1 s), styles parsed once into typed values instead of
  being written back into `style` strings. Linked stylesheets and `@font-face` files are read by the engine's
  loader, the same `baseDir` sandbox as images and fonts. `@font-face` accepts `data:` URLs. Semicolons inside
  `url(…)` no longer split a declaration; with two `font-size` declarations the winning one is used.
- `npm run bench`: reference benchmark through the public API (reports, invoices up to 8,000 rows, Markdown, a
  framework-size stylesheet), Markdown or JSON output.
- `pdfua: true`: PDF/UA-1 conformance (validated while writing). Error code `not_accessible`, warning
  `missing_alt`. `title` and `lang` options; Markdown takes its title from the first `# heading`.
- Warnings `moved_to_next_page`, `missing_glyph`, `stylesheet_failed`, `font_failed`, `broken_link`; warnings
  carry `path` and `page`.
- Error codes `engine_unavailable`, `invalid_tree` (with `details`: every problem and its JSON path), `font_failed`.
- `baseDir` option: where image, font and stylesheet files are read from (never outside it).
- `npm run build:native`: builds the Node module of the engine (napi-rs) into `native/`.

### Removed (breaking)

- Chromium and Playwright: no `npx turbopdf install`, no browser process.
- The `url` source: fetch the page yourself and pass `{ html, baseDir }`.
- Options `printBackground`, `media`, `javascript`, `headers`, `cookies`, `allowHosts`, `allowFileUrls`,
  `blockPrivateNetwork` and `deterministic` (output is always deterministic; backgrounds always printed). Passing
  one throws `invalid_source` with what to do instead.
- Warning `font_fallback`; `selector` on warnings (replaced by `path`). WOFF / WOFF2 fonts (use .ttf / .otf).
- `renderMarkdown()` and syntax highlighting of code blocks.
- `configure({ keepAlive })` is accepted and ignored; `close()` does nothing.

## 0.1.0

First release: **create**, **download**, **preview**.

### Added

- `turbopdf.create()` from HTML, a React element, a URL or Markdown (GitHub flavored, print stylesheet, syntax highlighting, `<!-- pagebreak -->`).
- Options: `format`, `orientation`, `width` / `height`, `margin`, `printBackground`, `media`, `css`, `header` / `footer` (`{page}`, `{pages}`), `fonts`, `headers`, `cookies`, `timeout`, `strict`, `deterministic`, `signal`.
- Output: `save()`, `toBuffer()`, `toStream()`, `toResponse()`, `pages`, `warnings`.
- `pdf.preview()`: one PNG per page rendered from the final PDF, with `save()`, `toBuffer()` and `toDataUrl()`.
- Fidelity by default: waits for fonts, images (lazy ones included) and network; lays out at the real page width; prints backgrounds; avoids cutting rows, images and headings; repeats table headers; `.turbopdf-break-before`, `.turbopdf-break-after`, `.turbopdf-keep-together`.
- Warnings: `font_fallback`, `image_failed`, `overflow`.
- Browser lifecycle: shared Chromium, one isolated context per PDF, `concurrency` limit, `keepAlive`, `warmup()`, `close()`.
- Security options: `javascript`, `allowHosts`, `blockPrivateNetwork`, `allowFileUrls` (`file://` is refused by default).
- `TurboPdfError` with codes `invalid_source`, `render_timeout`, `render_failed`, `react_not_installed`, `warnings_found`, `aborted`.
- CLI: `turbopdf dev <file>` (live preview of `.tsx`, `.html`, `.md`) and `turbopdf install`.
- ESM and CommonJS builds with types.

### Notes

- **Requires Node.js 20+.** The initial goal was Node 18, but Playwright (>= 1.5x) requires Node 20.
- `javascript: false` is enforced with a Content-Security-Policy rather than by disabling JavaScript in the browser, because TurboPDF needs to run code in the page to wait for fonts and images.
