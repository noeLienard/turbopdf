// The engine compiled to WebAssembly, behind the same interface as the native module. Used on any OS or
// CPU without a prebuilt native binary (and with TURBOPDF_ENGINE=wasm): same Rust code, same bytes out.
// Renders run in a worker thread, so they never block the event loop. The engine itself is
// `wasm/engine.cjs` (plain JavaScript, shipped next to `turbopdf.wasm`).
import { createRequire } from 'node:module';
import { join } from 'node:path';
import type { Worker as WorkerType } from 'node:worker_threads';
import type { NativeDocument, NativeEngine } from './native';

interface Handle {
  id: number;
  generation: number;
}

interface Core {
  render(tree: string, options?: { baseDir?: string; buffers?: Uint8Array[] }): { handle: Handle; pages: number; warnings: string; pdf: Uint8Array };
  png(handle: Handle, scale: number): Uint8Array[];
  html(handle: Handle): { style: string; pages: { width: number; height: number; html: string }[] };
  release(handle: Handle): void;
  convertHtml(html: string, options?: { baseDir?: string; css?: string; header?: string; footer?: string }): { parts: string; buffers: Uint8Array[] };
  htmlNodes(html: string): string;
  version(): string;
}

const toBuffer = (u8: Uint8Array) => Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength);

/** `dir` holds `engine.cjs` and `turbopdf.wasm`. */
export function wasmEngine(dir: string, wasm: Uint8Array<ArrayBuffer>): NativeEngine {
  const script = join(dir, 'engine.cjs');
  const module = new WebAssembly.Module(wasm);
  const load = createRequire(script);
  const { core } = load(script) as { core(m: WebAssembly.Module): Core };
  // loaded at run time, not imported: bundlers (Turbopack, webpack) treat `new Worker(path)` from a static
  // import as a worker entry to bundle, and fail on a path only known at run time
  const { Worker } = load('node:worker_threads') as typeof import('node:worker_threads');

  // the sync call (`htmlNodes`, for raw HTML inside Markdown) runs here; it is small and has no files
  let local: Core | undefined;
  const here = () => (local ??= core(module));

  let worker: WorkerType | undefined;
  let next = 0;
  const pending = new Map<number, { resolve(v: unknown): void; reject(e: Error): void }>();

  function call<T>(op: keyof Core, ...args: unknown[]): Promise<T> {
    if (!worker) {
      worker = new Worker(script, { workerData: { turbopdf: module } });
      worker.on('message', ({ id, result, error }: { id: number; result?: unknown; error?: string }) => {
        const p = pending.get(id);
        pending.delete(id);
        if (pending.size === 0) worker?.unref();
        if (error !== undefined) p?.reject(new Error(error));
        else p?.resolve(result);
      });
      worker.on('error', (e: Error) => {
        for (const p of pending.values()) p.reject(new Error(JSON.stringify({ code: 'render_failed', message: `The engine crashed: ${e.message}` })));
        pending.clear();
        worker = undefined;
      });
    }
    const id = next++;
    // keep the process alive only while a render is in flight
    worker.ref();
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      worker!.postMessage({ id, op, args });
    });
  }

  const released = new FinalizationRegistry<Handle>((handle) => {
    if (worker) worker.postMessage({ id: -1, op: 'release', args: [handle] });
  });

  return {
    async render(tree, options = {}) {
      const r = await call<ReturnType<Core['render']>>('render', tree, options);
      let pdf: Buffer | undefined = toBuffer(r.pdf);
      const doc: NativeDocument = {
        pages: r.pages,
        warnings: r.warnings,
        takePdf() {
          if (!pdf) throw new Error(JSON.stringify({ code: 'render_failed', message: 'the PDF was already taken' }));
          const out = pdf;
          pdf = undefined;
          return out;
        },
        png: async (scale = 2) => (await call<Uint8Array[]>('png', r.handle, scale)).map(toBuffer),
        html: () => call('html', r.handle),
      };
      released.register(doc, r.handle);
      return doc;
    },
    async convertHtml(html, options = {}) {
      const r = await call<ReturnType<Core['convertHtml']>>('convertHtml', html, options);
      return { parts: r.parts, buffers: r.buffers.map(toBuffer) };
    },
    htmlNodes: (html) => here().htmlNodes(html),
    version: () => here().version(),
  };
}
