import { fileURLToPath } from 'node:url';
import * as mupdf from 'mupdf';

/** An icon font (VS Code codicons, CC BY 4.0): a real TTF that is not built in, to test font loading. */
export const FONT_PATH = fileURLToPath(new URL('./fixtures/codicon.ttf', import.meta.url));
/** A private-use glyph that exists in that font (so it really gets embedded). */
export const FONT_GLYPH = '';


function open<T>(pdf: Buffer, fn: (doc: mupdf.Document) => T): T {
  const doc = mupdf.Document.openDocument(pdf, 'application/pdf');
  try {
    return fn(doc);
  } finally {
    doc.destroy();
  }
}

/** Page sizes in points. */
export function pageSizes(pdf: Buffer): Array<{ width: number; height: number }> {
  return open(pdf, (doc) =>
    Array.from({ length: doc.countPages() }, (_, i) => {
      const [x0, y0, x1, y1] = doc.loadPage(i).getBounds();
      return { width: x1 - x0, height: y1 - y0 };
    }),
  );
}

/** Text of every page. */
export function pageTexts(pdf: Buffer): string[] {
  return open(pdf, (doc) => Array.from({ length: doc.countPages() }, (_, i) => doc.loadPage(i).toStructuredText('preserve-whitespace').asText()));
}

/** Left edge (pt) of the first text block of a page. */
export function firstTextLeft(pdf: Buffer, pageIndex = 0): number {
  return open(pdf, (doc) => {
    const json = JSON.parse(doc.loadPage(pageIndex).toStructuredText('preserve-whitespace').asJSON()) as { blocks: Array<{ bbox: { x: number } }> };
    return json.blocks[0]!.bbox.x;
  });
}

/** RGB of a pixel of page 1 rendered at scale 1 (= points). */
export function pixelAt(pdf: Buffer, x: number, y: number): [number, number, number] {
  return open(pdf, (doc) => {
    const pix = doc.loadPage(0).toPixmap(mupdf.Matrix.scale(1, 1), mupdf.ColorSpace.DeviceRGB, false, true);
    const px = pix.getPixels();
    const i = (y * pix.getWidth() + x) * 3;
    return [px[i]!, px[i + 1]!, px[i + 2]!];
  });
}

export const pdfHas = (pdf: Buffer, needle: string): boolean => pdf.toString('latin1').toLowerCase().includes(needle.toLowerCase());

/** A document that takes `n` pages (each block is a full page). */
export const pagesOf = (n: number): string => Array.from({ length: n }, (_, i) => `<div style="height:200px;${i < n - 1 ? 'break-after:page;' : ''}">block ${i + 1}</div>`).join('');

/** Links of each page: `{ uri, external, page? (0-based target), bounds }`. */
export function pageLinks(pdf: Buffer): Array<Array<{ uri: string; external: boolean; target?: number; bounds: number[] }>> {
  return open(pdf, (doc) =>
    Array.from({ length: doc.countPages() }, (_, i) =>
      doc
        .loadPage(i)
        .getLinks()
        .map((l) => ({ uri: l.getURI(), external: l.isExternal(), target: l.isExternal() ? undefined : doc.resolveLink(l), bounds: [...l.getBounds()] })),
    ),
  );
}

/** Bookmarks as `{ title, page (0-based), children }`. */
export function outline(pdf: Buffer): Array<{ title?: string; page?: number; children: unknown[] }> {
  type Item = { title?: string; page?: number; down?: Item[] };
  const map = (items: Item[] | null | undefined): Array<{ title?: string; page?: number; children: unknown[] }> =>
    (items ?? []).map((i) => ({ title: i.title, page: i.page, children: map(i.down) }));
  return open(pdf, (doc) => map(doc.loadOutline() as Item[] | null));
}
