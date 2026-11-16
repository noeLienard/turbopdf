// `turbopdf/edge`: the WebAssembly engine on the calling thread, with no Node API.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { create, init } from '../src/edge';

const wasmFile = join(__dirname, '..', 'wasm', 'turbopdf.wasm');
const wasm = existsSync(wasmFile) ? readFileSync(wasmFile) : new Uint8Array();
const FONT = readFileSync(join(__dirname, 'fixtures', 'codicon.ttf'));

// needs `npm run build:wasm` (the native-only release jobs skip it)
describe.skipIf(!existsSync(wasmFile))('turbopdf/edge', () => {
  it('asks for init() first', async () => {
    await expect(create('<p>x</p>')).rejects.toMatchObject({ code: 'engine_unavailable' });
  });

  it('renders HTML, Markdown and a tree, with the preview and PNGs', async () => {
    await init(wasm);
    await init(wasm); // reused
    const pdf = await create({ html: '<h1>Hello</h1><table><thead><tr><th>A</th></tr></thead>' + '<tr><td>row</td></tr>'.repeat(120) + '</table>', footer: '{page}/{pages}' });
    expect(new TextDecoder().decode(pdf.toUint8Array().subarray(0, 5))).toBe('%PDF-');
    expect(pdf.pages).toBeGreaterThan(1);
    expect(pdf.toResponse().headers.get('content-type')).toBe('application/pdf');
    const [page] = await pdf.toPng({ scale: 1 });
    expect(page!.width).toBeGreaterThan(500);
    expect(page!.toDataUrl()).toMatch(/^data:image\/png;base64,/);
    expect((await pdf.preview()).pages).toHaveLength(pdf.pages);
    expect((await create({ markdown: '# Report\n\n| a | b |\n|---|---|\n| 1 | 2 |' })).pages).toBe(1);
  });

  it('takes fonts as bytes and refuses files, which edge runtimes do not have', async () => {
    await init(wasm);
    const ok = await create({ html: '<p style="font-family:Icons">x</p>', fonts: [{ family: 'Icons', data: FONT }] });
    expect(ok.pages).toBe(1);
    const e = await create({ html: '<p>x</p>', fonts: [{ family: 'A', path: 'a.ttf' }] }).catch((x: unknown) => x);
    expect(e).toMatchObject({ code: 'font_failed' });
    expect((e as Error).message).toContain('no baseDir');
  });
});
