import type { Document as TreeDocument, Watermark } from './tree';

export type Format = 'A3' | 'A4' | 'A5' | 'Letter' | 'Legal';
export type Orientation = 'portrait' | 'landscape';
/** A CSS length (`'15mm'`, `'1in'`) or a number of CSS pixels. */
export type Length = string | number;
/** One length for every side, or one per side. */
export type Margin = Length | { top?: Length; right?: Length; bottom?: Length; left?: Length };

/** Minimal structural type so TurboPDF never depends on @types/react. */
export interface ReactElementLike {
  type: unknown;
  props: unknown;
}

export interface FontSource {
  /** Family name to use in styles, e.g. 'Brand'. */
  family: string;
  /** Path to a .ttf, .otf, .woff or .woff2 file… */
  path?: string;
  /** …or the font bytes. Exactly one of `path` / `data`. */
  data?: Uint8Array;
  /** CSS font-weight this file provides. Default: 400 */
  weight?: number | string;
  style?: 'normal' | 'italic';
}

interface BaseOptions {
  /** Directory that relative image, font and stylesheet paths are read from (never outside it). Default: the working directory. */
  baseDir?: string;
  /** Page format. Default: 'A4', or the document's CSS `@page { size }`. Ignored when `width` and `height` are set. */
  format?: Format;
  /** Custom page size: a CSS length (`'210mm'`, `'8.5in'`) or a number of CSS pixels. Needs `height`. */
  width?: string | number;
  height?: string | number;
  /** Default: 'portrait' */
  orientation?: Orientation;
  /** CSS length (`'15mm'`, `'1in'`, `'20px'`) or per-side object. Default: '10mm', or the document's `@page { margin }`. */
  margin?: Margin;
  /** Extra CSS for `html` and `react` sources, applied after the document's own stylesheets. */
  css?: string;
  /**
   * Origins (or URL prefixes) that remote images may be downloaded from, e.g. `['https://cdn.example.com']`.
   * TurboPDF downloads them before rendering (the engine never uses the network): https or http only,
   * redirects checked against the list, images only, 20 MB and 10 s per image. Other remote URLs are
   * skipped with an `image_failed` warning. Default: none, nothing is downloaded.
   */
  allowRemote?: string[];
  /** Max duration in ms for the whole render. Default: 30000 */
  timeout?: number;

  /**
   * HTML repeated at the top of every page. `{page}` and `{pages}` are replaced by the page number and the total.
   * Styled by inline styles and by the document's stylesheets.
   */
  header?: string;
  /** Same as `header`, at the bottom. Example: `<div style="text-align:center">Page {page} / {pages}</div>` */
  footer?: string;
  /** Fonts to embed (.ttf, .otf, .woff, .woff2). Inter is built in. HTML can also use `@font-face` with local files. */
  fonts?: FontSource[];
  /** Text stamped across every page (`'DRAFT'`, `'PAID'`), or `{ text, color, fontSize, fontWeight, angle }`. */
  watermark?: string | Watermark;
  /** Document title (PDF metadata, shown by readers). Default: `<title>`, `<Document title>`, or the first `# heading` of Markdown. */
  title?: string;
  /** BCP 47 language (`'fr'`, `'en-US'`): PDF metadata, hyphenation, screen reader voice. Default: `<html lang>` / `<Document lang>`. */
  lang?: string;
  /** PDF bookmarks generated from the h1–h3 headings. Default: true. */
  bookmarks?: boolean;
  /**
   * Tagged PDF: headings, paragraphs, lists, tables, images with their `alt` and the reading order are
   * written for screen readers. Default: true. `false` gives smaller files for documents nobody reads aloud.
   */
  tagged?: boolean;
  /**
   * Conform to PDF/UA-1, the accessibility standard (European Accessibility Act). Needs a title and a
   * language (`<title>`, `<html lang>`, or `<Document title lang>`). Images without `alt` become decorative
   * with a `missing_alt` warning. Throws `not_accessible` when the PDF cannot conform. Default: false.
   */
  pdfua?: boolean;

  /** Throw a `warnings_found` error instead of returning warnings. Default: false. */
  strict?: boolean;
  /** Cancel the render. Rejects with `aborted`. */
  signal?: AbortSignal;
}

/** Exactly one source: `html`, `react`, `markdown` or `tree`. */
export type CreateOptions = BaseOptions &
  (
    | { html: string; react?: never; markdown?: never; tree?: never }
    | { react: ReactElementLike; html?: never; markdown?: never; tree?: never }
    | { markdown: string; html?: never; react?: never; tree?: never }
    /** A document tree (see `turbopdf/tree`). */
    | { tree: TreeDocument; html?: never; react?: never; markdown?: never }
  );

export interface Warning {
  type: 'overflow' | 'moved_to_next_page' | 'missing_glyph' | 'image_failed' | 'stylesheet_failed' | 'font_failed' | 'broken_link' | 'missing_alt' | 'unsupported_css';
  /** English message explaining what happened and what to do. */
  message: string;
  /** The node's `id`, or its JSON Pointer in the document tree. */
  path?: string;
  /** 1-based page number. */
  page?: number;
  /** `stylesheet_failed` / `font_failed` / downloaded `image_failed`: the URL that was not used. */
  url?: string;
}

export interface PngOptions {
  /** Pixel density multiplier. Default: 2 */
  scale?: number;
}

export interface ResponseOptions {
  /** Download name. Default: 'document.pdf' */
  filename?: string;
  /** 'inline' shows it in the browser, 'attachment' downloads it. Default: 'inline' */
  disposition?: 'inline' | 'attachment';
  /** Extra headers merged into the response. */
  headers?: Record<string, string>;
  status?: number;
}

export interface ConfigureOptions {
  /** Max PDFs rendered at the same time; the others wait in line. Default: 4 */
  concurrency?: number;
}

/**
 * The document as HTML/CSS, page by page: real text in the document's own fonts (selectable, searchable),
 * working links (internal ones jump to their target), images, backgrounds and borders. Same layout as the
 * PDF, whatever the source (HTML, Markdown, React or tree).
 */
export interface EdgePreview {
  /**
   * CSS shared by every page: base rules, the fonts used (`@font-face`, embedded) and the images.
   * Put it once in the page that shows the preview, e.g. `<style>{preview.style}</style>`.
   */
  readonly style: string;
  /** One HTML fragment per page (`<section class="turbopdf-page">…`), each needing {@link style}. */
  readonly pages: PreviewPage[];
  /** A complete HTML document (style + every page): for an `<iframe srcdoc>`, a file or a response. */
  toHtml(): string;
  /** A web-standard `Response` holding {@link toHtml}. */
  toResponse(init?: ResponseInit): Response;
}

export interface Preview extends EdgePreview {
  /** Write {@link toHtml} to disk. Missing folders are created. */
  save(path: string): Promise<void>;
}

export interface PreviewOptions {
  /**
   * Shrink each page to fit its container, as a whole: same lines, same page breaks, same proportions as
   * the PDF, never a reflow; and never larger than the real page size. Each page is wrapped in a
   * `<div class="turbopdf-fit">` (full width of its parent). Pure CSS: works inside `dangerouslySetInnerHTML`.
   * Default `false`: pages keep their real size (and may overflow a narrow screen).
   */
  scaleToFit?: boolean;
}

export interface PreviewPage {
  /** 1-based page number */
  page: number;
  /** Page width in points (1/72 in), as in the PDF */
  width: number;
  /** Page height in points */
  height: number;
  /** `<section class="turbopdf-page" …>…</section>` */
  html: string;
}

export interface EdgePngPage {
  /** 1-based page number */
  page: number;
  /** Image width in px */
  width: number;
  /** Image height in px */
  height: number;
  /** The PNG bytes. */
  toUint8Array(): Uint8Array;
  /** `data:image/png;base64,…`, ready for `<img src>`. */
  toDataUrl(): string;
}

export interface PngPage extends EdgePngPage {
  /** Write the PNG to disk. Missing folders are created. */
  save(path: string): Promise<void>;
  /** The PNG bytes, as a Node `Buffer`. */
  toBuffer(): Buffer;
}

/** A PDF made by `turbopdf/edge`: everything but the Node-only file and stream helpers. */
export interface EdgePdf {
  /** Number of pages */
  readonly pages: number;
  /** Problems detected while rendering. Never throws. */
  readonly warnings: Warning[];
  /** The PDF bytes. */
  toUint8Array(): Uint8Array;
  /** The PDF as a `Blob` (`application/pdf`): `URL.createObjectURL(pdf.toBlob())` to download it in a browser. */
  toBlob(): Blob;
  /** A web-standard `Response` with the right headers. Return it from Next.js, Hono, Bun, Deno… */
  toResponse(options?: ResponseOptions): Response;
  /** The document as HTML/CSS (real text, fonts and links), from the same layout as the PDF. */
  preview(options?: PreviewOptions): Promise<EdgePreview>;
  /** One PNG image per page, from the same layout as the PDF: thumbnails, emails, image diffs. */
  toPng(options?: PngOptions): Promise<EdgePngPage[]>;
}

export interface Pdf extends EdgePdf {
  /** Write the PDF to disk. Missing folders are created. */
  save(path: string): Promise<void>;
  /** The PDF bytes, as a Node `Buffer`. */
  toBuffer(): Buffer;
  /** A Node stream of the PDF, for `stream.pipe(res)` in Express. */
  toStream(): import('node:stream').Readable;
  preview(options?: PreviewOptions): Promise<Preview>;
  toPng(options?: PngOptions): Promise<PngPage[]>;
}
