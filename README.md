<div align="center">

# ⚡ TurboPDF

**HTML, React or Markdown → PDF in milliseconds. No browser. No network. Same bytes every time.**

A native Rust layout engine behind a tiny TypeScript API. Drop Puppeteer and keep your templates.

[![CI](https://github.com/noeLienard/turbopdf/actions/workflows/ci.yml/badge.svg)](https://github.com/noeLienard/turbopdf/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/turbopdf?color=cb3837)](https://www.npmjs.com/package/turbopdf)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust engine](https://img.shields.io/badge/engine-Rust-dea584?logo=rust)
![PDF/UA-1](https://img.shields.io/badge/PDF%2FUA--1-ready-2ea44f)

<img src=".github/assets/facture.png" width="40%" alt="An invoice written in React and CSS, rendered by TurboPDF" />
<img src=".github/assets/devis.png" width="40%" alt="A quote written with TurboPDF components, with a diagonal watermark" />
<br/>
<img src=".github/assets/billet.png" width="80%" alt="A concert ticket with a native QR code, rendered from a document tree" />

<sub>Real output, unretouched: an invoice in React + CSS, a quote built with components, a ticket with a native QR code.</sub>

</div>

---

```bash
npm install turbopdf
```

```ts
import turbopdf from 'turbopdf';

const pdf = await turbopdf.create('<h1>Hello, PDF 👋</h1>');
await pdf.save('hello.pdf');
```

That's the whole setup. You don't download Chrome, build a Docker image or babysit a headless browser.

## Why TurboPDF?

Making a PDF in JavaScript usually means **launching a whole browser** to print one page. That is slow and heavy,
it is painful to deploy on serverless, and every request runs page scripts. TurboPDF replaces the browser with a
purpose-built document engine written in Rust. It does Flexbox, Grid, text shaping, hyphenation and pagination.

- ⚡ **About 40× faster.** A 3-page invoice with 200 rows renders in about **20 ms**. A headless browser needs
  0.7–0.9 s for the same file.
- 🪶 **Nothing else to install.** No Chromium, no system fonts, no Dockerfile. The Inter font is built in.
- 🔒 **Safe for user HTML.** Scripts never run, nothing is fetched over the network, and files are read only
  inside `baseDir`.
- 🎯 **Deterministic.** The same input gives the same bytes on macOS, Linux and WebAssembly, so you can snapshot-test
  your PDFs.
- 📄 **Built for documents.** Table rows are never cut in half and headers repeat on every page. Page numbers,
  `see page 4` references, a table of contents, bookmarks and watermarks are built in.
- ♿ **Accessible by default.** Every PDF is tagged for screen readers. One flag gives you **PDF/UA-1**, the standard
  behind the European Accessibility Act.
- 🧩 **One API, four sources**: HTML/CSS, React, Markdown, or a typed document tree.

## Write it your way

<table>
<tr><th>HTML + CSS</th><th>React</th></tr>
<tr><td>

```ts
await turbopdf.create({
  html: `<h1>Invoice 42</h1>
         <table>…</table>`,
  css: 'h1 { color: #1d4ed8 }',
  format: 'A4',
  margin: '15mm',
});
```

</td><td>

```tsx
const Invoice = ({ id }) => (
  <div style={{ padding: 24 }}>
    <h1>Invoice {id}</h1>
  </div>
);

await turbopdf.create(<Invoice id={42} />);
```

</td></tr>
<tr><th>Markdown</th><th>Components (full control)</th></tr>
<tr><td>

```ts
await turbopdf.create({
  markdown: `# Monthly report

| Month   | Revenue  |
|---------|---------:|
| October | 12 000 € |`,
});
```

</td><td>

```tsx
import { Document, Page, Heading, QR, PageNumber }
  from 'turbopdf/components';

<Document title="Ticket" lang="en">
  <Page footer={<PageNumber format="{page}/{pages}" />}>
    <Heading>Your ticket</Heading>
    <QR value="https://acme.com/t/418" />
  </Page>
</Document>
```

</td></tr>
</table>

## Benchmarks

| | TurboPDF | Headless Chrome |
|---|---:|---:|
| 3-page invoice, 200 table rows | **~20 ms** | 700–900 ms |
| Start-up | **loads one native module** | launches a browser |
| What you ship | **one ~9 MB engine, fonts included** | a full Chromium |

<sub>Measured on Apple Silicon with Node 20. Browser timings count only printing an already-open page; launching the browser
is extra.</sub>

## How it compares

| | **TurboPDF** | Puppeteer / Playwright | React-pdf | PDFKit |
|---|---|---|---|---|
| HTML & CSS input | ✅ **native engine** | ✅ through Chromium | ❌ | ❌ |
| React input | ✅ **your own components** | ✅ through SSR | ✅ its own primitives | ❌ |
| Markdown input | ✅ | ❌ | ❌ | ❌ |
| Runs without a browser | ✅ | ❌ | ✅ | ✅ |
| Safe with untrusted input | ✅ **no scripts, no network** | ⚠️ needs hardening | – | – |
| Byte-identical output | ✅ | ❌ | ❌ | ⚠️ manual |
| Rows never split, headers repeat | ✅ **built in** | ⚠️ print CSS | ⚠️ manual | ⚠️ manual |
| Tagged PDF / PDF/UA | ✅ **default / one flag** | ⚠️ experimental | ❌ | ⚠️ manual |
| HTML preview of the PDF | ✅ | ❌ | ❌ | ❌ |

## Everything included

| | |
|---|---|
| 🎨 **CSS** | Flexbox, Grid (`fr`, `repeat()`), `calc()`, `var()`, `@page`, `@media print`, `::before`/`::after`, `position: absolute`, `@font-face` |
| 🔤 **Text** | Real shaping (kerning, ligatures), hyphenation from `lang`, your own TTF/OTF fonts |
| 🖼️ **Images** | PNG, JPEG, WebP, GIF, and SVG kept as **vectors**, including inline `<svg>` |
| 🔳 **QR codes** | `<img src="qr:…">`, `![](qr:…)` or `<QR>`, drawn as vectors: SEPA/EPC payments, tickets, links |
| 📑 **Long documents** | Headers and footers with `{page}/{pages}`, table of contents, `see page N`, bookmarks, page breaks, keep-together |
| 🔗 **Links** | External links and internal `#id` jumps, clickable in the PDF |
| 💧 **Stamps** | `watermark: 'PAID'` on every page |
| ♿ **Accessibility** | Tagged PDF by default. `pdfua: true` checks the title, language, alt text and that every character has a glyph |
| 👀 **Preview** | `pdf.preview()` returns real HTML/CSS (selectable text, working links, light fonts), and `pdf.toPng()` returns images |
| 🔥 **Live reload** | `npx turbopdf dev invoice.tsx` renders the PDF again on every save |
| ⚠️ **Warnings, not crashes** | Overflow, missing glyph, broken link… each warning says how to fix it. Use `strict: true` in CI |

## Ship it

Return a web `Response` from **Next.js**, **Hono**, **Bun** or **Deno**:

```tsx
// app/api/invoice/route.tsx
import turbopdf from 'turbopdf';

export async function GET() {
  const pdf = await turbopdf.create(<Invoice />);
  return pdf.toResponse({ filename: 'invoice.pdf', disposition: 'attachment' });
}
```

With **Express**, stream it: `pdf.toStream().pipe(res)`. Renders run on Node's thread pool and never block
your event loop.

## Preview as HTML, not images

```tsx
const preview = await pdf.preview();

<style dangerouslySetInnerHTML={{ __html: preview.style }} />
{preview.pages.map((p) => <div key={p.page} dangerouslySetInnerHTML={{ __html: p.html }} />)}
```

Each page is real HTML/CSS, laid out by the same engine as the PDF. You can select text and find it with Ctrl+F,
and links work. Fonts are cut down to the glyphs you use, about 200 KB a page. The preview looks the same whether
you wrote HTML, Markdown or React.

## Documentation

📖 **[The full guide](docs/guide.md)**: every option, supported HTML and CSS, Markdown, components, QR codes,
accessibility, warnings and errors.

🦀 **[The engine](engine/README.md)**: a Rust workspace (tree → layout → PDF / PNG / HTML) that also builds for
WebAssembly.

## Contributing

Issues and PRs are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), then:

```bash
npm install && npm run build:native && npm run check
```

If TurboPDF spared you from shipping a browser, **⭐ star the repo**. It helps other people find it.

## License

[MIT](LICENSE)
