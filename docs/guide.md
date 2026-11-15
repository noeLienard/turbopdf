# TurboPDF guide

Everything TurboPDF does, option by option. New here? Start with the [README](../README.md).

```bash
npm install turbopdf
```

## Your first PDF

```ts
import turbopdf from 'turbopdf';

const pdf = await turbopdf.create('<h1>Invoice</h1>');
await pdf.save('invoice.pdf');
```

That's it. Inter is built in, backgrounds are printed, tables are not cut in the middle of a row, table headers
repeat. About 20 ms for a 3-page invoice with 200 rows (a headless browser takes 0.7–0.9 s for the same file).

## Why TurboPDF

TurboPDF is a document engine, not a browser automation wrapper or a low-level drawing API. The comparison below
uses each project's standard workflow; plugins and application-specific integration can extend any of them.

| | **TurboPDF** | [Puppeteer](https://pptr.dev/) | [Playwright](https://playwright.dev/) | [React-pdf](https://react-pdf.org/) | [PDFKit](https://pdfkit.org/) |
|---|---|---|---|---|---|
| Accepted sources | **HTML, React, Markdown and a typed document tree through one API** | HTML or a live web page | HTML or a live web page | React-pdf components | Drawing API |
| HTML to PDF without a browser | **Yes — native Rust engine** | No — launches Chrome | No — PDF export uses Chromium | No HTML input | No HTML input |
| Untrusted document behaviour | **No scripts execute; no network request is made** | Page scripts and requests run unless restricted | Page scripts and requests run unless restricted | No web page; remote assets are application-controlled | No web page; resources are application-controlled |
| Reproducible output | **Byte-identical PDF for the same input** | Not promised | Not promised | Not promised | Application-dependent |
| Long document pagination | **Rows stay whole, table headers repeat, keep-together and page references are built in** | Depends on print CSS and Chromium | Depends on print CSS and Chromium | Page wrapping primitives; document logic is explicit | Table and drawing logic is explicit |
| Accessibility | **Tagged by default; PDF/UA-1 mode validates title, language, alt text and glyph coverage** | Tagged PDF is experimental | Tagged PDF is opt-in | No documented PDF/UA validation | Tagged PDF/PDF-UA are available but structure is authored manually |

## How it works

Every source becomes a **document tree** (pages, views, text, images, tables), laid out by the Rust engine
(flexbox, text shaping, hyphenation, pagination) and written to PDF:

- `html`: parsed, with its CSS applied (see [HTML and CSS](#html-and-css)). Scripts never run.
- `react`: rendered to static markup, then handled like HTML. Use the components below for full control.
- `markdown`: converted directly, styled for print.
- `tree`: the document tree itself (types in `turbopdf/tree`).

Nothing is fetched over the network. Images, fonts and stylesheets are read from `baseDir` (default: the working
directory) and never outside it. The same input gives byte-identical PDFs on macOS, Linux and in WebAssembly.

### Components (`turbopdf/components`)

```tsx
import { Document, Page, View, Text, Span, Link, Heading, Image, QR, Table, Row, Cell, PageNumber, PageBreak, TableOfContents, KeepTogether, Spacer, Divider } from 'turbopdf/components';

const Invoice = () => (
  <Document title="Invoice 42" lang="fr">
    <Page margin={36} footer={<PageNumber format="Page {page} / {pages}" style={{ textAlign: 'center' }} />}>
      <Heading>Invoice 42</Heading>
      <Text>Customer: <Span style={{ fontWeight: 700 }}>Acme</Span></Text>
      <Image src="logo.png" style={{ width: 80 }} />
      <Table columns={[{ width: 'auto' }, { width: 80, align: 'right' }]} cellStyle={{ padding: 4 }} altRowStyle={{ backgroundColor: '#f3f4f6' }}>
        <Row header><Cell><Text>Item</Text></Cell><Cell><Text>Price</Text></Cell></Row>
        {items.map((i) => <Row key={i.id}><Cell><Text>{i.name}</Text></Cell><Cell><Text>{i.price}</Text></Cell></Row>)}
      </Table>
    </Page>
  </Document>
);

await turbopdf.create(<Invoice />);
```

Sizes are in points (1/72 in). Styles include Flexbox and Grid (`flexDirection`, `gridTemplateColumns`, `gap`,
`padding`, `border`, `backgroundColor`, `fontSize`, …). Rows are never split, table headers repeat, `keepTogether` moves a block to
the next page instead of cutting it. Image and font paths are read from `baseDir` (default: the working
directory) and never outside it.

Links, table of contents and stamps:

```tsx
<Page watermark="DRAFT">
  <TableOfContents levels={2} />        {/* one line per heading, with its page, each linked */}
  <Text>Prices are <Link href="#prices">further on</Link>, details on <Link href="https://acme.com">our site</Link>.</Text>
  <PageNumber target="prices" format="see page {page}" />
  <Heading id="prices">Prices</Heading>
  <Image src="logo.svg" link="https://acme.com" />   {/* SVG stays vector; PNG, JPEG, WebP, GIF too */}
</Page>
```

`<Span link>`, `<View link>` and `<Image link>` take a URL (`https:`, `mailto:`, `tel:`…) or `#id`. A link to an
id that does not exist gives a `broken_link` warning; other schemes (`javascript:`…) are never clickable.
Headings h1–h3 also become PDF bookmarks (`<Document bookmarks={false}>` to turn them off).

You can also pass the document tree itself: `turbopdf.create({ tree })` (types in `turbopdf/tree`).

### QR codes

Drawn by the engine as vector shapes: sharp in the PDF, the preview and the PNGs, no dependency, no network.
Same feature in every source:

| Source | Write |
|---|---|
| HTML | `<img src="qr:https://acme.com/pay/42" alt="Pay online" width="90" data-ecc="Q" style="color:#1e3a8a">` |
| Markdown | `![Pay online](qr:https://acme.com/pay/42)` |
| React | `<QR value="https://acme.com/pay/42" size={90} color="#1e3a8a" alt="Pay online" link="https://acme.com/pay/42" />` |
| Tree | `{ "type": "image", "src": { "qr": "https://…", "ecc": "Q", "color": "#1e3a8a", "background": "#ffffff" } }` |

The text can be anything up to 2953 bytes: a URL, a SEPA payment (EPC "GiroCode": the banking app fills in the
transfer), a vCard, a ticket number. `ecc` is the error correction: `L`, `M` (default), `Q`, `H` (scans with up
to 30 % covered, e.g. by a logo). In HTML, `color` and `background-color` colour the code. Keep it dark on a light
background and at least about 2 cm wide (`width="60"` CSS px or more): scanners need contrast and size.

## Create

One source, exactly one of `html`, `react`, `markdown`, `tree`:

```ts
// HTML (a string is always HTML)
await turbopdf.create('<h1>Invoice</h1>');

// HTML with local images and stylesheets: relative paths are read from baseDir
await turbopdf.create({ html: await readFile('invoice.html', 'utf8'), baseDir: './templates' });

// React: pass an element, nothing else to configure
await turbopdf.create(<Invoice customer="Acme" />);

// Markdown (GitHub flavored), styled for print by default
await turbopdf.create({
  markdown: '# Report\n\nThis month **revenue** is up.\n\n| Month | Revenue |\n|---|---|\n| October | 12,000 € |',
});

// Every source accepts the same options
await turbopdf.create({ html, format: 'A4', margin: '15mm', css: 'body { font-size: 10pt }' });
```

To render a web page, fetch its HTML yourself and pass `{ html, baseDir }`: TurboPDF never makes requests.

### Options

| Option | Default | Description |
|---|---|---|
| `html` · `react` · `markdown` · `tree` | – | **The source. Exactly one.** |
| `baseDir` | working directory | Where relative image, font and stylesheet paths are read from (never outside it) |
| `format` | `'A4'` | `'A3'` `'A4'` `'A5'` `'Letter'` `'Legal'` |
| `orientation` | `'portrait'` | or `'landscape'` |
| `width`, `height` | – | Custom page size (`'100mm'`, `'8in'`, or px). Replaces `format`. |
| `margin` | `'10mm'` | A CSS length, or `{ top, right, bottom, left }` |
| `css` | – | Extra CSS for `html` and `react`, applied after the document's own |
| `header`, `footer` | – | HTML repeated on every page. `{page}` and `{pages}` are replaced. See [below](#headers-and-footers). |
| `fonts` | – | `[{ family, path \| data, weight?, style? }]`: `.ttf` / `.otf` fonts to embed |
| `watermark` | – | Text stamped diagonally on every page (`'PAID'`), or `{ text, color, fontSize, fontWeight, angle }` |
| `bookmarks` | `true` | PDF bookmarks from the h1–h3 headings |
| `title`, `lang` | from the source | Document title and language (`<title>`, `<html lang>`, `<Document>`; Markdown: the first `# heading`) |
| `tagged` | `true` | Tagged PDF for screen readers. See [Accessibility](#accessibility). |
| `pdfua` | `false` | Conform to PDF/UA-1. See [Accessibility](#accessibility). |
| `timeout` | `30000` | Max duration in ms |
| `strict` | `false` | Throw on warnings instead of returning them |
| `signal` | – | An `AbortSignal` to cancel |

Without `format` / `margin`, a CSS `@page { size: A5 landscape; margin: 1cm }` rule in the document is used.
Options of the browser-based versions (`url`, `engine`, `printBackground`, `media`, `javascript`, `headers`,
`cookies`, `allowHosts`, `allowFileUrls`, `blockPrivateNetwork`, `deterministic`) are refused with an error that
says what to do instead.

### HTML and CSS

HTML is converted to the document tree, with CSS from `<style>` elements, local `<link rel="stylesheet">` files,
the `css` option and `style` attributes, cascaded by specificity (`!important` included).

- **Selectors**: type, `*`, `.class`, `#id`, `[attr]`, `[attr=value]`, descendant, `>`, `:first-child`,
  `:last-child`, `:nth-child()`, `:root`, `::before` and `::after` (`content`, including `attr()`). CSS custom
  properties inherit and support nested `var()` references and fallbacks. `@media print` / `all` rules apply,
  `screen` rules do not.
- **Properties**: `display` (`block`, `flex`, `grid`, `none`), Flexbox, CSS Grid auto-placement with fixed,
  percentage and `fr` tracks (`repeat()` included), `width` / `height` and their
  min/max, `margin`, `padding`, `border`, `border-radius`, `background(-color)`, `color`, `font-family`,
  `font-size`, `font-weight`, `font-style`, `line-height`, `text-align`, `break-before` / `break-after` /
  `break-inside` (and the `page-break-*` aliases), `position: absolute` (and `fixed`) with `top` / `right` /
  `bottom` / `left` / `inset`, placed in the parent box. Lengths accept `em`, `rem`, `vw`, `vh` and absolute
  units, plus `calc()`, `min()`, `max()` and `clamp()` when they resolve to a physical length.
- **Elements**: `div` and the other block elements, `p`, `h1`–`h6`, `span` / `b` / `strong` / `i` / `em`, `br`,
  `hr`, `a href` (a URL or `#id`, around text or around a whole block), `img` (PNG, JPEG, WebP, GIF, SVG),
  inline `<svg>` (drawn as vectors), `table` (with `thead`, repeated on every page), `ul` / `ol`, `pre`,
  `blockquote`. `<title>` and `<html lang>` fill the PDF metadata.
- **Fonts**: `@font-face { font-family: Brand; src: url(fonts/brand.ttf) }` with local `.ttf` / `.otf` files,
  read inside `baseDir` (relative to the stylesheet that declares them).

Not supported: JavaScript, `float`, web fonts downloaded from a URL and WOFF / WOFF2 files (a
`font_failed` warning says what to do), remote images and stylesheets (each one gives a warning).

### Markdown

Markdown is converted with GitHub syntax (tables with alignment, task lists, ~~strikethrough~~, autolinks), code blocks, and styles designed for print: readable typography, clear headings, light table borders, repeated table headers, quotes, images fitted to the page. No CSS needed.

````md
# Monthly report

Here is the **summary**. Revenue is up, details below.

| Month   | Revenue  |
|---------|---------:|
| October | 12 000 € |

- [x] Close the books
- [ ] Review the budget

```ts
export const growth = (a: number, b: number) => (b - a) / a;
```

<!-- pagebreak -->

## Next steps
````

`<!-- pagebreak -->` starts a new page, `<!-- toc -->` inserts a table of contents. Links are clickable, and headings get GitHub-style ids, so `[see prices](#prices)` jumps to `## Prices`. Raw HTML is allowed (with inline styles). For full control over styles, use `html`.

A string passed straight to `create()` is **always HTML**. Markdown always goes through the `markdown` option, so there is never any ambiguity.

### Headers and footers

```ts
await turbopdf.create({
  html,
  footer: '<div style="text-align:center">Page {page} / {pages}</div>',
});
```

Header and footer are HTML, styled inline or by the document's stylesheets. They take their own room at the top and bottom of every page, inside the margins.

## Accessibility

Every PDF is **tagged**: screen readers get the headings, paragraphs, lists, tables (with header cells),
links, images with their `alt`, and the reading order. Nothing to learn, it comes from what you already write:

| You write | Screen readers get |
|---|---|
| `h1`–`h6`, `# Markdown` | headings (also the PDF bookmarks) |
| `p`, `blockquote`, `pre` / code blocks | paragraphs, quotes, code |
| `ul` / `ol` / `li`, Markdown lists | lists with their items |
| `table`, `th` / `thead`, Markdown tables | tables with header cells (a header repeated on every page is read once) |
| `img alt="…"` | a figure with its description |
| `img alt=""`, `aria-hidden="true"`, `role="presentation"`, `hr` | decorative: skipped |
| `a href` | links |
| `section`, `article` | sections |
| headers, footers, watermark, backgrounds | not read (artifacts) |

With components, `<View role="list">`, `role="list_item"`, `role="artifact"`… do the same; see the `Role` type.

**PDF/UA** (the accessibility standard behind the European Accessibility Act):

```ts
await turbopdf.create({ html, pdfua: true });   // needs <title> and <html lang>, or the title / lang options
```

It also requires every image to have an `alt` (otherwise the image is made decorative, with a `missing_alt`
warning) and every character to be in a font: if not, `create()` throws `not_accessible` with the list of
problems in `error.details`. `tagged: false` turns tagging off (a 200-row invoice: 61 KB instead of 190 KB).

## Download

```ts
await pdf.save('invoice.pdf');   // writes the file, creates missing folders
const buffer = pdf.toBuffer();   // a Buffer
const stream = pdf.toStream();   // a Node stream
const res = pdf.toResponse({ filename: 'invoice.pdf', disposition: 'attachment' });  // a web Response
```

**Next.js** (route handler), **Hono**, **Bun**, **Deno**: return a web `Response`.

```ts
// app/api/invoice/route.ts
export async function GET() {
  const pdf = await turbopdf.create(<Invoice />);
  return pdf.toResponse({ filename: 'invoice.pdf', disposition: 'attachment' });
}
```

**Express**:

```ts
app.get('/invoice.pdf', async (req, res) => {
  const pdf = await turbopdf.create(<Invoice id={req.params.id} />);
  res.type('pdf').set('Content-Disposition', 'attachment; filename="invoice.pdf"');
  pdf.toStream().pipe(res);
});
```

## Preview

`pdf.preview()` gives the document as **HTML/CSS**, page by page: real text in the document's own fonts
(selectable, copyable, searchable with Ctrl+F), working links (internal ones jump to their target),
images, backgrounds and borders. It is drawn from the same layout as the PDF, so it looks the same
whatever you wrote it in: HTML, Markdown, React or a tree.

```tsx
const preview = await pdf.preview();

// in a React page: the style once, then one <section> per page
<style dangerouslySetInnerHTML={{ __html: preview.style }} />
{preview.pages.map((p) => <div key={p.page} dangerouslySetInnerHTML={{ __html: p.html }} />)}

// or a complete HTML document
preview.toHtml();                        // string, e.g. for <iframe srcDoc>
return preview.toResponse();             // from a route
await preview.save('preview.html');
```

| Field | What it is |
|---|---|
| `style` | CSS shared by all pages: base rules, the fonts used (embedded `@font-face`) and the images |
| `pages[i].html` | `<section class="turbopdf-page">…</section>`, the size of the page in `pt` |
| `pages[i].width`, `.height` | Page size in points, as in the PDF |

Everything is scoped under `.turbopdf-page`, so it does not leak into your page (and your CSS does not
reach it). Link targets get ids prefixed with `turbopdf-`. To show pages smaller, use CSS `zoom` on a wrapper.
Fonts are embedded as light WOFF files: the whole font is kept (kerning, ligatures, so the browser places text
exactly like the engine) but only the glyphs the document uses keep their outlines. A typical page is
200–300 KB in all, about 60 KB per font face.

### Images: `pdf.toPng()`

One PNG per page, for thumbnails, emails or image diffs in tests:

```ts
const pages = await pdf.toPng();              // scale: 2 by default (sharp)
await pages[0].save('page-1.png');
const src = pages[0].toDataUrl();             // <img src={src} />
```

### Live preview while you write

```bash
npx turbopdf dev ./invoice.tsx    # a file whose default export is a React component
npx turbopdf dev ./report.md
npx turbopdf dev ./page.html
```

Opens a local page showing the real PDF, page by page. It refreshes every time you save the file, lists the warnings, and has a download button. In a `.tsx` file you can also `export const options = { format: 'A5', margin: '12mm' }`.

## Warnings

Problems are returned, never thrown: you always get a PDF.

```ts
const pdf = await turbopdf.create({ html });
for (const w of pdf.warnings) console.warn(w.type, w.message);
```

| `type` | Meaning |
|---|---|
| `overflow` | An element is wider or taller than its box |
| `moved_to_next_page` | A block that must not be split was moved to the next page |
| `missing_glyph` | A character is not in any loaded font |
| `image_failed` | An image could not be loaded (missing file, corrupt data, remote URL) |
| `stylesheet_failed` | A `<link>` stylesheet was not applied (remote, missing, or outside `baseDir`) |
| `font_failed` | An `@font-face` font was not loaded (remote, WOFF, missing, or outside `baseDir`) |
| `broken_link` | A link to `#id` points to no element |
| `missing_alt` | `pdfua`: an image has no `alt`, it was marked decorative |

Warnings carry `path` (the element's `id`, or its position in the document tree) and `page` when known.
Every `message` is in English and says what to do. In CI, pass `strict: true` to fail on any warning (`TurboPdfError` with `code: 'warnings_found'` and the list in `error.warnings`).

### Page break classes

| Class | Effect |
|---|---|
| `.turbopdf-break-before` | Starts a new page before the element |
| `.turbopdf-break-after` | Starts a new page after the element |
| `.turbopdf-keep-together` | Never splits the element across two pages |

## Production

```ts
turbopdf.configure({ concurrency: 4 });
await turbopdf.warmup(); // optional: load the engine before the first request
```

- Renders run on Node's thread pool, never on the main thread. `concurrency` (default 4) caps how many run at once; the others wait in line.
- No process to manage: `close()` is kept for compatibility and does nothing.
- The engine is a native module (`turbopdf.<platform>-<arch>.node`), also available as WebAssembly (see `engine/README.md`).

## Security

TurboPDF is safe for HTML from users by design: scripts never run, nothing is fetched over the network, and files
are only read inside `baseDir`. Set `timeout` to bound the work a large document can cause.

## Errors

TurboPDF throws a `TurboPdfError` whose `message` says what to do.

| `code` | When |
|---|---|
| `invalid_source` | No source, several sources, or an invalid option |
| `render_timeout` | The render took longer than `timeout` |
| `render_failed` | The document could not be rendered (e.g. your React component threw) |
| `invalid_tree` | The document tree is invalid (`details` lists every problem with its path) |
| `font_failed` | A font file could not be loaded or parsed |
| `not_accessible` | `pdfua` is on and the document cannot conform (`details` lists the problems) |
| `engine_unavailable` | No native engine for this platform |
| `react_not_installed` | A React element was passed but `react` / `react-dom` are not installed |
| `warnings_found` | `strict` is on and the render produced warnings |
| `aborted` | The `signal` was aborted |

```ts
import { TurboPdfError } from 'turbopdf';

try {
  await turbopdf.create({ html, timeout: 5_000 });
} catch (e) {
  if (e instanceof TurboPdfError && e.code === 'render_timeout') { /* … */ }
}
```

## Requirements

Node.js 20 or later. React is optional: install `react` and `react-dom` only if you pass React elements.

## License

MIT
