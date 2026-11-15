// Static markup (what React's renderToStaticMarkup produces) → document tree nodes.
//
// Two vocabularies are understood:
//  * TurboPDF components (`turbopdf/components`) render `<turbopdf-*>` elements whose `data-p` attribute holds
//    their props as JSON: they map 1:1 to tree nodes.
//  * HTML (div, p, h1–h6, span/b/i, a, img, svg, table, ul/ol, hr, br), its CSS already cascaded into
//    `style` attributes (see ./cascade): mapped to the closest tree nodes.
import type { Cell, Column, Heading, Image, Node, Page, PageBreak, PageNumber, QrEcc, Row, Span, Style, TableOfContents, Text, TextContent, TextStyle, View } from '../tree';
import { declarations } from './cascade';
import { cssLength, parseCss, textStyleOf } from './css';
import { imageSource, svgSource } from './images';

export interface El {
  tag: string;
  attrs: Record<string, string>;
  children: Child[];
  /** `<svg>`: its exact source (attribute case matters: `viewBox`), children are not parsed. */
  raw?: string;
}
export type Child = El | string;

const VOID = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'source', 'track', 'wbr']);
const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0' };

function decode(s: string): string {
  return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e: string) => {
    if (e[0] === '#') {
      const code = e[1] === 'x' || e[1] === 'X' ? Number.parseInt(e.slice(2), 16) : Number.parseInt(e.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : m;
    }
    return ENTITIES[e.toLowerCase()] ?? m;
  });
}

/** Tolerant HTML tokenizer: enough for generated markup, not a full HTML5 parser. */
export function parseMarkup(html: string): Child[] {
  const root: El = { tag: '#root', attrs: {}, children: [] };
  const stack: El[] = [root];
  const re = /<!--[\s\S]*?-->|<!doctype[^>]*>|<\/([a-zA-Z][\w-]*)\s*>|<([a-zA-Z][\w-]*)((?:\s+[^\s=>/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|([^<]+|<)/gi;
  for (let m = re.exec(html); m; m = re.exec(html)) {
    const [, close, open, rawAttrs, selfClose, text] = m;
    const top = stack[stack.length - 1]!;
    if (text !== undefined) {
      top.children.push(decode(text));
    } else if (open) {
      const tag = open.toLowerCase();
      const attrs: Record<string, string> = {};
      for (const a of (rawAttrs ?? '').matchAll(/([^\s=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g)) {
        attrs[a[1]!.toLowerCase()] = decode(a[2] ?? a[3] ?? a[4] ?? '');
      }
      const el: El = { tag, attrs, children: [] };
      top.children.push(el);
      if (tag === 'svg') {
        // keep the SVG verbatim: it is handed to the engine as an image
        let end = m.index + m[0].length;
        if (!selfClose) {
          const inner = /<(\/?)svg\b[^>]*?(\/?)>/gi;
          inner.lastIndex = end;
          let depth = 1;
          for (let t = inner.exec(html); t && depth > 0; t = inner.exec(html)) {
            if (t[1]) depth--;
            else if (!t[2]) depth++;
            end = t.index + t[0].length;
          }
          if (depth > 0) end = html.length;
        }
        el.raw = html.slice(m.index, end);
        re.lastIndex = end;
        continue;
      }
      if (!selfClose && !VOID.has(tag)) stack.push(el);
    } else if (close) {
      const tag = close.toLowerCase();
      const at = stack.map((e) => e.tag).lastIndexOf(tag);
      if (at > 0) stack.length = at;
    }
  }
  return root.children;
}

// ------------------------------------------------------------------ helpers

function props<T extends object>(el: El): T {
  const raw = el.attrs['data-p'];
  return (raw ? JSON.parse(raw) : {}) as T;
}

const INLINE = new Set(['span', 'b', 'strong', 'i', 'em', 'a', 'code', 'small', 'u', 's', 'sub', 'sup', 'label', 'br', 'mark', 'abbr', 'time', 'turbopdf-span']);
/** An `<a>` around blocks or images (a clickable card, a linked logo) is a block itself. */
const isInline = (c: Child): boolean => typeof c === 'string' || (INLINE.has(c.tag) && !(c.tag === 'a' && c.children.some((x) => !isInline(x))));

function clean<T extends object>(o: T): T {
  for (const k of Object.keys(o) as (keyof T)[]) if (o[k] === undefined) delete o[k];
  return o;
}

function sameStyle(a: Span, b: Span): boolean {
  return JSON.stringify(a.style ?? {}) === JSON.stringify(b.style ?? {}) && a.link === b.link;
}

/** `href` of a link: `#id`, a URL, or nothing for anchors without a target. */
function hrefOf(el: El): string | undefined {
  const h = el.attrs.href?.trim();
  return h && h !== '#' ? h : undefined;
}

/** Inline children → spans, with HTML whitespace collapsing. */
function spans(children: Child[], style: TextStyle | undefined, out: Span[] = [], link?: string): Span[] {
  for (const c of children) {
    if (typeof c === 'string') {
      out.push(clean({ text: c.replace(/[ \t\r\n\f]+/g, ' '), style, link }));
      continue;
    }
    if (hidden(c)) continue;
    let s: TextStyle | undefined = style;
    let l = link;
    const add = (extra: TextStyle) => (s = { ...s, ...extra });
    switch (c.tag) {
      case 'br':
        out.push(clean({ text: '\n', style, link }));
        continue;
      case 'b':
      case 'strong':
        add({ fontWeight: 700 });
        break;
      case 'i':
      case 'em':
        add({ fontStyle: 'italic' });
        break;
      case 'a':
        if (hrefOf(c)) {
          l = hrefOf(c);
          add({ color: '#2563eb' });
        }
        break;
      case 'turbopdf-span': {
        const p = props<{ style?: TextStyle; link?: string }>(c);
        add(p.style ?? {});
        if (p.link) l = p.link;
        break;
      }
    }
    const css = textStyleOf(parseCss(c.attrs.style));
    if (css) add(css);
    // block elements inside a text (e.g. <div> in a <p>) start a new line
    const block = !INLINE.has(c.tag) && !c.tag.startsWith('turbopdf-');
    if (block) out.push({ text: '\n', style });
    spans(c.children, s, out, l);
    if (block) out.push({ text: '\n', style });
  }
  return out;
}

/** Spans → `TextContent`: merged, trimmed, a plain string when there is no styling. */
function content(children: Child[]): TextContent {
  const merged: Span[] = [];
  for (const sp of spans(children, undefined)) {
    const last = merged[merged.length - 1];
    if (last && sameStyle(last, sp)) last.text += sp.text;
    else merged.push({ ...sp });
  }
  // trim around hard breaks and at both ends
  for (const sp of merged) sp.text = sp.text.replace(/ *\n */g, '\n');
  if (merged.length) {
    merged[0]!.text = merged[0]!.text.replace(/^[ \n]+/, '');
    const last = merged[merged.length - 1]!;
    last.text = last.text.replace(/[ \n]+$/, '');
  }
  const kept = merged.filter((s) => s.text.length > 0).map((s) => clean(s));
  if (kept.length === 0) return '';
  if (kept.length === 1 && !kept[0]!.style && !kept[0]!.link) return kept[0]!.text;
  return kept;
}

function isBlank(c: TextContent): boolean {
  return typeof c === 'string' ? c.trim() === '' : c.every((s) => s.text.trim() === '');
}

/** Merge `style` under the node's own style (the node's values win). */
function withStyle(node: Node, style: Style): Node {
  const n = node as { style?: Style };
  if (Object.keys(style).length) n.style = { ...style, ...n.style };
  return node;
}

// ------------------------------------------------------------------ blocks

/**
 * Children of a container → block nodes. Runs of inline content become one text node, except in a flex
 * container, where (as in CSS) every child element is its own flex item: `<div style="display:flex">
 * <span>A</span><span>B</span></div>` lays A and B out side by side, not as one line of text.
 */
export function blocks(children: Child[], flex = false): Node[] {
  if (flex) {
    const out: Node[] = [];
    for (const c of children) {
      if (typeof c === 'string') {
        if (c.trim()) out.push({ type: 'text', content: c.trim().replace(/[ \t\r\n\f]+/g, ' ') });
      } else if (isInline(c) && c.tag !== 'br') {
        if (hidden(c)) continue;
        const text = content([c]);
        if (!isBlank(text)) {
          // the element is a flex item: its box (position, background, padding…) applies to the whole text
          const box = parseCss(c.attrs.style);
          const node: Node = Object.keys(box).length ? { type: 'text', style: box, content: text } : { type: 'text', content: text };
          const id = c.attrs.id;
          if (id && !usedIds.has(id)) {
            usedIds.add(id);
            node.id = id;
          }
          out.push(node);
        }
      } else if (c.tag !== 'br') {
        out.push(...block(c));
      }
    }
    return out;
  }
  const out: Node[] = [];
  let inline: Child[] = [];
  const flush = () => {
    if (inline.length) {
      const c = content(inline);
      if (!isBlank(c)) out.push({ type: 'text', content: c });
      inline = [];
    }
  };
  for (const c of children) {
    if (isInline(c)) {
      inline.push(c);
      continue;
    }
    flush();
    out.push(...block(c as El));
  }
  flush();
  return out;
}

/** Last value of a CSS property in the element's (cascaded) style. */
function cssProp(el: El, ...names: string[]): string | undefined {
  let v: string | undefined;
  for (const [k, value] of declarations(el.attrs.style ?? '')) if (names.includes(k)) v = value.toLowerCase();
  return v;
}

function isItemLayout(el: El): boolean {
  return /^(inline-)?(flex|grid)$/.test(cssProp(el, 'display') ?? '');
}

function hidden(el: El): boolean {
  return cssProp(el, 'display') === 'none' || cssProp(el, 'visibility') === 'hidden' || 'hidden' in el.attrs;
}

const BREAK = /^(page|always|left|right|recto|verso)$/;

/** One element → nodes, with CSS pagination: `break-before/after: page`, `break-inside: avoid`, `display: none`. */
/** HTML ids already given to a node: tree ids must be unique, later duplicates are dropped. */
let usedIds = new Set<string>();

function block(el: El): Node[] {
  if (hidden(el)) return [];
  const nodes = blockOf(el);
  // aria-hidden / role="presentation": decorative, skipped by screen readers
  if (el.attrs['aria-hidden'] === 'true' || /^(presentation|none)$/.test(el.attrs.role ?? '')) {
    for (const n of nodes) {
      if (n.type === 'view' || n.type === 'text') n.role = 'artifact';
      else if (n.type === 'image') n.alt = '';
    }
  }
  const id = el.attrs.id;
  if (id && !usedIds.has(id) && nodes.length === 1 && nodes[0]!.type !== 'page_break' && !(nodes[0] as { id?: string }).id) {
    usedIds.add(id);
    (nodes[0] as { id?: string }).id = id;
  }
  if (cssProp(el, 'break-inside', 'page-break-inside') === 'avoid') {
    for (const n of nodes) if (n.type === 'view' || n.type === 'text' || n.type === 'heading' || n.type === 'table') n.keepTogether = true;
  }
  const before = BREAK.test(cssProp(el, 'break-before', 'page-break-before') ?? '');
  const after = BREAK.test(cssProp(el, 'break-after', 'page-break-after') ?? '');
  return [...(before ? [{ type: 'page_break' } as Node] : []), ...nodes, ...(after ? [{ type: 'page_break' } as Node] : [])];
}

function blockOf(el: El): Node[] {
  const style = parseCss(el.attrs.style);
  switch (el.tag) {
    // ---- TurboPDF components
    case 'turbopdf-view':
      return [{ ...props<Omit<View, 'children'>>(el), type: 'view', children: blocks(el.children) }];
    case 'turbopdf-text':
      return [{ ...props<Omit<Text, 'content'>>(el), type: 'text', content: content(el.children) }];
    case 'turbopdf-heading':
      return [{ ...props<Omit<Heading, 'content'>>(el), type: 'heading', content: content(el.children) }];
    case 'turbopdf-image':
      return [{ ...props<Image>(el), type: 'image' }];
    case 'turbopdf-table':
      return [componentTable(el)];
    case 'turbopdf-pagebreak':
      return [{ ...props<PageBreak>(el), type: 'page_break' }];
    case 'turbopdf-pagenumber':
      return [{ ...props<PageNumber>(el), type: 'page_number' }];
    case 'turbopdf-toc':
      return [{ ...props<TableOfContents>(el), type: 'table_of_contents' }];
    case 'turbopdf-document':
    case 'turbopdf-page':
    case 'turbopdf-body':
      return blocks(el.children);
    case 'turbopdf-header':
    case 'turbopdf-footer':
      return [];

    // ---- HTML
    case 'p':
    case 'pre':
    case 'blockquote':
    case 'figcaption': {
      const text = el.tag === 'pre' ? preContent(el) : content(el.children);
      const s: Style = { margin: { bottom: 8 }, ...style };
      if (el.tag === 'pre') Object.assign(s, { backgroundColor: '#f3f4f6', padding: 8, fontSize: 9, ...style });
      if (el.tag === 'blockquote') Object.assign(s, { padding: { left: 10 }, color: '#4b5563', ...style });
      const role = ({ pre: 'code', blockquote: 'blockquote', figcaption: 'caption' } as const)[el.tag as 'pre' | 'blockquote' | 'figcaption'];
      return [withStyle(clean({ type: 'text' as const, role, content: text }), s)];
    }
    case 'h1':
    case 'h2':
    case 'h3':
    case 'h4':
    case 'h5':
    case 'h6':
      return [withStyle({ type: 'heading', level: Number(el.tag[1]), content: content(el.children) }, style)];
    case 'img':
      return image(el, style);
    case 'svg':
      return inlineSvg(el, style);
    case 'a': {
      // a link around blocks: the image itself, or the whole box, is clickable
      const href = hrefOf(el);
      const inner = blocks(el.children);
      if (href && inner.length === 1 && inner[0]!.type === 'image' && !Object.keys(style).length) return [{ ...inner[0]!, link: href }];
      return [withStyle(clean({ type: 'view' as const, link: href, children: inner }), style)];
    }
    case 'table':
      return [htmlTable(el, style)];
    case 'hr':
      return [{ type: 'view', role: 'artifact', style: { height: 0.75, backgroundColor: '#e5e7eb', margin: { top: 8, bottom: 8 }, ...style }, children: [] }];
    case 'ul':
    case 'ol':
      return [list(el, style)];
    case 'script':
    case 'style':
    case 'head':
    case 'title':
    case 'meta':
    case 'link':
    case 'template':
      return [];
    case 'section':
    case 'article':
      return [withStyle({ type: 'view', role: 'section', children: blocks(el.children, isItemLayout(el)) }, style)];
    default:
      // div, header, footer, main, nav, button, li…: a box
      return [withStyle({ type: 'view', children: blocks(el.children, isItemLayout(el)) }, style)];
  }
}

/**
 * Preformatted text: the engine collapses ordinary spaces, so indentation and runs of spaces become
 * non-breaking spaces. Single spaces between words stay normal (copy / paste and search keep working).
 */
export function keepSpaces(text: string): string {
  return text.replace(/^ +/gm, (m) => '\u00a0'.repeat(m.length)).replace(/ {2,}/g, (m) => ` ${'\u00a0'.repeat(m.length - 1)}`);
}

function preContent(el: El): TextContent {
  const raw = (function text(c: Child[]): string {
    return c.map((x) => (typeof x === 'string' ? x : x.tag === 'br' ? '\n' : text(x.children))).join('');
  })(el.children);
  // keep indentation: the engine collapses ordinary spaces
  return keepSpaces(raw.replace(/\n$/, ''));
}

/** `width` / `height` attributes (numbers are CSS pixels), under the CSS of the element. */
function sizeAttrs(el: El, style: Style): Style {
  const len = (v?: string) => (v && !v.endsWith('%') ? cssLength(/^\d+(\.\d+)?$/.test(v) ? `${v}px` : v) : undefined);
  return clean({ width: len(el.attrs.width), height: len(el.attrs.height), ...style });
}

function image(el: El, style: Style): Node[] {
  // remote URLs too: the engine refuses them with an `image_failed` warning (it never uses the network)
  const { src, format } = imageSource(el.attrs.src ?? '');
  if ('qr' in src) {
    // a QR code takes its colours from CSS (`color`, `background-color`) and `data-ecc="L|M|Q|H"`
    const { color, backgroundColor, ...rest } = style;
    const ecc = el.attrs['data-ecc']?.toUpperCase();
    const qr = clean({ ...src, color, background: backgroundColor, ecc: ecc && /^[LMQH]$/.test(ecc) ? (ecc as QrEcc) : undefined });
    return [withStyle(clean({ type: 'image' as const, src: qr, alt: el.attrs.alt }), sizeAttrs(el, rest))];
  }
  return [withStyle(clean({ type: 'image' as const, src, format, alt: el.attrs.alt }), sizeAttrs(el, style))];
}

function inlineSvg(el: El, style: Style): Node[] {
  const { src, format } = svgSource(el.raw ?? '<svg xmlns="http://www.w3.org/2000/svg"/>');
  return [withStyle(clean({ type: 'image' as const, src, format, alt: el.attrs['aria-label'] }), sizeAttrs(el, style))];
}

function list(el: El, style: Style): Node {
  let n = Number(el.attrs.start ?? 1);
  const items = el.children.filter((c): c is El => typeof c !== 'string' && c.tag === 'li');
  return withStyle(
    {
      type: 'view',
      role: el.tag === 'ol' ? 'ordered_list' : 'list',
      children: items.map((li) => ({
        type: 'view',
        role: 'list_item',
        style: { flexDirection: 'row', margin: { bottom: 2 }, ...parseCss(li.attrs.style) },
        children: [
          { type: 'text', role: 'list_label', style: { width: 14, flexShrink: 0 }, content: el.tag === 'ol' ? `${n++}.` : '•' },
          { type: 'view', role: 'list_body', style: { flexGrow: 1, flexShrink: 1 }, children: blocks(li.children) },
        ],
      })),
    },
    { margin: { bottom: 8 }, ...style },
  );
}

// ------------------------------------------------------------------ tables

function cellOf(el: El, header: boolean): Cell {
  const style = parseCss(el.attrs.style);
  // HTML defaults for <th>: bold, centred
  if (header) Object.assign(style, { fontWeight: 700, textAlign: 'center', ...style });
  const span = Number(el.attrs.colspan ?? 1);
  return clean({ children: blocks(el.children), colSpan: span > 1 ? span : undefined, style: Object.keys(style).length ? style : undefined });
}

function htmlTable(el: El, style: Style): Node {
  const header: Row[] = [];
  const rows: Row[] = [];
  const rowOf = (tr: El, isHeader: boolean): Row => {
    const s = parseCss(tr.attrs.style);
    const cells = tr.children.filter((c): c is El => typeof c !== 'string' && (c.tag === 'td' || c.tag === 'th')).map((c) => cellOf(c, isHeader || c.tag === 'th'));
    return clean({ cells, style: Object.keys(s).length ? s : undefined });
  };
  const visit = (children: Child[], section: 'head' | 'body') => {
    for (const c of children) {
      if (typeof c === 'string') continue;
      if (c.tag === 'thead') visit(c.children, 'head');
      else if (c.tag === 'tbody' || c.tag === 'tfoot') visit(c.children, 'body');
      else if (c.tag === 'tr') (section === 'head' ? header : rows).push(rowOf(c, section === 'head'));
    }
  };
  visit(el.children, 'body');
  // no <thead>: a first row made only of <th> is the header
  const first = el.children.find((c): c is El => typeof c !== 'string' && (c.tag === 'tr' || c.tag === 'tbody'));
  const firstTr = first?.tag === 'tbody' ? first.children.find((c): c is El => typeof c !== 'string' && c.tag === 'tr') : first;
  if (!header.length && firstTr && firstTr.children.every((c) => typeof c === 'string' || c.tag === 'th') && rows.length > 1) header.push(rows.shift()!);

  const count = Math.max(1, ...[...header, ...rows].map((r) => r.cells.reduce((n, c) => n + (c.colSpan ?? 1), 0)));
  // widths: from the first row whose cells all span one column
  const sample = [...header, ...rows].find((r) => r.cells.length === count);
  const columns: Column[] = Array.from({ length: count }, (_, i) => {
    const s = sample?.cells[i]?.style;
    return clean({ width: s?.width ?? 'auto', align: s?.textAlign });
  });
  // pad rows that cover fewer columns than the table
  for (const r of [...header, ...rows]) {
    const covered = r.cells.reduce((n, c) => n + (c.colSpan ?? 1), 0);
    for (let i = covered; i < count; i++) r.cells.push({ children: [] });
  }
  const { width: _w, ...tableStyle } = style;
  return clean({
    type: 'table' as const,
    columns,
    header: header.length ? header : undefined,
    rows,
    cellStyle: { padding: { top: 2, right: 3, bottom: 2, left: 3 } },
    style: Object.keys(tableStyle).length ? tableStyle : undefined,
  });
}

function componentTable(el: El): Node {
  const p = props<Record<string, unknown>>(el);
  const header: Row[] = [];
  const rows: Row[] = [];
  for (const r of el.children) {
    if (typeof r === 'string' || r.tag !== 'turbopdf-row') continue;
    const { header: isHeader, ...rp } = props<{ header?: boolean } & Omit<Row, 'cells'>>(r);
    const cells: Cell[] = r.children
      .filter((c): c is El => typeof c !== 'string' && c.tag === 'turbopdf-cell')
      .map((c) => ({ ...props<Omit<Cell, 'children'>>(c), children: blocks(c.children) }));
    (isHeader ? header : rows).push({ ...rp, cells });
  }
  return clean({ type: 'table' as const, ...(p as object), columns: (p.columns as Column[]) ?? [], header: header.length ? header : undefined, rows });
}

// ------------------------------------------------------------------ pages

export interface PageParts {
  props: Partial<Page>;
  header?: Node[];
  footer?: Node[];
  children: Node[];
}

/** Find `<turbopdf-page>` elements (with their header / footer) at the top of the markup. */
export function pagesOf(children: Child[]): { meta?: Record<string, unknown>; pages: PageParts[] } {
  usedIds = new Set();
  let top = children;
  let meta: Record<string, unknown> | undefined;
  const doc = children.find((c): c is El => typeof c !== 'string' && c.tag === 'turbopdf-document');
  if (doc) {
    meta = props<Record<string, unknown>>(doc);
    top = doc.children;
  }
  const pages: PageParts[] = [];
  let loose: Child[] = [];
  const flushLoose = () => {
    const nodes = blocks(loose);
    if (nodes.length) pages.push({ props: {}, children: nodes });
    loose = [];
  };
  for (const c of top) {
    if (typeof c !== 'string' && c.tag === 'turbopdf-page') {
      flushLoose();
      const part = (tag: string) => {
        const el = c.children.find((x): x is El => typeof x !== 'string' && x.tag === tag);
        return el ? blocks(el.children) : undefined;
      };
      const header = part('turbopdf-header');
      const footer = part('turbopdf-footer');
      pages.push({ props: props(c), header: header?.length ? header : undefined, footer: footer?.length ? footer : undefined, children: part('turbopdf-body') ?? [] });
    } else {
      loose.push(c);
    }
  }
  flushLoose();
  return { meta, pages };
}
