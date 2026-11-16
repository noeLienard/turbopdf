// Bytes helpers that work everywhere (Node, Bun, Deno, edge runtimes, browsers): no `Buffer` required.

const encoder = new TextEncoder();

export const utf8 = (s: string): Uint8Array => encoder.encode(s);

export function toBase64(b: Uint8Array): string {
  if (typeof Buffer !== 'undefined') return Buffer.from(b.buffer, b.byteOffset, b.byteLength).toString('base64');
  let s = '';
  for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
  return btoa(s);
}

/** Big-endian u32 at `at` (PNG header fields). */
export const u32be = (b: Uint8Array, at: number): number => new DataView(b.buffer, b.byteOffset, b.byteLength).getUint32(at);
