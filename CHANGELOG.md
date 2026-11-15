# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com), and the project follows [Semantic Versioning](https://semver.org).

## Unreleased

TurboPDF no longer uses a browser: every source is rendered by a Rust engine.

### Added

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
