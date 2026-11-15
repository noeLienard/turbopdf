import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { TurboPdfError } from './errors';
import type { PngPage, Preview, PreviewPage } from './types';

export function checkScale(scale: number): void {
  if (!(scale > 0 && scale <= 8)) throw new TurboPdfError('invalid_source', `Invalid PNG scale ${scale}. Use a number between 0 and 8 (default 2).`);
}

/** PNGs from the engine → pages (size read from the PNG header). */
export function fromPngs(pngs: Buffer[]): PngPage[] {
  return pngs.map((png, i) => ({
    page: i + 1,
    width: png.readUInt32BE(16),
    height: png.readUInt32BE(20),
    toBuffer: () => png,
    toDataUrl: () => `data:image/png;base64,${png.toString('base64')}`,
    async save(path: string) {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, png);
    },
  }));
}

export interface HtmlFromEngine {
  style: string;
  pages: { width: number; height: number; html: string }[];
}

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

/** HTML preview from the engine → {@link Preview}. */
export function fromHtml(p: HtmlFromEngine, meta: { title?: string; lang?: string }): Preview {
  const html: PreviewPage[] = p.pages.map((x, i) => ({ page: i + 1, width: x.width, height: x.height, html: x.html }));
  const toHtml = () =>
    `<!DOCTYPE html><html${meta.lang ? ` lang="${escape(meta.lang)}"` : ''}><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>${escape(meta.title ?? 'Preview')}</title>` +
    `<style>body{margin:0;padding:24px 0;background:#e5e7eb}.turbopdf-page{margin:0 auto 24px;box-shadow:0 2px 12px rgba(0,0,0,.15)}${p.style}</style></head><body>${html.map((x) => x.html).join('')}</body></html>`;
  return {
    style: p.style,
    pages: html,
    toHtml,
    toResponse: (init) => new Response(toHtml(), { ...init, headers: { 'Content-Type': 'text/html; charset=utf-8', ...init?.headers } }),
    async save(path) {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, toHtml());
    },
  };
}
