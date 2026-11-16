// The WebAssembly engine for Node, Bun and Deno, used where no native binary exists (and with
// TURBOPDF_ENGINE=wasm): same Rust code, same bytes out. Renders run in a worker thread (dist/wasm-worker.cjs),
// so they never block the event loop; without the built worker (tests from source) they run on this thread.
import { existsSync } from 'node:fs';
import nodeModule from 'node:module';
import { join } from 'node:path';
import type { Worker as WorkerType } from 'node:worker_threads';
import type { NativeDocument, NativeEngine } from './engine-api';
import { nodeFiles } from './node-files';
import { core, type Core, type Handle, inThread } from './wasm-core';

function workerScript(here: string): string | undefined {
  return [process.env.TURBOPDF_WASM_WORKER, join(here, 'wasm-worker.cjs'), join(here, '..', '..', 'dist', 'wasm-worker.cjs')].find(
    (p): p is string => Boolean(p) && existsSync(p!),
  );
}

export function wasmEngine(wasm: Uint8Array<ArrayBuffer>, here: string): NativeEngine {
  const module = new WebAssembly.Module(wasm);
  // the sync call (`htmlNodes`, for raw HTML inside Markdown) runs here; it is small and reads no file
  let local: Core | undefined;
  const here_ = () => (local ??= core(module, nodeFiles));
  const script = workerScript(here);
  if (!script) return inThread(here_());

  // loaded at run time, not imported: bundlers (Turbopack, webpack) treat `new Worker(path)` from a static
  // import as a worker entry to bundle, and fail on a path only known at run time
  const { Worker } = nodeModule.createRequire(script)('node:worker_threads') as typeof import('node:worker_threads');
  let worker: WorkerType | undefined;
  let next = 0;
  const pending = new Map<number, { resolve(v: unknown): void; reject(e: Error): void }>();

  function call<T>(op: keyof Core, ...args: unknown[]): Promise<T> {
    if (!worker) {
      worker = new Worker(script!, { workerData: { turbopdf: module } });
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
    worker?.postMessage({ id: -1, op: 'release', args: [handle] });
  });

  return {
    async render(tree, options = {}) {
      const r = await call<ReturnType<Core['render']>>('render', tree, options);
      let pdf: Uint8Array | undefined = r.pdf;
      const doc: NativeDocument = {
        pages: r.pages,
        warnings: r.warnings,
        takePdf() {
          if (!pdf) throw new Error(JSON.stringify({ code: 'render_failed', message: 'the PDF was already taken' }));
          const out = pdf;
          pdf = undefined;
          return out;
        },
        png: (scale = 2) => call<Uint8Array[]>('png', r.handle, scale),
        html: () => call('html', r.handle),
      };
      released.register(doc, r.handle);
      return doc;
    },
    convertHtml: (html, options = {}) => call('convertHtml', html, options),
    htmlNodes: (html) => here_().htmlNodes(html),
    version: () => here_().version(),
  };
}
