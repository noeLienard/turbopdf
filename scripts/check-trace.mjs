// Serverless platforms (Vercel, Netlify, Next.js `output: 'standalone'`) copy only the files a tracer sees
// used. Check that @vercel/nft finds every prebuilt engine package, the WebAssembly engine and its worker
// from both builds, so a deployed app never ends up without its engine. Run after `npm run build`.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

// the tracer Vercel uses, installed apart: adding it to package.json makes npm rewrite the lockfile
// without other platforms' optional binaries (npm/cli#4828)
const NFT = '@vercel/nft@1.11.4';
const tools = mkdtempSync(join(tmpdir(), 'turbopdf-nft-'));
execFileSync('npm', ['install', '--prefix', tools, '--no-save', '--no-audit', '--no-fund', '--silent', NFT], {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
const { nodeFileTrace } = await import(pathToFileURL(join(tools, 'node_modules', '@vercel', 'nft', 'out', 'index.js')).href);

// the engine packages of scripts/package-platforms.mjs (not imported: it packs when loaded)
const TARGETS = Object.fromEntries(
  ['darwin-arm64', 'darwin-x64', 'linux-x64-gnu', 'linux-arm64-gnu', 'linux-x64-musl', 'win32-x64-msvc'].map((t) => [t, true]),
);
const root = new URL('..', import.meta.url).pathname;
const app = mkdtempSync(join(tmpdir(), 'turbopdf-trace-'));
const pkg = join(app, 'node_modules', 'turbopdf');
mkdirSync(pkg, { recursive: true });
cpSync(join(root, 'dist'), join(pkg, 'dist'), { recursive: true });
cpSync(join(root, 'package.json'), join(pkg, 'package.json'));
mkdirSync(join(pkg, 'wasm'));
writeFileSync(join(pkg, 'wasm', 'turbopdf.wasm'), ''); // stand-in: the tracer only needs the path
// a stand-in for each prebuilt package: only its layout matters to the tracer
for (const target of Object.keys(TARGETS)) {
  const dir = join(app, 'node_modules', '@turbopdf', `engine-${target}`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: `@turbopdf/engine-${target}`, main: 'turbopdf.node' }));
  writeFileSync(join(dir, 'turbopdf.node'), '');
}
writeFileSync(join(app, 'app.mjs'), "import turbopdf from 'turbopdf';\nawait turbopdf.create('<p>x</p>');\n");
writeFileSync(join(app, 'app.cjs'), "const turbopdf = require('turbopdf');\nturbopdf.create('<p>x</p>');\n");

let failed = false;
for (const file of ['app.mjs', 'app.cjs']) {
  const { fileList } = await nodeFileTrace([join(app, file)], { base: app });
  const wanted = [
    ...Object.keys(TARGETS).map((t) => `node_modules/@turbopdf/engine-${t}/turbopdf.node`),
    'node_modules/turbopdf/wasm/turbopdf.wasm',
    'node_modules/turbopdf/dist/wasm-worker.cjs',
  ];
  const missing = wanted.filter((f) => !fileList.has(f));
  console.log(`${file}: ${missing.length ? `NOT traced: ${missing.join(', ')}` : `all ${Object.keys(TARGETS).length} engines, the wasm engine and its worker traced`}`);
  if (missing.length) failed = true;
}
if (failed) {
  console.error('A file tracer (Vercel, Netlify) would deploy without an engine: keep the literal requires and paths of src/rust/native.ts.');
  process.exit(1);
}
