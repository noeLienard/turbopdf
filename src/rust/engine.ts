// `create()`: source → document tree → native render (Rust engine, no browser, no network).
import type { HtmlFromEngine } from '../preview-types';
import { TurboPdfError } from '../errors';
import { FORMATS, normalizeMargin, toPx } from '../formats';
import { holdSlotUntil } from '../queue';
import type { Document, Edges, FontFace, Node, Page, PageSize, Warning as TreeWarning, Watermark } from '../tree';
import type { CreateOptions, Format, ReactElementLike, Warning } from '../types';
import { markdownToNodes } from './markdown';
import { toBase64 } from '../bytes';
import { nativeError, type NativeDocument } from './engine-api';
import { defaultBaseDir, engine } from './provider';

const invalid = (message: string) => new TurboPdfError('invalid_source', message);

/** True for objects that look like a React element. */
export function isReactElement(value: unknown): value is ReactElementLike {
  return typeof value === 'object' && value !== null && '$$typeof' in value;
}

const pt = (v: string | number) => toPx(v) * 0.75;

interface Geometry {
  size: PageSize;
  orientation?: 'portrait' | 'landscape';
  margin: Edges;
}

/** The document's `@page` rule, resolved by the engine. */
interface PageSetup {
  size?: PageSize;
  landscape?: boolean;
  margin?: Edges;
}

/** Explicit options win, then the document's `@page` rule, then the defaults (A4 portrait, 10mm). */
function geometry(o: CreateOptions, page: PageSetup): Geometry {
  let size: PageSize = 'A4';
  let orientation: 'portrait' | 'landscape' | undefined = 'portrait';

  if (page.size) {
    size = page.size;
    if (typeof size !== 'string') orientation = undefined;
  }
  if (page.landscape) orientation = 'landscape';

  if (o.width !== undefined || o.height !== undefined) {
    if (o.width === undefined || o.height === undefined) throw invalid('Custom page size needs both `width` and `height`.');
    const width = pt(o.width);
    const height = pt(o.height);
    if (!(width > 0 && height > 0)) throw invalid('`width` and `height` must be positive.');
    size = { width, height };
    orientation = undefined;
  } else if (o.format !== undefined || o.orientation !== undefined) {
    const format: Format = o.format ?? (typeof size === 'string' ? size : 'A4');
    if (!FORMATS.includes(format)) throw invalid(`Unknown format "${String(format)}". Use one of: ${FORMATS.join(', ')}.`);
    const orient = o.orientation ?? orientation ?? 'portrait';
    if (orient !== 'portrait' && orient !== 'landscape') throw invalid(`Unknown orientation "${String(orient)}". Use 'portrait' or 'landscape'.`);
    size = format;
    orientation = orient;
  }

  let margin: Edges;
  if (o.margin === undefined && page.margin !== undefined) {
    margin = page.margin;
  } else {
    const m = normalizeMargin(o.margin);
    margin = { top: pt(m.top), right: pt(m.right), bottom: pt(m.bottom), left: pt(m.left) };
  }
  return { size, ...(orientation ? { orientation } : {}), margin };
}

async function fontsOf(o: CreateOptions, buffers?: Uint8Array[]): Promise<FontFace[] | undefined> {
  if (!o.fonts?.length) return undefined;
  const out: FontFace[] = [];
  for (const f of o.fonts) {
    if (!f.family || (f.path === undefined) === (f.data === undefined)) throw invalid('Each font needs a `family` and exactly one of `path` or `data`.');
    if (f.path !== undefined && !/\.(ttf|otf)$/i.test(f.path)) {
      throw invalid(`Unsupported font file "${f.path}". Use .ttf or .otf (WOFF / WOFF2 are not supported).`);
    }
    // a path is read by the engine, inside baseDir; bytes go next to the tree, without base64
    let source: FontFace['source'];
    if (f.path !== undefined) source = { file: f.path };
    else if (buffers) source = { buffer: buffers.push(f.data!) - 1 };
    else source = { data: toBase64(f.data!) };
    const weight = f.weight === undefined ? 400 : f.weight === 'bold' ? 700 : f.weight === 'normal' ? 400 : Number(f.weight);
    if (!(weight >= 100 && weight <= 900)) throw invalid(`Invalid font weight "${String(f.weight)}" for "${f.family}". Use 100–900.`);
    out.push({ family: f.family, weight, style: f.style ?? 'normal', source });
  }
  return out;
}

async function reactMarkup(element: unknown): Promise<string> {
  if (!isReactElement(element)) throw invalid('`react` must be a React element, e.g. react: <Invoice />.');
  let server: { renderToStaticMarkup(el: unknown): string };
  try {
    server = (await import('react-dom/server')) as unknown as typeof server;
  } catch (cause) {
    throw new TurboPdfError('react_not_installed', 'A React element was passed but "react" and "react-dom" are not installed. Run: npm install react react-dom', { cause });
  }
  try {
    return server.renderToStaticMarkup(element);
  } catch (cause) {
    throw new TurboPdfError('render_failed', `Your React component threw while rendering: ${(cause as Error).message}`, { cause });
  }
}

interface PageParts {
  props: Partial<Page>;
  header?: Node[];
  footer?: Node[];
  children: Node[];
}

/** What the engine's HTML converter returns (`convertHtml`). */
interface Parts {
  meta?: Record<string, unknown>;
  pages: PageParts[];
  header?: Node[];
  footer?: Node[];
  page: PageSetup;
  /** `@font-face` fonts, as `{ buffer: n }` sources into `buffers`. */
  fonts: FontFace[];
  buffers: Uint8Array[];
}

function watermarkOf(w: CreateOptions['watermark']): Watermark | undefined {
  if (w === undefined) return undefined;
  const stamp = typeof w === 'string' ? { text: w } : w;
  if (!stamp || typeof stamp.text !== 'string' || !stamp.text.trim()) throw invalid('`watermark` must be a non-empty string or { text, color?, fontSize?, angle? }.');
  return stamp;
}

/**
 * HTML (from a string, from React, or only the header / footer of a Markdown document) → pages, by the
 * engine: HTML5 parsing, the CSS cascade, linked stylesheets and `@font-face` files read inside `baseDir`.
 */
async function markupParts(markup: string, o: CreateOptions, baseDir: string | undefined, warnings: Warning[]): Promise<Parts> {
  if (o.css !== undefined && typeof o.css !== 'string') throw invalid('`css` must be a string.');
  for (const k of ['header', 'footer'] as const) {
    if (o[k] !== undefined && typeof o[k] !== 'string') throw invalid(`\`${k}\` must be an HTML string.`);
  }
  let out: { parts: string; buffers: Uint8Array[] };
  try {
    out = await engine().convertHtml(markup, { baseDir, css: o.css, header: o.header, footer: o.footer });
  } catch (e) {
    throw nativeError(e);
  }
  const parts = JSON.parse(out.parts) as Omit<Parts, 'buffers'> & { warnings: Warning[] };
  warnings.push(...parts.warnings);
  return { ...parts, buffers: out.buffers };
}

/**
 * Build the document tree for any source. Exported for tests and debugging. With `buffers`, bytes given in
 * the options are appended to it and referenced as `{ buffer: n }` instead of being base64-encoded.
 */
export async function toTree(o: CreateOptions, warnings: Warning[] = [], buffers?: Uint8Array[]): Promise<Document> {
  if (o.tree !== undefined) return o.tree;
  const baseDir = o.baseDir ?? defaultBaseDir();
  let parts: Parts;
  if (o.markdown !== undefined) {
    if (typeof o.markdown !== 'string' || !o.markdown.trim()) throw invalid('`markdown` must be a non-empty string.');
    if (o.css !== undefined) throw invalid('`css` applies to `html` and `react` sources. Markdown is styled by TurboPDF; use `html` for full control.');
    const pages = [{ props: {}, children: markdownToNodes(o.markdown) }];
    // the header / footer options are HTML, converted by the engine
    const bands = o.header !== undefined || o.footer !== undefined ? await markupParts('', o, baseDir, warnings) : undefined;
    parts = { pages, header: bands?.header, footer: bands?.footer, page: {}, fonts: [], buffers: [] };
  } else if (o.html !== undefined) {
    if (typeof o.html !== 'string' || !o.html.trim()) throw invalid('`html` must be a non-empty string.');
    parts = await markupParts(o.html, o, baseDir, warnings);
  } else {
    parts = await markupParts(await reactMarkup(o.react), o, baseDir, warnings);
  }

  const geo = geometry(o, parts.page);
  const { header, footer } = parts;
  const watermark = watermarkOf(o.watermark);
  for (const k of ['bookmarks', 'tagged', 'pdfua'] as const) {
    if (o[k] !== undefined && typeof o[k] !== 'boolean') throw invalid(`\`${k}\` must be true or false.`);
  }
  // `<Document bookmarks>` is a document setting, not metadata
  const { bookmarks: docBookmarks, tagged: docTagged, pdfua: docPdfua, ...meta } = (parts.meta ?? {}) as Record<string, unknown>;
  for (const k of ['title', 'lang'] as const) {
    if (o[k] !== undefined && (typeof o[k] !== 'string' || !o[k].trim())) throw invalid(`\`${k}\` must be a non-empty string.`);
    if (o[k] !== undefined) meta[k] = o[k];
  }
  if (o.markdown !== undefined && meta.title === undefined) {
    // Markdown: the first top-level heading names the document
    const h1 = parts.pages[0]?.children.find((n) => n.type === 'heading' && n.level === 1);
    if (h1?.type === 'heading') meta.title = typeof h1.content === 'string' ? h1.content : h1.content.map((s) => s.text).join('');
  }
  const bookmarks = o.bookmarks ?? (docBookmarks as boolean | undefined);
  const tagged = o.tagged ?? (docTagged as boolean | undefined);
  const pdfua = o.pdfua ?? (docPdfua as boolean | undefined);
  if (pdfua && tagged === false) throw invalid('`pdfua: true` needs a tagged PDF: remove `tagged: false`.');
  if (pdfua) {
    const missing = [!meta.title && 'a title (<title>, or <Document title>)', !meta.lang && 'a language (<html lang="fr">, or <Document lang>)'].filter(Boolean);
    if (missing.length) throw invalid(`\`pdfua: true\` needs ${missing.join(' and ')}.`);
  }
  const doc: Document = {
    type: 'document',
    version: 1,
    ...(Object.keys(meta).length ? { meta } : {}),
    ...(bookmarks === false ? { bookmarks: false } : {}),
    ...(tagged === false ? { tagged: false } : {}),
    ...(pdfua ? { pdfua: true } : {}),
    children: (parts.pages.length ? parts.pages : [{ props: {}, children: [] }]).map(
      (p): Page => ({
        type: 'page',
        ...geo,
        ...(watermark ? { watermark } : {}),
        ...p.props,
        ...((p.header ?? header) ? { header: p.header ?? header } : {}),
        ...((p.footer ?? footer) ? { footer: p.footer ?? footer } : {}),
        children: p.children,
      }),
    ),
  };
  // fonts given with the option come last: they win over @font-face rules of the same family and weight
  // `@font-face` bytes from the converter: next to the tree, or inline when only the tree is wanted
  const offset = buffers?.length ?? 0;
  buffers?.push(...parts.buffers);
  const faces = parts.fonts.map((f): FontFace => {
    const n = (f.source as { buffer: number }).buffer;
    return { ...f, source: buffers ? { buffer: n + offset } : { data: toBase64(parts.buffers[n]!) } };
  });
  const fonts = [...faces, ...((await fontsOf(o, buffers)) ?? [])];
  if (fonts.length) doc.fonts = fonts;
  return doc;
}

const at = (path: string, page?: number) => `${path}${page ? ` (page ${page})` : ''}`;

/** Engine warnings → SDK warnings: same `type`, plus an English message saying what to do. */
export function toWarning(w: TreeWarning): Warning {
  switch (w.type) {
    case 'overflow': {
      const how = [w.dx > 0 ? `${w.dx.toFixed(1)}pt too wide` : '', w.dy > 0 ? `${w.dy.toFixed(1)}pt too tall` : ''].filter(Boolean).join(' and ');
      return { type: 'overflow', path: w.path, page: w.page, message: `${at(w.path, w.page)} is ${how} for its box. Give it more room, let it wrap, or make it smaller.` };
    }
    case 'moved_to_next_page':
      return {
        type: 'moved_to_next_page',
        path: w.path,
        page: w.toPage,
        message: `${w.path} was moved from page ${w.fromPage} to page ${w.toPage} so as not to be split (${w.reason.replace(/_/g, ' ')}).`,
      };
    case 'missing_glyph':
      return {
        type: 'missing_glyph',
        path: w.path,
        page: w.page,
        message: `"${w.character}" (U+${w.codepoint.toString(16).toUpperCase().padStart(4, '0')}) is not in font "${w.fontFamily}" at ${at(w.path, w.page)}. Load a font that has it with the \`fonts\` option.`,
      };
    case 'image_failed':
      return { type: 'image_failed', path: w.path, message: `Image ${w.path} could not be loaded: ${w.reason}` };
    case 'missing_alt':
      return {
        type: 'missing_alt',
        path: w.path,
        page: w.page,
        message: `Image ${w.path} (page ${w.page}) has no alt text, so it was marked decorative. Describe it with alt="…", or write alt="" if it is decorative.`,
      };
    case 'broken_link':
      return {
        type: 'broken_link',
        path: w.path,
        page: w.page,
        message: `Link to #${w.target} (page ${w.page}) goes nowhere: no element has id="${w.target}". Add the id, or fix the link.`,
      };
  }
}

function withDeadline<T>(work: Promise<T>, o: CreateOptions): Promise<T> {
  const timeout = o.timeout ?? 30_000;
  if (!(timeout > 0)) throw invalid('`timeout` must be a positive number of milliseconds.');
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new TurboPdfError('render_timeout', `Rendering took longer than ${timeout} ms. Raise \`timeout\` or simplify the document.`)), timeout);
    const onAbort = () => reject(new TurboPdfError('aborted', 'Rendering was aborted.'));
    o.signal?.addEventListener('abort', onAbort, { once: true });
    work.then(resolve, reject).finally(() => {
      clearTimeout(timer);
      o.signal?.removeEventListener('abort', onAbort);
    });
  });
}

export interface RenderResult {
  pdf: Uint8Array;
  pages: number;
  warnings: Warning[];
  png(scale: number): Promise<Uint8Array[]>;
  html(): Promise<HtmlFromEngine>;
  meta: { title?: string; lang?: string };
}

export function renderPdf(o: CreateOptions): Promise<RenderResult> {
  const completion = (async (): Promise<RenderResult> => {
    const native = engine();
    const sdkWarnings: Warning[] = [];
    const buffers: Uint8Array[] = [];
    const doc = await toTree(o, sdkWarnings, buffers);
    let rendered: NativeDocument;
    try {
      rendered = await native.render(JSON.stringify(doc), { baseDir: o.baseDir ?? defaultBaseDir(), buffers });
    } catch (e) {
      throw e instanceof TurboPdfError ? e : nativeError(e);
    }
    const fail = (e: unknown) => Promise.reject(nativeError(e));
    return {
      pdf: rendered.takePdf(),
      pages: rendered.pages,
      warnings: [...sdkWarnings, ...(JSON.parse(rendered.warnings) as TreeWarning[]).map(toWarning)],
      // drawn from the layout above: the document is not parsed or laid out again
      png: (scale) => rendered.png(scale).catch(fail),
      html: () =>
        typeof rendered.html === 'function'
          ? rendered.html().catch(fail)
          : Promise.reject(
              new TurboPdfError(
                'engine_unavailable',
                'The loaded Rust engine is older than this SDK. Restart the process: a native module is never reloaded while Node runs (e.g. stop and rerun `next dev`).',
              ),
            ),
      meta: { title: doc.meta?.title, lang: doc.meta?.lang },
    };
  })();
  return holdSlotUntil(withDeadline(completion, o), completion);
}
