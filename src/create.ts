import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { Readable } from 'node:stream';
import { PapierError } from './errors';
import { checkScale, fromHtml, fromPngs } from './preview';
import { withSlot } from './queue';
import { isReactElement, renderPdf } from './rust/engine';
import type { CreateOptions, Pdf, PngOptions, PngPage, Preview, ReactElementLike, ResponseOptions, Warning } from './types';

const SOURCES = ['html', 'react', 'markdown', 'tree'] as const;

/** Options of earlier, browser-based versions, with what to do instead. */
const REMOVED: Record<string, string> = {
  url: 'Papier renders without a browser and never touches the network. Fetch the page yourself and pass { html, baseDir }.',
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

class PdfImpl implements Pdf {
  readonly pages: number;
  readonly warnings: Warning[];
  private readonly buffer: Buffer;
  private readonly draw: (scale: number) => Promise<PngPage[]>;
  private readonly html: () => Promise<Preview>;
  private pngs = new Map<number, Promise<PngPage[]>>();
  private htmlPreview?: Promise<Preview>;

  constructor(buffer: Buffer, pages: number, warnings: Warning[], draw: (scale: number) => Promise<PngPage[]>, html: () => Promise<Preview>) {
    this.buffer = buffer;
    this.pages = pages;
    this.warnings = warnings;
    this.draw = draw;
    this.html = html;
  }

  toBuffer(): Buffer {
    return this.buffer;
  }

  toStream(): Readable {
    return Readable.from([this.buffer]);
  }

  toResponse(options: ResponseOptions = {}): Response {
    const filename = options.filename ?? 'document.pdf';
    const ascii = filename.replace(/[^\x20-\x7e]|["\\]/g, '_');
    return new Response(new Uint8Array(this.buffer), {
      status: options.status ?? 200,
      headers: {
        'Content-Type': 'application/pdf',
        'Content-Length': String(this.buffer.length),
        'Content-Disposition': `${options.disposition ?? 'inline'}; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(filename)}`,
        ...options.headers,
      },
    });
  }

  async save(path: string): Promise<void> {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, this.buffer);
  }

  preview(): Promise<Preview> {
    if (!this.htmlPreview) {
      const p = this.html();
      this.htmlPreview = p;
      p.catch(() => (this.htmlPreview = undefined));
    }
    return this.htmlPreview;
  }

  toPng(options: PngOptions = {}): Promise<PngPage[]> {
    const scale = options.scale ?? 2;
    let cached = this.pngs.get(scale);
    if (!cached) {
      cached = this.draw(scale);
      this.pngs.set(scale, cached);
      cached.catch(() => this.pngs.delete(scale));
    }
    return cached;
  }
}

const invalid = (message: string) => new PapierError('invalid_source', message);

function normalizeInput(input: unknown): CreateOptions {
  if (typeof input === 'string') return { html: input };
  if (isReactElement(input)) return { react: input };
  if (typeof input === 'object' && input !== null) return input as CreateOptions;
  throw invalid('create() expects an HTML string, a React element or an options object, e.g. create({ markdown: "# Hi" }).');
}

function assertStrict(o: CreateOptions, warnings: Warning[]): void {
  if (!o.strict || !warnings.length) return;
  const err = new PapierError('warnings_found', `Rendering produced ${warnings.length} warning(s) and \`strict\` is on:\n${warnings.map((w) => `  - [${w.type}] ${w.message}`).join('\n')}`);
  err.warnings = warnings;
  throw err;
}

/**
 * Create a PDF from HTML, a React element, Markdown or a document tree.
 *
 * @example
 * const pdf = await create('<h1>Invoice</h1>');
 * await pdf.save('invoice.pdf');
 */
export async function create(html: string): Promise<Pdf>;
export async function create(element: ReactElementLike): Promise<Pdf>;
export async function create(options: CreateOptions): Promise<Pdf>;
export async function create(input: string | ReactElementLike | CreateOptions): Promise<Pdf> {
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
  if (o.signal?.aborted) throw new PapierError('aborted', 'Rendering was aborted before it started.');

  const r = await withSlot(o.signal, () => renderPdf(o));
  assertStrict(o, r.warnings);
  return new PdfImpl(
    r.pdf,
    r.pages,
    r.warnings,
    async (scale) => {
      checkScale(scale);
      return fromPngs(await r.png(scale));
    },
    async () => fromHtml(await r.html(), r.meta),
  );
}
