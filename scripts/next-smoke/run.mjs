// A Next.js app with `output: "standalone"` and a route handler that renders a PDF, built from the packed
// tarballs with no tracing config (no `outputFileTracingIncludes`). Checks that the standalone output carries
// the native engine, the WebAssembly engine and its worker, then runs the server and fetches a PDF rendered
// by the native engine: in Amazon Linux 2023 with Docker (as on Vercel / AWS Lambda), or here with --no-docker.
//
//   node scripts/next-smoke/run.mjs <packed dir> [--no-docker]
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const packed = resolve(process.argv[2] ?? 'packed');
const docker = !process.argv.includes('--no-docker');
const target = (() => {
  const base = `${process.platform}-${process.arch}`;
  if (process.platform === 'win32') return `${base}-msvc`;
  if (process.platform !== 'linux') return base;
  return `${base}-${process.report.getReport().header.glibcVersionRuntime ? 'gnu' : 'musl'}`;
})();

const tgz = (prefix) => {
  const f = readdirSync(packed).find((n) => n.startsWith(prefix) && /-\d+\.\d+\.\d+.*\.tgz$/.test(n));
  if (!f) throw new Error(`no ${prefix}*.tgz in ${packed}`);
  return `file:${join(packed, f)}`;
};
// every engine package from the tarballs: never the registry, whose version may not exist yet
const engines = Object.fromEntries(
  readdirSync(packed)
    .filter((n) => n.startsWith('turbopdf-engine-'))
    .map((n) => [`@turbopdf/${n.replace(/^turbopdf-/, '').replace(/-\d+\.\d+\.\d+.*\.tgz$/, '')}`, `file:${join(packed, n)}`]),
);

const app = mkdtempSync(join(tmpdir(), 'turbopdf-next-'));
const write = (file, text) => {
  mkdirSync(dirname(join(app, file)), { recursive: true });
  writeFileSync(join(app, file), text);
};
write(
  'package.json',
  JSON.stringify({ private: true, type: 'module', dependencies: { next: '16', react: '19', 'react-dom': '19', turbopdf: tgz('turbopdf-') }, overrides: engines }, null, 2),
);
write('next.config.mjs', "export default { output: 'standalone', serverExternalPackages: ['turbopdf'] };\n");
write('app/layout.js', 'export default function Layout({ children }) { return <html><body>{children}</body></html>; }\n');
write(
  'app/api/pdf/route.js',
  `import turbopdf, { engineInfo } from 'turbopdf';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';

export async function GET() {
  const pdf = await turbopdf.create('<h1>Invoice 42 ✔</h1><p>Rendered by a Next.js route handler.</p>');
  const info = engineInfo();
  return pdf.toResponse({ headers: { 'x-turbopdf-engine': info.kind, 'x-turbopdf-fallback': info.fallbackReason ?? '' } });
}
`,
);

const run = (cmd, args) => execFileSync(cmd, args, { cwd: app, stdio: 'inherit', env: { ...process.env, NEXT_TELEMETRY_DISABLED: '1' } });
run('npm', ['install', '--no-audit', '--no-fund']);
run('npx', ['next', 'build']);

const standalone = join(app, '.next', 'standalone');
const wanted = [
  `node_modules/@turbopdf/engine-${target}/turbopdf.node`,
  'node_modules/turbopdf/wasm/turbopdf.wasm',
  'node_modules/turbopdf/dist/wasm-worker.cjs',
];
const missing = wanted.filter((f) => !existsSync(join(standalone, f)));
if (missing.length) throw new Error(`the standalone output lacks: ${missing.join(', ')}`);
console.log(`standalone output carries: ${wanted.join(', ')}`);

const port = 3123;
const server = docker
  ? spawn(
      'docker',
      [
        'run', '--rm', '-p', `${port}:${port}`, '-e', `PORT=${port}`, '-e', 'HOSTNAME=0.0.0.0',
        '-v', `${standalone}:/app:ro`, '-v', `${dirname(dirname(process.execPath))}:/opt/node:ro`, '-w', '/app',
        '--entrypoint', '/opt/node/bin/node', 'amazonlinux:2023', 'server.js',
      ],
      { stdio: 'inherit' },
    )
  : spawn(process.execPath, ['server.js'], { cwd: standalone, stdio: 'inherit', env: { ...process.env, PORT: String(port), HOSTNAME: '127.0.0.1' } });

try {
  let res;
  for (let i = 0; i < 120 && !res; i++) {
    res = await fetch(`http://127.0.0.1:${port}/api/pdf`).catch(() => undefined);
    if (!res) await new Promise((r) => setTimeout(r, 500));
  }
  if (!res) throw new Error('the server did not answer');
  const body = new Uint8Array(await res.arrayBuffer());
  const engine = res.headers.get('x-turbopdf-engine');
  console.log(`GET /api/pdf → ${res.status}, ${body.length} bytes, engine ${engine} ${res.headers.get('x-turbopdf-fallback')}`);
  if (res.status !== 200 || new TextDecoder().decode(body.subarray(0, 5)) !== '%PDF-') throw new Error('no PDF');
  if (engine !== 'native') throw new Error(`expected the native engine, got ${engine}`);
} finally {
  server.kill();
}
