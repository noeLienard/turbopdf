import type { Document as TreeDocument, Watermark } from './tree';

export type Format = 'A3' | 'A4' | 'A5' | 'Letter' | 'Legal';
export type Orientation = 'portrait' | 'landscape';
export type Margin = string | { top?: string; right?: string; bottom?: string; left?: string };

/** Minimal structural type so TurboPDF never depends on @types/react. */
export interface ReactElementLike {
  type: unknown;
  props: unknown;
}

export interface FontSource {
  /** Family name to use in styles, e.g. 'Brand'. */
  family: string;
  /** Path to a .ttf / .otf file… */
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
  /** Max duration in ms for the whole render. Default: 30000 */
  timeout?: number;

  /**
   * HTML repeated at the top of every page. `{page}` and `{pages}` are replaced by the page number and the total.
   * Styled by inline styles and by the document's stylesheets.
   */
  header?: string;
  /** Same as `header`, at the bottom. Example: `<div style="text-align:center">Page {page} / {pages}</div>` */
  footer?: string;
  /** Fonts to embed (.ttf / .otf). Inter is built in. HTML can also use `@font-face` with local files. */
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
  type: 'overflow' | 'moved_to_next_page' | 'missing_glyph' | 'image_failed' | 'stylesheet_failed' | 'font_failed' | 'broken_link' | 'missing_alt';
  /** English message explaining what happened and what to do. */
  message: string;
  /** The node's `id`, or its JSON Pointer in the document tree. */
  path?: string;
  /** 1-based page number. */
  page?: number;
  /** `stylesheet_failed` / `font_failed`: the URL that was not used. */
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
export interface Preview {
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
  /** Write {@link toHtml} to disk. Missing folders are created. */
  save(path: string): Promise<void>;
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

export interface PngPage {
  /** 1-based page number */
  page: number;
  /** Image width in px */
  width: number;
  /** Image height in px */
  height: number;
  /** Write the PNG to disk. Missing folders are created. */
  save(path: string): Promise<void>;
  /** The PNG bytes. */
  toBuffer(): Buffer;
  /** `data:image/png;base64,…`, ready for `<img src>`. */
  toDataUrl(): string;
}

export interface Pdf {
  /** Number of pages */
  readonly pages: number;
  /** Problems detected while rendering. Never throws. */
  readonly warnings: Warning[];
  /** Write the PDF to disk. Missing folders are created. */
  save(path: string): Promise<void>;
  /** The PDF bytes. */
  toBuffer(): Buffer;
  /** A web-standard `Response` with the right headers. Return it from Next.js, Hono, Bun, Deno… */
  toResponse(options?: ResponseOptions): Response;
  /** A Node stream of the PDF, for `stream.pipe(res)` in Express. */
  toStream(): import('node:stream').Readable;
  /** The document as HTML/CSS (real text, fonts and links), from the same layout as the PDF. */
  preview(): Promise<Preview>;
  /** One PNG image per page, from the same layout as the PDF: thumbnails, emails, image diffs. */
  toPng(options?: PngOptions): Promise<PngPage[]>;
}
