// React components that map 1:1 to the document tree of the Rust engine.
//
//   import { Document, Page, View, Text, Table, Row, Cell, PageNumber } from 'turbopdf/components';
//
// They render custom elements carrying their props as JSON; turbopdf turns that markup into the tree.
// Hooks, context and your own components work as usual: React does the rendering.
import { createElement, type ReactNode } from 'react';
import { imageBytes, imageSource } from './rust/images';
import type { Color, Column, Dimension, Edges, ObjectFit, PageFormat, PdfA, Role, Style, TextStyle, Watermark } from './tree';

export type { Style, TextStyle, Column, Role, Watermark } from './tree';

type Children = { children?: ReactNode };

function el(tag: string, props: object, children?: ReactNode) {
  const clean = Object.fromEntries(Object.entries(props).filter(([, v]) => v !== undefined));
  return createElement(tag, { 'data-p': JSON.stringify(clean) }, children);
}

export interface DocumentProps extends Children {
  title?: string;
  author?: string;
  subject?: string;
  keywords?: string[];
  /** BCP 47 language (`fr`, `en-US`). Also enables hyphenation. */
  lang?: string;
  /** ISO 8601. Omitted by default so the same document gives the same bytes. */
  creationDate?: string;
  /** PDF bookmarks from the headings of levels 1–3. Default true. */
  bookmarks?: boolean;
  /** Tagged PDF (structure for screen readers). Default true. */
  tagged?: boolean;
  /** PDF/UA-1 conformance: needs `title` and `lang`. Default false. */
  pdfua?: boolean;
  /** PDF/A conformance level (`'2b'`, `'3b'`, `'3a'`…): see the `pdfa` option. */
  pdfa?: PdfA;
}

/** Root of the document: metadata, and one or more `<Page>`. */
export function Document({ children, ...meta }: DocumentProps) {
  return el('turbopdf-document', meta, children);
}

export interface PageProps extends Children {
  /** Paper size, or `{ width, height }` in points. Default A4. */
  size?: PageFormat | { width: number; height: number };
  orientation?: 'portrait' | 'landscape';
  /** Points. Default 36 (½ inch). */
  margin?: Edges;
  style?: Style;
  /** Repeated at the top of every page. `{page}` and `{pages}` are replaced in its texts. */
  header?: ReactNode;
  /** Repeated at the bottom of every page. */
  footer?: ReactNode;
  /** Text stamped across every page (`DRAFT`, `PAID`…), or its full settings. */
  watermark?: string | Watermark;
  id?: string;
}

/** A section of pages sharing size, margins, header and footer. Content flows over as many pages as needed. */
export function Page({ children, header, footer, watermark, ...props }: PageProps) {
  const stamp = typeof watermark === 'string' ? { text: watermark } : watermark;
  return el('turbopdf-page', { ...props, watermark: stamp }, [
    createElement('turbopdf-header', { key: 'h' }, header),
    createElement('turbopdf-footer', { key: 'f' }, footer),
    createElement('turbopdf-body', { key: 'b' }, children),
  ]);
}

export interface ViewProps extends Children {
  style?: Style;
  /** Move the whole block to the next page rather than splitting it. */
  keepTogether?: boolean;
  /** Make the whole box clickable: a URL or `#id` of a node of the document. */
  link?: string;
  /** What the box is for screen readers: `list`, `list_item`, `section`, `artifact` (decorative)… */
  role?: Role;
  id?: string;
}

/** A flexbox container (column by default). */
export function View({ children, ...props }: ViewProps) {
  return el('turbopdf-view', props, children);
}

export interface TextProps extends Children {
  style?: Style;
  keepTogether?: boolean;
  /** What the text is for screen readers: `code`, `blockquote`, `caption`, `list_label`, `artifact`… Default: a paragraph. */
  role?: Role;
  id?: string;
}

/** A paragraph. Strings, numbers, `<Span>`, `<b>`, `<i>` and `<br/>` inside. */
export function Text({ children, ...props }: TextProps) {
  return el('turbopdf-text', props, children);
}

/** A styled run inside a `<Text>`. `link`: a URL or `#id` of a node of the document. */
export function Span({ children, style, link }: Children & { style?: TextStyle; link?: string }) {
  return el('turbopdf-span', { style, link }, children);
}

/** A clickable run inside a `<Text>`: a URL (`https:`, `mailto:`…) or `#id` of a node of the document. */
export function Link({ children, href, style }: Children & { href: string; style?: TextStyle & { color?: Color } }) {
  return el('turbopdf-span', { style: { color: '#2563eb', ...style }, link: href }, children);
}

export interface HeadingProps extends Omit<TextProps, 'role'> {
  /** 1–6. Default 1. */
  level?: number;
  /** Keep on the same page as what follows. Default true. */
  keepWithNext?: boolean;
}

export function Heading({ children, level = 1, ...props }: HeadingProps) {
  return el('turbopdf-heading', { level, ...props }, children);
}

export interface ImageProps {
  /** A file path (relative to `baseDir`, default the working directory), a `data:` URL, or the bytes. */
  src: string | Uint8Array;
  fit?: ObjectFit;
  /** What the image shows, read by screen readers. `''` for a decorative image. */
  alt?: string;
  style?: Style;
  /** Make the image clickable: a URL or `#id` of a node of the document. */
  link?: string;
  id?: string;
}

/** A PNG, JPEG, WebP, GIF or SVG image (SVG stays vector in the PDF). Never fetched from the network. */
export function Image({ src, ...props }: ImageProps) {
  const { src: source, format } = typeof src === 'string' ? imageSource(src) : imageBytes(src);
  return el('turbopdf-image', { src: source, format, ...props });
}

export interface QRProps {
  /** Text to encode: a URL, a payment string (EPC/SEPA), a vCard, a ticket number… */
  value: string;
  /** Side in points (1/72 in). Default 90 (about 3 cm). */
  size?: number;
  /** Error correction: `L`, `M` (default), `Q`, `H` (still scans with 30 % damaged or covered). */
  ecc?: 'L' | 'M' | 'Q' | 'H';
  /** Colour of the modules. Default black. Keep it dark on a light background. */
  color?: string;
  /** Colour behind the code. Default white. */
  background?: string;
  /** What the code is for, read by screen readers, e.g. "Payer la facture". `''` for decorative. */
  alt?: string;
  /** Make the code clickable in the PDF too (usually the same URL). */
  link?: string;
  style?: Style;
  id?: string;
}

/**
 * A QR code, drawn by the engine as vector shapes: sharp in the PDF, the preview and the PNGs.
 * Same as `<img src="qr:…">` in HTML and `![alt](qr:…)` in Markdown.
 */
export function QR({ value, size = 90, ecc, color, background, style, ...props }: QRProps) {
  const src = { qr: value, ecc, color, background }; // undefined fields are dropped by JSON
  return el('turbopdf-image', { src, ...props, style: { width: size, height: size, ...style } });
}

export interface TableProps extends Children {
  columns: Column[];
  /** Repeat the header rows on every page. Default true. */
  repeatHeader?: boolean;
  headerStyle?: Style;
  cellStyle?: Style;
  /** Applied to every other body row (zebra). */
  altRowStyle?: Style;
  keepTogether?: boolean;
  style?: Style;
  id?: string;
}

/** A table: `<Row header>` rows first, then `<Row>`s. Rows are never split across pages. */
export function Table({ children, ...props }: TableProps) {
  return el('turbopdf-table', props, children);
}

export function Row({ children, ...props }: Children & { header?: boolean; style?: Style; id?: string }) {
  return el('turbopdf-row', props, children);
}

export function Cell({ children, ...props }: Children & { colSpan?: number; style?: Style }) {
  return el('turbopdf-cell', props, children);
}

/** Force a page break. */
export function PageBreak({ id }: { id?: string } = {}) {
  return el('turbopdf-pagebreak', { id });
}

/**
 * Current page number. `format` defaults to `"{page}"`; `{pages}` is the total.
 * With `target` (the `id` of a node), `{page}` is the page where that node starts: `see page 4`.
 */
export function PageNumber(props: { format?: string; target?: string; style?: Style; id?: string }) {
  return el('turbopdf-pagenumber', props);
}

export interface TableOfContentsProps {
  /** Deepest heading level listed. Default 3. */
  levels?: number;
  style?: Style;
  /** Style of each line (title on the left, page number on the right). */
  entryStyle?: Style;
  id?: string;
}

/** One line per heading of the document with its page number; each line links to its heading. */
export function TableOfContents(props: TableOfContentsProps) {
  return el('turbopdf-toc', props);
}

/** Keep its content on one page. */
export function KeepTogether({ children, style }: Children & { style?: Style }) {
  return View({ children, style, keepTogether: true });
}

/** Empty space. */
export function Spacer({ size = 12 }: { size?: number }) {
  return View({ style: { height: size, flexShrink: 0 } });
}

/** A horizontal line. */
export function Divider({ color = '#e5e7eb', thickness = 0.75, spacing = 8 }: { color?: string; thickness?: number; spacing?: Dimension & number }) {
  return View({ style: { height: thickness, backgroundColor: color, margin: { top: spacing, bottom: spacing }, flexShrink: 0 } });
}
