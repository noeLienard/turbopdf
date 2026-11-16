// `create()` for Node, Bun and Deno: the shared core plus files and streams.
import { Readable } from 'node:stream';
import { EdgePdfImpl, render } from './core';
import { asBuffer, nodePng, nodePreview, writeTo } from './preview';
import type { CreateOptions, Pdf, PngPage, Preview, ReactElementLike } from './types';
import './node-host';

class PdfImpl extends EdgePdfImpl<PngPage, Preview> implements Pdf {
  toBuffer(): Buffer {
    return asBuffer(this.bytes);
  }

  toStream(): Readable {
    return Readable.from([this.toBuffer()]);
  }

  save(path: string): Promise<void> {
    return writeTo(path, this.bytes);
  }
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
  return new PdfImpl(await render(input), nodePng, nodePreview);
}
