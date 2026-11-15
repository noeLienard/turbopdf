// `create()`: source → document tree → native render (Rust engine, no browser, no network).
import type { HtmlFromEngine } from '../preview';
import { readFile, realpath } from 'node:fs/promises';
import { extname, isAbsolute, posix, relative, resolve } from 'node:path';
import { TurboPdfError } from '../errors';
import { FORMATS, normalizeMargin, toPx } from '../formats';
import { holdSlotUntil } from '../queue';
import type { Document, Edges, FontFace, FontStyle, Node, Page, PageSize, Warning as TreeWarning, Watermark } from '../tree';
import type { CreateOptions, Format, ReactElementLike, Warning } from '../types';
import { applyStylesheet, parseStylesheets, styleElements, stylesheetLinks, type FontFaceRule, type PageRule, type StyleSource, type Stylesheet } from './cascade';
import { parseCss } from './css';
import { markdownToNodes } from './markdown';
import { blocks, pagesOf, parseMarkup, type Child, type El, type PageParts } from './markup';
import { loadNative, nativeError, type NativeDocument } from './native';

const invalid = (message: string) => new TurboPdfError('invalid_source', message);

/** True for objects that look like a React element. */
export function isReactElement(value: unknown): value is ReactElementLike {
  return typeof value === 'object' && value !== null && '$$typeof' in value;
}

/** Built-in rules: the page-break utility classes. */
const UA_CSS = `
.turbopdf-break-before { break-before: page }
.turbopdf-break-after { break-after: page }
.turbopdf-keep-together { break-inside: avoid }
`;

const pt = (v: string | number) => toPx(v) * 0.75;
const PAPER: Record<string, Format> = { a3: 'A3', a4: 'A4', a5: 'A5', letter: 'Letter', legal: 'Legal' };

interface Geometry {
  size: PageSize;
  orientation?: 'portrait' | 'landscape';
  margin: Edges;
}

/** Explicit options win, then the document's `@page` rule, then the defaults (A4 portrait, 10mm). */
function geometry(o: CreateOptions, page: PageRule): Geometry {
  let size: PageSize = 'A4';
  let orientation: 'portrait' | 'landscape' | undefined = 'portrait';

  if (page.size) {
    const tokens = page.size.toLowerCase().split(/\s+/);
    const paper = tokens.map((t) => PAPER[t]).find(Boolean);
    const lengths = tokens.map((t) => parseCss(`width:${t}`).width).filter((v): v is number => typeof v === 'number');
    if (paper) size = paper;
    if (lengths.length === 2) {
      size = { width: lengths[0]!, height: lengths[1]! };
      orientation = undefined;
    }
    if (tokens.includes('landscape')) orientation = 'landscape';
  }

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
  if (o.margin === undefined && page.margin) {
    margin = parseCss(`padding:${page.margin}`).padding ?? 28.35;
  } else {
    const m = normalizeMargin(o.margin);
    margin = { top: pt(m.top), right: pt(m.right), bottom: pt(m.bottom), left: pt(m.left) };
  }
  return { size, ...(orientation ? { orientation } : {}), margin };
}

async function fontsOf(o: CreateOptions, buffers?: Buffer[]): Promise<FontFace[] | undefined> {
  if (!o.fonts?.length) return undefined;
  const out: FontFace[] = [];
  for (const f of o.fonts) {
    if (!f.family || (f.path === undefined) === (f.data === undefined)) throw invalid('Each font needs a `family` and exactly one of `path` or `data`.');
    if (f.path !== undefined && !['.ttf', '.otf'].includes(extname(f.path).toLowerCase())) {
      throw invalid(`Unsupported font file "${f.path}". Use .ttf or .otf (WOFF / WOFF2 are not supported).`);
    }
    // a path is read by the engine, inside baseDir; bytes go next to the tree, without base64
    let source: FontFace['source'];
    if (f.path !== undefined) source = { file: f.path };
    else if (buffers) source = { buffer: buffers.push(Buffer.from(f.data!.buffer, f.data!.byteOffset, f.data!.byteLength)) - 1 };
    else source = { data: Buffer.from(f.data!).toString('base64') };
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

const isRemote = (url: string) => /^[a-z][\w+.-]*:/i.test(url) && !url.startsWith('file:');

/** A local file inside `baseDir`, or why it cannot be read. */
async function readInside(url: string, baseDir: string): Promise<{ bytes: Buffer } | { reason: string }> {
  try {
    const requested = url.replace(/^file:\/\//, '');
    const unresolvedBase = resolve(baseDir);
    const unresolvedPath = resolve(unresolvedBase, requested);
    const unresolvedRelative = relative(unresolvedBase, unresolvedPath);
    if (unresolvedRelative.startsWith('..') || isAbsolute(unresolvedRelative)) return { reason: `it is outside baseDir (${baseDir}).` };
    const base = await realpath(unresolvedBase);
    const path = await realpath(unresolvedPath);
    const rel = relative(base, path);
    if (rel.startsWith('..') || isAbsolute(rel)) return { reason: `it is outside baseDir (${baseDir}).` };
    return { bytes: await readFile(path) };
  } catch {
    return { reason: 'file not found.' };
  }
}

/** Read a local stylesheet, never outside `baseDir`. Remote stylesheets are not fetched. */
async function linked(href: string, baseDir: string, warnings: Warning[]): Promise<StyleSource | undefined> {
  const fail = (reason: string) => {
    warnings.push({ type: 'stylesheet_failed', url: href, message: `Stylesheet ${href} was not applied: ${reason}` });
    return undefined;
  };
  if (isRemote(href)) return fail('remote stylesheets are never fetched. Inline it in a <style> element or pass it with the `css` option.');
  const r = await readInside(href, baseDir);
  if ('reason' in r) return fail(r.reason);
  // url(…)s inside it are relative to its own folder
  return { css: r.bytes.toString('utf8'), base: posix.dirname(href.replace(/^file:\/\//, '').replace(/\\/g, '/')).replace(/^\.$/, '') };
}

const FONT_FORMATS = new Set(['truetype', 'opentype', 'ttf', 'otf']);

/** `@font-face` rules → embedded fonts: the first local .ttf / .otf source of each rule. */
async function fontFaces(rules: FontFaceRule[], baseDir: string, warnings: Warning[]): Promise<FontFace[]> {
  const out: FontFace[] = [];
  for (const rule of rules) {
    const fail = (url: string, reason: string) =>
      warnings.push({ type: 'font_failed', url, message: `Font "${rule.family}" from ${url} was not loaded: ${reason} Text in "${rule.family}" uses Inter instead.` });
    const usable = rule.src.find((s) => !isRemote(s.url) && (s.format ? FONT_FORMATS.has(s.format) : /\.(ttf|otf)$/i.test(s.url)));
    if (!usable) {
      const first = rule.src[0]!;
      fail(first.url, isRemote(first.url) ? 'web fonts are never downloaded. Put the .ttf / .otf file next to your HTML and use url(fonts/…).' : 'only .ttf and .otf files are supported (not WOFF / WOFF2): convert it, e.g. with fonttools.');
      continue;
    }
    const r = await readInside(usable.url, baseDir);
    if ('reason' in r) {
      fail(usable.url, r.reason);
      continue;
    }
    const w = (rule.weight ?? 'normal').trim().split(/\s+/)[0]!.toLowerCase();
    const weight = w === 'bold' ? 700 : w === 'normal' ? 400 : Math.min(900, Math.max(100, Math.round(Number(w) / 100) * 100 || 400));
    const style: FontStyle = /italic|oblique/i.test(rule.style ?? '') ? 'italic' : 'normal';
    out.push({ family: rule.family, weight, style, source: { data: r.bytes.toString('base64') } });
  }
  return out;
}

function find(children: Child[], tag: string): El | undefined {
  for (const c of children) {
    if (typeof c === 'string') continue;
    if (c.tag === tag) return c;
    const inner = find(c.children, tag);
    if (inner) return inner;
  }
  return undefined;
}

interface Parts {
  meta?: Record<string, unknown>;
  pages: PageParts[];
  sheet: Stylesheet;
}

function watermarkOf(w: CreateOptions['watermark']): Watermark | undefined {
  if (w === undefined) return undefined;
  const stamp = typeof w === 'string' ? { text: w } : w;
  if (!stamp || typeof stamp.text !== 'string' || !stamp.text.trim()) throw invalid('`watermark` must be a non-empty string or { text, color?, fontSize?, angle? }.');
  return stamp;
}

/** HTML (from a string or from React) → pages, with the CSS cascade applied. */
async function markupParts(markup: string, o: CreateOptions, baseDir: string, warnings: Warning[]): Promise<Parts> {
  const children = parseMarkup(markup);
  const css: StyleSource[] = [UA_CSS];
  for (const href of stylesheetLinks(children)) {
    const text = await linked(href, baseDir, warnings);
    if (text) css.push(text);
  }
  css.push(...styleElements(children));
  if (o.css !== undefined) {
    if (typeof o.css !== 'string') throw invalid('`css` must be a string.');
    css.push(o.css);
  }
  const sheet = parseStylesheets(css);
  applyStylesheet(children, sheet);

  const { meta, pages } = pagesOf(children);
  // <html lang> and <title> fill the metadata a component tree would give
  const m: Record<string, unknown> = { ...meta };
  const lang = find(children, 'html')?.attrs.lang;
  const title = find(children, 'title')?.children.filter((x) => typeof x === 'string').join('').trim();
  if (lang && m.lang === undefined) m.lang = lang;
  if (title && m.title === undefined) m.title = title;
  return { meta: Object.keys(m).length ? m : undefined, pages, sheet };
}

/** `header` / `footer` options are HTML: converted like the body, `{page}` / `{pages}` kept. */
function band(html: string | undefined, sheet: Stylesheet): Node[] | undefined {
  if (html === undefined) return undefined;
  const children = parseMarkup(html);
  applyStylesheet(children, sheet);
  const nodes = blocks(children);
  return nodes.length ? nodes : undefined;
}

/**
 * Build the document tree for any source. Exported for tests and debugging. With `buffers`, bytes given in
 * the options are appended to it and referenced as `{ buffer: n }` instead of being base64-encoded.
 */
export async function toTree(o: CreateOptions, warnings: Warning[] = [], buffers?: Buffer[]): Promise<Document> {
  if (o.tree !== undefined) return o.tree;
  const baseDir = o.baseDir ?? process.cwd();
  let parts: Parts;
  if (o.markdown !== undefined) {
    if (typeof o.markdown !== 'string' || !o.markdown.trim()) throw invalid('`markdown` must be a non-empty string.');
    if (o.css !== undefined) throw invalid('`css` applies to `html` and `react` sources. Markdown is styled by TurboPDF; use `html` for full control.');
    parts = { pages: [{ props: {}, children: markdownToNodes(o.markdown) }], sheet: parseStylesheets([]) };
  } else if (o.html !== undefined) {
    if (typeof o.html !== 'string' || !o.html.trim()) throw invalid('`html` must be a non-empty string.');
    parts = await markupParts(o.html, o, baseDir, warnings);
  } else {
    parts = await markupParts(await reactMarkup(o.react), o, baseDir, warnings);
  }

  const geo = geometry(o, parts.sheet.page);
  const header = band(o.header, parts.sheet);
  const footer = band(o.footer, parts.sheet);
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
  const fonts = [...(await fontFaces(parts.sheet.fontFaces, baseDir, warnings)), ...((await fontsOf(o, buffers)) ?? [])];
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
  pdf: Buffer;
  pages: number;
  warnings: Warning[];
  png(scale: number): Promise<Buffer[]>;
  html(): Promise<HtmlFromEngine>;
  meta: { title?: string; lang?: string };
}

export function renderPdf(o: CreateOptions): Promise<RenderResult> {
  const completion = (async (): Promise<RenderResult> => {
    const native = loadNative();
    const sdkWarnings: Warning[] = [];
    const buffers: Buffer[] = [];
    const doc = await toTree(o, sdkWarnings, buffers);
    let rendered: NativeDocument;
    try {
      rendered = await native.render(JSON.stringify(doc), { baseDir: o.baseDir ?? process.cwd(), buffers });
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
