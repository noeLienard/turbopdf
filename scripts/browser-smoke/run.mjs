// `turbopdf/edge` in a real browser (Chromium): the PDF is made on the device. Checks that rendering sends no
// request (only the engine and the app's font are downloaded beforehand) and that the bytes are identical
// to the native engine's. Run after `npm run build` and `npm run build:wasm`.
//
//   node scripts/browser-smoke/run.mjs
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const font = join(root, 'test', 'fixtures', 'pacifico', 'pacifico-latin-400-normal.woff2');
const html = `<style>h1{color:#1d4ed8} td{padding:4px} tr:nth-child(even){background:#f3f4f6}</style>
<h1>Invoice 42 ✔</h1><p style="font-family:Script">Thank you</p>
<table>${Array.from({ length: 40 }, (_, i) => `<tr><td>Item ${i + 1}</td><td>${i * 7},00 €</td></tr>`).join('')}</table>
<img src="qr:https://example.com/pay/42" width="80" alt="Pay online">`;

// the page: the public edge API bundled as an app's bundler would
const out = mkdtempSync(join(tmpdir(), 'turbopdf-browser-'));
const { build } = await import(pathToFileURL(join(root, 'node_modules', 'esbuild', 'lib', 'main.js')).href);
await build({ entryPoints: [join(root, 'scripts', 'browser-smoke', 'page.mjs')], bundle: true, format: 'esm', platform: 'browser', outfile: join(out, 'bundle.js'), logLevel: 'warning' });

const files = {
  '/': ['text/html', '<!doctype html><meta charset="utf-8"><script type="module" src="/bundle.js"></script>'],
  '/bundle.js': ['text/javascript', readFileSync(join(out, 'bundle.js'))],
  '/turbopdf.wasm': ['application/wasm', readFileSync(join(root, 'wasm', 'turbopdf.wasm'))],
  '/font.woff2': ['font/woff2', readFileSync(font)],
};
const server = createServer((req, res) => {
  const f = files[req.url];
  if (!f) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': f[0] }).end(f[1]);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${server.address().port}`;

// Playwright, installed apart: adding it to package.json makes npm rewrite the lockfile (npm/cli#4828)
const tools = mkdtempSync(join(tmpdir(), 'turbopdf-playwright-'));
const sh = { stdio: 'inherit', shell: process.platform === 'win32' };
execFileSync('npm', ['install', '--prefix', tools, '--no-save', '--no-audit', '--no-fund', '--silent', 'playwright@1.58.2'], sh);
execFileSync(join(tools, 'node_modules', '.bin', 'playwright'), ['install', ...(process.platform === 'linux' ? ['--with-deps'] : []), 'chromium'], sh);
const { chromium } = await import(pathToFileURL(join(tools, 'node_modules', 'playwright', 'index.mjs')).href);

const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.error('page error:', e));
  await page.goto(origin);
  await page.waitForFunction(() => typeof window.prepare === 'function');
  await page.evaluate((u) => window.prepare(u), `${origin}/font.woff2`);

  const requests = [];
  page.on('request', (r) => requests.push(r.url()));
  const t0 = Date.now();
  const got = await page.evaluate((h) => window.render(h), html);
  const ms = Date.now() - t0;
  const browserPdf = Buffer.from(got.base64, 'base64');

  // the native engine, from the same inputs
  process.env.TURBOPDF_ENGINE = 'native';
  const { default: turbopdf } = await import(pathToFileURL(join(root, 'dist', 'index.js')).href);
  const nativePdf = (await turbopdf.create({ html, fonts: [{ family: 'Script', data: readFileSync(font) }], title: 'Browser smoke', lang: 'en' })).toBuffer();

  const sha = (b) => createHash('sha256').update(b).digest('hex').slice(0, 16);
  console.log(`browser: ${got.pages} page(s), ${browserPdf.length} bytes in ${ms} ms, sha ${sha(browserPdf)}; native: sha ${sha(nativePdf)}`);
  console.log(`requests while rendering: ${requests.length ? requests.join(', ') : 'none'}`);
  const problems = [
    browserPdf.subarray(0, 5).toString() !== '%PDF-' && 'not a PDF',
    got.warnings.length && `warnings: ${JSON.stringify(got.warnings)}`,
    requests.length && 'rendering used the network',
    !browserPdf.equals(nativePdf) && 'the browser PDF differs from the native one',
  ].filter(Boolean);
  if (problems.length) throw new Error(problems.join('; '));
  console.log('ok: made in the browser, offline, byte-identical to the native engine');
} finally {
  await browser.close();
  server.close();
}
