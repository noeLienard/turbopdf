// `turbopdf/edge`: TurboPDF for edge runtimes (Cloudflare Workers, Vercel Edge, Deno Deploy) and browsers.
// The same engine compiled to WebAssembly, on the calling thread, with no Node API. Load it once with
// `init()`, then `create()` as usual. There is no file system: pass fonts as bytes and images as data: URLs.
//
//   import { init, create } from 'turbopdf/edge';
//   import wasm from 'turbopdf/turbopdf.wasm';     // Cloudflare Workers; elsewhere a URL or the bytes
//   await init(wasm);
//   const pdf = await create('<h1>Hello</h1>');
//   return pdf.toResponse();
import { EdgePdfImpl, render } from './core';
import { TurboPdfError } from './errors';
import { setHost } from './rust/provider';
import { core, inThread } from './rust/wasm-core';
import type { CreateOptions, EdgePdf, ReactElementLike } from './types';
import type { NativeEngine } from './rust/engine-api';

export { configure } from './queue';
export { TurboPdfError } from './errors';
export type { TurboPdfErrorCode } from './errors';
export type { Watermark } from './tree';
export type { CreateOptions, EdgePdf, EdgePngPage, EdgePreview, PngOptions, PreviewOptions, PreviewPage, ReactElementLike, ResponseOptions, Warning } from './types';

/** What `init()` accepts: the compiled module, its bytes, a `fetch` response, or a URL to fetch. */
export type WasmSource = WebAssembly.Module | ArrayBuffer | ArrayBufferView | Response | Promise<Response> | URL | string;

let ready: Promise<NativeEngine> | undefined;

async function compile(source: WasmSource): Promise<WebAssembly.Module> {
  if (source instanceof WebAssembly.Module) return source;
  if (typeof source === 'string' || source instanceof URL) source = fetch(source);
  if (source instanceof Promise || (typeof Response !== 'undefined' && source instanceof Response)) {
    const res = await source;
    if (!res.ok) throw new TurboPdfError('engine_unavailable', `Could not load the TurboPDF engine: HTTP ${res.status} for ${res.url}.`);
    return WebAssembly.compile(await res.arrayBuffer());
  }
  return WebAssembly.compile(source as BufferSource);
}

/** Load the engine (`turbopdf/turbopdf.wasm` of the package). Call it once; later calls reuse it. */
export function init(source: WasmSource): Promise<void> {
  ready ??= compile(source).then(
    (module) => {
      const engine = inThread(core(module));
      setHost({ engine: () => engine, baseDir: () => undefined });
      return engine;
    },
    (cause: unknown) => {
      ready = undefined;
      throw cause instanceof TurboPdfError ? cause : new TurboPdfError('engine_unavailable', `Could not load the TurboPDF engine: ${(cause as Error).message}`, { cause });
    },
  );
  return ready.then(() => undefined);
}

/** Create a PDF from HTML, a React element, Markdown or a document tree. Call {@link init} first. */
export async function create(html: string): Promise<EdgePdf>;
export async function create(element: ReactElementLike): Promise<EdgePdf>;
export async function create(options: CreateOptions): Promise<EdgePdf>;
export async function create(input: string | ReactElementLike | CreateOptions): Promise<EdgePdf> {
  if (!ready) throw new TurboPdfError('engine_unavailable', 'Call `init(wasm)` from `turbopdf/edge` before `create()`.');
  await ready;
  return new EdgePdfImpl(await render(input));
}

const turbopdf = { init, create };
export default turbopdf;
