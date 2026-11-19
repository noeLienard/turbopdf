// Browsers refuse `new WebAssembly.Instance` on the main thread for a module over 8 MB (Chrome throws a
// RangeError): the edge engine must then create its instance asynchronously.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

const wasmFile = join(__dirname, '..', 'wasm', 'turbopdf.wasm');

describe.skipIf(!existsSync(wasmFile))('turbopdf/edge in a browser main thread', () => {
  afterEach(() => vi.restoreAllMocks());

  it('falls back to WebAssembly.instantiate when the synchronous Instance is refused', async () => {
    const sync = vi.spyOn(WebAssembly, 'Instance').mockImplementation(function () {
      throw new RangeError('WebAssembly.Instance is disallowed on the main thread, if the buffer size is larger than 8MB.');
    });
    const instantiate = vi.spyOn(WebAssembly, 'instantiate');
    vi.resetModules();
    const { create, init } = await import('../src/edge');
    await init(readFileSync(wasmFile));
    const pdf = await create({ html: '<h1>Hello</h1>' });
    expect(new TextDecoder().decode(pdf.toUint8Array().subarray(0, 5))).toBe('%PDF-');
    expect((await pdf.toPng({ scale: 1 })).length).toBe(1);
    expect(sync).toHaveBeenCalled();
    expect(instantiate).toHaveBeenCalled();
  });
});
