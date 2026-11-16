// Node additions to the outputs: `save(path)` and `toBuffer()`.
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import type { EdgePngPage, EdgePreview, PngPage, Preview } from './types';

export async function writeTo(path: string, data: Uint8Array | string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, data);
}

export const asBuffer = (u8: Uint8Array): Buffer => (Buffer.isBuffer(u8) ? u8 : Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength));

export function nodePng(p: EdgePngPage): PngPage {
  return { ...p, toBuffer: () => asBuffer(p.toUint8Array()), save: (path) => writeTo(path, p.toUint8Array()) };
}

export function nodePreview(p: EdgePreview): Preview {
  return { ...p, save: (path) => writeTo(path, p.toHtml()) };
}
