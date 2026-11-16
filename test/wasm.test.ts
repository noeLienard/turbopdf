// The WebAssembly engine (fallback where no native binary exists) must give the same bytes as the native one.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import type { NativeEngine } from '../src/rust/engine-api';
import { wasmEngine } from '../src/rust/wasm';

const root = join(__dirname, '..');
const nativeFile = join(root, 'native', `turbopdf.${process.platform}-${process.arch}.node`);
const wasmFile = join(root, 'wasm', 'turbopdf.wasm');
const both = existsSync(nativeFile) && existsSync(wasmFile);

describe.skipIf(!both)('wasm engine', () => {
  const native = (both ? createRequire(__filename)(nativeFile) : undefined) as NativeEngine;
  const wasm = (both ? wasmEngine(readFileSync(wasmFile), join(root, 'src', 'rust')) : undefined) as NativeEngine;

  it('converts HTML exactly like the native engine', async () => {
    const html = readFileSync(join(root, 'examples/showcase/report.html'), 'utf8');
    const options = { baseDir: join(root, 'examples/showcase') };
    const [a, b] = await Promise.all([native.convertHtml(html, options), wasm.convertHtml(html, options)]);
    expect(b.parts).toBe(a.parts);
    expect(b.buffers.length).toBe(a.buffers.length);
  });

  it('writes the same PDF, PNG and HTML preview bytes for every engine fixture', async () => {
    const dir = join(root, 'engine/fixtures');
    // fixtures with `file` sources need assets that are not in the repo
    const trees = readdirSync(dir).filter((f) => f.endsWith('.json') && !readFileSync(join(dir, f), 'utf8').includes('"file"'));
    expect(trees.length).toBeGreaterThanOrEqual(4);
    for (const f of trees) {
      const tree = readFileSync(join(dir, f), 'utf8');
      const options = { baseDir: dir };
      const [a, b] = await Promise.all([native.render(tree, options), wasm.render(tree, options)]);
      expect(b.pages, f).toBe(a.pages);
      expect(b.warnings, f).toBe(a.warnings);
      expect(Buffer.from(b.takePdf()).equals(a.takePdf()), f).toBe(true);
      const [pa, pb] = await Promise.all([a.png(1), b.png(1)]);
      expect(pb.every((x, i) => Buffer.from(x).equals(pa[i]!)), f).toBe(true);
      expect(await b.html(), f).toEqual(await a.html());
    }
  });

  it('reads files only inside baseDir, with the native messages', async () => {
    const tree = (file: string) => JSON.stringify({ version: 1, type: 'document', fonts: [{ family: 'X', weight: 400, source: { file } }], children: [{ type: 'page', children: [] }] });
    for (const file of ['../package.json', 'missing.ttf']) {
      const errors = await Promise.all(
        [native, wasm].map((e) => e.render(tree(file), { baseDir: join(root, 'test') }).then(() => '', (err: Error) => err.message)),
      );
      expect(errors[1]).toBe(errors[0]);
      expect(errors[0]!).toContain('font_failed');
    }
  });

  it('reports its version', () => {
    expect(wasm.version()).toMatch(/wasm/);
  });
});
