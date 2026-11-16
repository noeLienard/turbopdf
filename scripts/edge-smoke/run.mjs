// Bundle scripts/edge-smoke/worker.mjs as Wrangler does and run it in workerd (Cloudflare's runtime) with
// Miniflare: no Node API, no file system. Usage (in a folder where turbopdf, esbuild and miniflare are
// installed): node run.mjs <path to worker.mjs>
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';
import { dirname, join } from 'node:path';

const entry = process.argv[2];
const out = join(dirname(entry), 'out', 'worker.js');
await build({
  entryPoints: [entry],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  conditions: ['workerd', 'worker', 'browser'],
  mainFields: ['module', 'main'],
  loader: { '.wasm': 'copy' },
  outfile: out,
  logLevel: 'warning',
});
const mf = new Miniflare({
  modules: true,
  scriptPath: out,
  modulesRules: [{ type: 'CompiledWasm', include: ['**/*.wasm'] }],
  compatibilityDate: '2025-01-01',
});
try {
  for (const q of ['kind=html', 'kind=markdown', 'kind=html&png']) {
    const res = await mf.dispatchFetch(`http://localhost/?${q}`);
    const body = new Uint8Array(await res.arrayBuffer());
    const head = new TextDecoder().decode(body.subarray(0, 5));
    if (res.status !== 200 || !(head === '%PDF-' || body[1] === 0x50)) throw new Error(`${q}: HTTP ${res.status} ${new TextDecoder().decode(body.subarray(0, 300))}`);
    console.log(`workerd ${q}: HTTP ${res.status}, ${res.headers.get('content-type')}, ${body.length} bytes`);
  }
} finally {
  await mf.dispose();
}
