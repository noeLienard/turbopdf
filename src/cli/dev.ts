import { spawn } from 'node:child_process';
import { existsSync, readFileSync, watch, writeFileSync, mkdtempSync } from 'node:fs';
import { createServer, type ServerResponse } from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { create } from '../index';
import { PapierError } from '../errors';
import type { CreateOptions, Pdf, PngPage, Warning } from '../types';
import { UI_HTML } from './ui';

type Extra = Partial<Omit<CreateOptions, 'html' | 'react' | 'markdown' | 'tree'>>;

const USAGE = `Usage:
  papier dev <file> [--port 4000] [--no-open]

  <file>  a React component (.tsx/.jsx/.ts/.js, default export), an .html file or a .md file

Live preview of the real PDF, page by page. Saves in the file refresh the preview.`;

/** Nearest tsconfig.json going up from `dir`, if any. */
function findTsconfig(dir: string): string | undefined {
  for (let d = dir; ; d = dirname(d)) {
    const candidate = join(d, 'tsconfig.json');
    if (existsSync(candidate)) return candidate;
    if (dirname(d) === d) return undefined;
  }
}

/** A tsconfig that forces the automatic JSX runtime, extending the project's one if present. */
function jsxTsconfig(file: string): string {
  const dir = mkdtempSync(join(tmpdir(), 'papier-'));
  const base = findTsconfig(dirname(file));
  const path = join(dir, 'tsconfig.json');
  writeFileSync(path, JSON.stringify({ ...(base ? { extends: base } : {}), compilerOptions: { jsx: 'react-jsx' } }));
  return path;
}

let revCounter = 0;

/** Turn the user's file into options for `create()`. */
async function loadSource(file: string, tsconfig: () => string): Promise<CreateOptions> {
  const ext = extname(file).toLowerCase();
  if (ext === '.md' || ext === '.markdown') return { markdown: readFileSync(file, 'utf8') };
  if (ext === '.html' || ext === '.htm') return { html: readFileSync(file, 'utf8'), baseDir: dirname(file) }; // relative images / stylesheets keep working

  if (!['.tsx', '.jsx', '.ts', '.js', '.mjs', '.cjs'].includes(ext)) {
    throw new PapierError('invalid_source', `Unsupported file type "${ext}". Use a .tsx/.jsx component, an .html file or a .md file.`);
  }

  const { tsImport } = await import('tsx/esm/api');
  const mod = (await tsImport(`${pathToFileURL(file).href}?v=${++revCounter}`, { parentURL: import.meta.url, tsconfig: tsconfig() })) as {
    default?: unknown;
    options?: Extra;
  };
  const exported = mod.default;
  if (exported === undefined) throw new PapierError('invalid_source', `${basename(file)} has no default export. Use: export default function Invoice() { return <h1>…</h1>; }`);

  // resolve React from the user's project, not from papier
  const req = createRequire(pathToFileURL(file).href);
  let element: unknown = exported;
  let renderToStaticMarkup: (el: unknown) => string;
  try {
    const React = (await import(pathToFileURL(req.resolve('react')).href)) as { default?: { createElement: (c: unknown) => unknown }; createElement?: (c: unknown) => unknown };
    const createElement = React.default?.createElement ?? React.createElement!;
    if (typeof exported === 'function') element = createElement(exported);
    const server = (await import(pathToFileURL(req.resolve('react-dom/server')).href)) as { default?: { renderToStaticMarkup: typeof renderToStaticMarkup }; renderToStaticMarkup?: typeof renderToStaticMarkup };
    renderToStaticMarkup = server.default?.renderToStaticMarkup ?? server.renderToStaticMarkup!;
  } catch (cause) {
    throw new PapierError('react_not_installed', 'Could not load "react" and "react-dom" from your project. Run: npm install react react-dom', { cause });
  }
  let html: string;
  try {
    html = renderToStaticMarkup(element);
  } catch (cause) {
    throw new PapierError('render_failed', `Your component threw while rendering: ${(cause as Error).message}`, { cause });
  }
  return { baseDir: dirname(file), ...(mod.options ?? {}), html } as CreateOptions;
}

interface State {
  rev: number;
  status: 'rendering' | 'ready' | 'error';
  error: string | null;
  warnings: Warning[];
  pdf: Pdf | null;
  pages: PngPage[];
}

export async function dev(argv: string[]): Promise<void> {
  const args = argv.filter((a) => !a.startsWith('--'));
  const flag = (name: string) => argv.includes(`--${name}`);
  const portIdx = argv.indexOf('--port');
  const port = portIdx >= 0 ? Number(argv[portIdx + 1]) : 4000;

  if (!args[0] || flag('help')) {
    console.log(USAGE);
    process.exit(args[0] || flag('help') ? 0 : 1);
  }
  const file = resolve(args[0]);
  if (!existsSync(file)) {
    console.error(`File not found: ${args[0]}`);
    process.exit(1);
  }

  let tsconfigPath: string | undefined;
  const tsconfig = () => (tsconfigPath ??= jsxTsconfig(file));

  const state: State = { rev: 0, status: 'rendering', error: null, warnings: [], pdf: null, pages: [] };
  const clients = new Set<ServerResponse>();
  const notify = () => clients.forEach((c) => c.write(`data: ${state.rev}\n\n`));

  let running = false;
  let queued = false;
  async function renderNow(): Promise<void> {
    if (running) { queued = true; return; }
    running = true;
    state.status = 'rendering';
    notify();
    const t0 = Date.now();
    try {
      const pdf = await create(await loadSource(file, tsconfig));
      state.pdf = pdf;
      state.pages = await pdf.toPng();
      state.warnings = pdf.warnings;
      state.error = null;
      state.status = 'ready';
      console.log(`✓ ${pdf.pages} page${pdf.pages > 1 ? 's' : ''}${pdf.warnings.length ? `, ${pdf.warnings.length} warning(s)` : ''} (${Date.now() - t0}ms)`);
    } catch (e) {
      state.error = e instanceof PapierError ? `${e.code}: ${e.message}` : String((e as Error)?.message ?? e);
      state.status = 'error';
      console.error(`✗ ${state.error}`);
    }
    state.rev++;
    running = false;
    notify();
    if (queued) { queued = false; void renderNow(); }
  }

  const server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    if (url.pathname === '/') {
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' }).end(UI_HTML);
    } else if (url.pathname === '/events') {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' });
      res.write('retry: 1000\n\n');
      clients.add(res);
      req.on('close', () => clients.delete(res));
    } else if (url.pathname === '/state') {
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }).end(
        JSON.stringify({ file: basename(file), rev: state.rev, status: state.status, error: state.error, warnings: state.warnings, count: state.pages.length, hasPdf: !!state.pdf }),
      );
    } else if (url.pathname === '/download.pdf' && state.pdf) {
      const name = `${basename(file, extname(file))}.pdf`;
      res.writeHead(200, { 'Content-Type': 'application/pdf', 'Content-Disposition': `attachment; filename="${name}"` }).end(state.pdf.toBuffer());
    } else {
      const m = /^\/page\/(\d+)\.png$/.exec(url.pathname);
      const page = m ? state.pages[Number(m[1]) - 1] : undefined;
      if (page) res.writeHead(200, { 'Content-Type': 'image/png', 'Cache-Control': 'no-store' }).end(page.toBuffer());
      else res.writeHead(404).end('Not found');
    }
  });

  await new Promise<void>((ok, fail) => {
    server.once('error', fail);
    server.listen(port, '127.0.0.1', ok);
  }).catch((e: NodeJS.ErrnoException) => {
    console.error(e.code === 'EADDRINUSE' ? `Port ${port} is already in use. Try: papier dev ${args[0]} --port ${port + 1}` : e.message);
    process.exit(1);
  });

  const url = `http://localhost:${port}`;
  console.log(`papier dev — ${basename(file)}\n  preview: ${url}\n  Ctrl+C to stop`);

  // watch the file's folder (editors often replace files instead of writing them)
  let timer: NodeJS.Timeout | undefined;
  watch(dirname(file), { recursive: process.platform !== 'linux' }, (_event, name) => {
    if (!name || /(^|[\\/])(node_modules|\.git|dist)([\\/]|$)/.test(String(name))) return;
    clearTimeout(timer);
    timer = setTimeout(() => void renderNow(), 150);
  });

  if (!flag('no-open')) {
    const opener = process.platform === 'darwin' ? ['open', [url]] : process.platform === 'win32' ? ['cmd', ['/c', 'start', url]] : ['xdg-open', [url]];
    spawn(opener[0] as string, opener[1] as string[], { stdio: 'ignore', detached: true }).on('error', () => {}).unref();
  }

  const stop = () => process.exit(0);
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);

  await renderNow();
}

// executed as `papier dev …`
const [command, ...rest] = process.argv.slice(2);
if (command === 'install') {
  console.log('Nothing to install: Papier renders without a browser.');
} else if (command === 'dev') {
  dev(rest).catch((e) => { console.error(e); process.exit(1); });
} else {
  console.log(command ? `Unknown command "${command}".\n\n${USAGE}` : USAGE);
  process.exit(command ? 1 : 0);
}
