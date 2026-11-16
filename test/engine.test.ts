// Which engine renders, and why the native one is not used when it fails to load.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import turbopdf, { engineInfo } from '../src/index';

const root = join(__dirname, '..');
const scratch = mkdtempSync(join(tmpdir(), 'turbopdf-engine-'));
// a "native module" that cannot load, like a binary built for a newer glibc
const bad = join(scratch, 'bad.node');
writeFileSync(bad, 'not a shared library');

/** Run `code` in a new process (the engine is chosen once per process), from the sources. */
function child(code: string, env: Record<string, string>) {
  const script = join(scratch, `child-${Math.random().toString(36).slice(2)}.mts`);
  writeFileSync(script, `import turbopdf, { engineInfo } from ${JSON.stringify(join(root, 'src/index.ts'))};\n${code}`);
  const r = spawnSync(process.execPath, ['--import', 'tsx', script], {
    cwd: root,
    env: { ...process.env, TURBOPDF_ENGINE: '', TURBOPDF_NO_FALLBACK_WARNING: '', ...env },
    encoding: 'utf8',
  });
  return { status: r.status, out: r.stdout.trim(), err: r.stderr };
}

describe('engine', () => {
  it('engineInfo() says which engine renders', () => {
    const info = engineInfo();
    expect(info.kind).toBe(process.env.TURBOPDF_ENGINE === 'wasm' ? 'wasm' : 'native');
    expect(info.target).toMatch(/^(darwin|linux|win32)-/);
    expect(info.version).toMatch(/^\d+\.\d+\.\d+/);
    expect(info.fallbackReason).toBeUndefined();
    expect(turbopdf.engineInfo).toBe(engineInfo);
  });

  it('warmup() runs once: later calls return the same promise', async () => {
    const a = turbopdf.warmup();
    expect(turbopdf.warmup()).toBe(a);
    await a;
  });

  // the release's native jobs test without the WebAssembly build: nothing to fall back to there
  it.skipIf(!existsSync(join(root, 'wasm', 'turbopdf.wasm')))('a native engine that fails to load: WebAssembly, with the loader message, and one warning', () => {
    const code = `const pdf = await turbopdf.create('<p>x</p>');
await turbopdf.create('<p>y</p>');
console.log(JSON.stringify({ info: engineInfo(), pdf: new TextDecoder().decode(pdf.toBuffer().subarray(0, 5)) }));`;
    const r = child(code, { TURBOPDF_NATIVE: bad });
    expect(r.status, r.err).toBe(0);
    const { info, pdf } = JSON.parse(r.out) as { info: { kind: string; fallbackReason: string }; pdf: string };
    expect(pdf).toBe('%PDF-');
    expect(info.kind).toBe('wasm');
    expect(info.fallbackReason).toContain('bad.node failed to load');
    expect(r.err.match(/\[turbopdf\] using the slower WebAssembly engine/g)).toHaveLength(1);
    expect(r.err).toContain('bad.node failed to load');
    // the warning can be turned off
    expect(child(code, { TURBOPDF_NATIVE: bad, TURBOPDF_NO_FALLBACK_WARNING: '1' }).err).not.toContain('[turbopdf]');
  });

  it('TURBOPDF_ENGINE=native: engine_unavailable carries the loader error as its cause', () => {
    const r = child(
      `try { await turbopdf.create('<p>x</p>'); } catch (e) { console.log(JSON.stringify({ code: e.code, message: e.message, cause: String(e.cause?.message ?? e.cause) })); }`,
      { TURBOPDF_NATIVE: bad, TURBOPDF_ENGINE: 'native' },
    );
    const e = JSON.parse(r.out) as { code: string; message: string; cause: string };
    expect(e.code).toBe('engine_unavailable');
    expect(e.message).toContain('bad.node failed to load');
    expect(e.cause.length).toBeGreaterThan(0);
  });
});
