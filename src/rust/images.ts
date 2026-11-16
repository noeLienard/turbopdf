// Image sources for the document tree: file paths, `data:` URLs and raw bytes, with their format.
import type { BinarySource, ImageFormat, ImageSource } from '../tree';
import { toBase64, utf8 } from '../bytes';

const MIME: Record<string, ImageFormat> = { png: 'png', jpeg: 'jpeg', jpg: 'jpeg', webp: 'webp', gif: 'gif', 'svg+xml': 'svg', svg: 'svg' };

/** Format from the first bytes (`undefined`: let the engine decide). */
export function sniff(bytes: Uint8Array): ImageFormat | undefined {
  const ascii = (from: number, to: number) => String.fromCharCode(...bytes.subarray(from, to));
  if (bytes[0] === 0x89 && bytes[1] === 0x50) return 'png';
  if (bytes[0] === 0xff && bytes[1] === 0xd8) return 'jpeg';
  if (ascii(0, 4) === 'RIFF' && ascii(8, 12) === 'WEBP') return 'webp';
  if (ascii(0, 4) === 'GIF8') return 'gif';
  const head = new TextDecoder().decode(bytes.subarray(0, 512)).replace(/^﻿/, '').trimStart();
  if (head.startsWith('<svg') || (/^<(\?xml|!--|!doctype)/i.test(head) && head.includes('<svg'))) return 'svg';
  return undefined;
}

/** A `src` string (path, file:// or data: URL) → tree source and format. Remote URLs are kept: the engine reports them. */
export function imageSource(src: string): { src: ImageSource; format?: ImageFormat } {
  // `qr:<text>`: a QR code drawn by the engine (vector), e.g. <img src="qr:https://exemple.fr">
  if (/^qr:/i.test(src.trim())) return { src: { qr: src.trim().slice(3) } };
  const data = /^data:image\/([a-z0-9.+-]+)((?:;[^,;]*)*?)(;base64)?,(.*)$/is.exec(src.trim());
  if (data) {
    const format = MIME[data[1]!.toLowerCase()];
    // `data:image/svg+xml;utf8,<svg …>` is URL-encoded text: the tree wants base64
    const payload = data[3] ? data[4]!.replace(/\s+/g, '') : toBase64(utf8(decodeURIComponent(data[4]!)));
    return { src: { data: payload }, format };
  }
  const ext = /\.([a-z]+)(?:[?#].*)?$/i.exec(src)?.[1]?.toLowerCase();
  return { src: { file: src }, format: ext ? MIME[ext] : undefined };
}

/** Raw bytes → tree source and format. */
export function imageBytes(bytes: Uint8Array): { src: BinarySource; format?: ImageFormat } {
  return { src: { data: toBase64(bytes) }, format: sniff(bytes) };
}

/** An inline `<svg>…</svg>` (its exact source text) → tree source. */
export function svgSource(markup: string): { src: BinarySource; format: ImageFormat } {
  const withNs = /\sxmlns\s*=/.test(markup.slice(0, markup.indexOf('>'))) ? markup : markup.replace(/^<svg\b/i, '<svg xmlns="http://www.w3.org/2000/svg"');
  return { src: { data: toBase64(utf8(withNs)) }, format: 'svg' };
}
