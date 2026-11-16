// `create()` without any Node API: validation, rendering through the engine set by the entry point, and a
// PDF with web-standard outputs. `create.ts` (Node) and `edge.ts` (edge runtimes, browsers) build on it.
import { toBase64, u32be } from './bytes';
import { TurboPdfError } from './errors';
import { withSlot } from './queue';
import { isReactElement, renderPdf, type RenderResult } from './rust/engine';
import type { CreateOptions, EdgePdf, EdgePngPage, EdgePreview, PngOptions, PreviewPage, ResponseOptions, Warning } from './types';
import type { HtmlFromEngine } from './preview-types';

const SOURCES = ['html', 'react', 'markdown', 'tree'] as const;

/** Options of earlier, browser-based versions, with what to do instead. */
const REMOVED: Record<string, string> = {
  url: 'TurboPDF renders without a browser and never touches the network. Fetch the page yourself and pass { html, baseDir }.',
  engine: 'There is a single engine now (Rust); remove the option.',
  printBackground: 'Backgrounds are always printed; remove the option.',
  media: 'Stylesheets are applied as for print (`@media print` rules apply); remove the option.',
  javascript: 'Scripts are never run; remove the option.',
  headers: 'Nothing is fetched over the network; remove the option.',
  cookies: 'Nothing is fetched over the network; remove the option.',
  allowHosts: 'Nothing is fetched over the network; remove the option.',
  allowFileUrls: 'Local files are read from `baseDir`; remove the option.',
  blockPrivateNetwork: 'Nothing is fetched over the network; remove the option.',
  deterministic: 'The output is always deterministic (same input, same bytes); remove the option.',
};

const invalid = (message: string) => new TurboPdfError('invalid_source', message);

function normalizeInput(input: unknown): CreateOptions {
  if (typeof input === 'string') return { html: input };
  if (isReactElement(input)) return { react: input };
  if (typeof input === 'object' && input !== null) return input as CreateOptions;
  throw invalid('create() expects an HTML string, a React element or an options object, e.g. create({ markdown: "# Hi" }).');
}

function assertStrict(o: CreateOptions, warnings: Warning[]): void {
  if (!o.strict || !warnings.length) return;
  const err = new TurboPdfError('warnings_found', `Rendering produced ${warnings.length} warning(s) and \`strict\` is on:\n${warnings.map((w) => `  - [${w.type}] ${w.message}`).join('\n')}`);
  err.warnings = warnings;
  throw err;
}

/** Validate the input and render it. */
export async function render(input: unknown): Promise<RenderResult> {
  const o = normalizeInput(input);
  for (const [key, help] of Object.entries(REMOVED)) {
    if ((o as unknown as Record<string, unknown>)[key] !== undefined) {
      const what = key === 'url' ? 'URL sources are no longer supported' : `\`${key}\` is no longer supported`;
      throw invalid(`${what}: ${help}`);
    }
  }
  const given = SOURCES.filter((k) => o[k] !== undefined);
  if (given.length !== 1) {
    throw invalid(
      given.length === 0
        ? 'No source provided. Pass exactly one of: html, react, markdown, tree.'
        : `Several sources provided (${given.join(', ')}). Pass exactly one of: html, react, markdown, tree.`,
    );
  }
  if (o.signal?.aborted) throw new TurboPdfError('aborted', 'Rendering was aborted before it started.');
  const r = await withSlot(o.signal, () => renderPdf(o));
  assertStrict(o, r.warnings);
  return r;
}

export function checkScale(scale: number): void {
  if (!(scale > 0 && scale <= 8)) throw new TurboPdfError('invalid_source', `Invalid PNG scale ${scale}. Use a number between 0 and 8 (default 2).`);
}

/** PNGs from the engine → pages (size read from the PNG header). */
export function pngPages(pngs: Uint8Array[]): EdgePngPage[] {
  return pngs.map((png, i) => ({
    page: i + 1,
    width: u32be(png, 16),
    height: u32be(png, 20),
    toUint8Array: () => png,
    toDataUrl: () => `data:image/png;base64,${toBase64(png)}`,
  }));
}

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

/** HTML preview from the engine → {@link EdgePreview}. */
export function previewOf(p: HtmlFromEngine, meta: { title?: string; lang?: string }): EdgePreview {
  const pages: PreviewPage[] = p.pages.map((x, i) => ({ page: i + 1, width: x.width, height: x.height, html: x.html }));
  const toHtml = () =>
    `<!DOCTYPE html><html${meta.lang ? ` lang="${escape(meta.lang)}"` : ''}><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>${escape(meta.title ?? 'Preview')}</title>` +
    `<style>body{margin:0;padding:24px 0;background:#e5e7eb}.turbopdf-page{margin:0 auto 24px;box-shadow:0 2px 12px rgba(0,0,0,.15)}${p.style}</style></head><body>${pages.map((x) => x.html).join('')}</body></html>`;
  return {
    style: p.style,
    pages,
    toHtml,
    toResponse: (init) => new Response(toHtml(), { ...init, headers: { 'Content-Type': 'text/html; charset=utf-8', ...init?.headers } }),
  };
}

/**
 * A rendered PDF with web-standard outputs; PNGs and the preview are made once (per scale), on demand.
 * `Png` / `Html` let the Node build add its file helpers to what is cached.
 */
export class EdgePdfImpl<Png extends EdgePngPage = EdgePngPage, Html extends EdgePreview = EdgePreview> implements EdgePdf {
  readonly pages: number;
  readonly warnings: Warning[];
  protected readonly bytes: Uint8Array;
  private readonly r: RenderResult;
  private readonly png: (p: EdgePngPage) => Png;
  private readonly html: (p: EdgePreview) => Html;
  private pngs = new Map<number, Promise<Png[]>>();
  private htmlPreview?: Promise<Html>;

  constructor(r: RenderResult, png?: (p: EdgePngPage) => Png, html?: (p: EdgePreview) => Html) {
    this.r = r;
    this.png = png ?? ((p) => p as Png);
    this.html = html ?? ((p) => p as Html);
    this.bytes = r.pdf;
    this.pages = r.pages;
    this.warnings = r.warnings;
  }

  toUint8Array(): Uint8Array {
    return this.bytes;
  }

  toResponse(options: ResponseOptions = {}): Response {
    const filename = options.filename ?? 'document.pdf';
    const ascii = filename.replace(/[^\x20-\x7e]|["\\]/g, '_');
    return new Response(this.bytes as Uint8Array<ArrayBuffer>, {
      status: options.status ?? 200,
      headers: {
        'Content-Type': 'application/pdf',
        'Content-Length': String(this.bytes.length),
        'Content-Disposition': `${options.disposition ?? 'inline'}; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(filename)}`,
        ...options.headers,
      },
    });
  }

  preview(): Promise<Html> {
    if (!this.htmlPreview) {
      const p = this.r.html().then((h) => this.html(previewOf(h, this.r.meta)));
      this.htmlPreview = p;
      p.catch(() => (this.htmlPreview = undefined));
    }
    return this.htmlPreview;
  }

  toPng(options: PngOptions = {}): Promise<Png[]> {
    const scale = options.scale ?? 2;
    let cached = this.pngs.get(scale);
    if (!cached) {
      try {
        checkScale(scale);
      } catch (e) {
        return Promise.reject(e);
      }
      cached = this.r.png(scale).then((pngs) => pngPages(pngs).map(this.png));
      this.pngs.set(scale, cached);
      cached.catch(() => this.pngs.delete(scale));
    }
    return cached;
  }
}
