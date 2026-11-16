// The engine compiled to WebAssembly, driven on the calling thread. No Node API: file access is injected
// (`Files`), so the same code runs in a Node worker (`wasm-worker.ts`), on edge runtimes and in browsers.

interface Exports {
  memory: WebAssembly.Memory;
  turbopdf_alloc(len: number): number;
  turbopdf_free(ptr: number, len: number): void;
  turbopdf_output_ptr(): number;
  turbopdf_output_len(): number;
  turbopdf_push_buffer(ptr: number, len: number): void;
  turbopdf_prepare(ptr: number, len: number, files: number): number;
  turbopdf_png(handle: number, scale: number): number;
  turbopdf_html(handle: number): number;
  turbopdf_release(handle: number): void;
  turbopdf_convert(ptr: number, len: number): number;
  turbopdf_html_nodes(ptr: number, len: number): number;
  turbopdf_version(): number;
}

/** How the host reads files: `base` checks and resolves a baseDir, `read` a path inside it. */
export interface Files {
  base(dir: string): string;
  read(base: string, path: string): Uint8Array;
}

export interface Handle {
  id: number;
  generation: number;
}

export interface Core {
  render(tree: string, options?: { baseDir?: string; buffers?: Uint8Array[] }): { handle: Handle; pages: number; warnings: string; pdf: Uint8Array };
  png(handle: Handle, scale: number): Uint8Array[];
  html(handle: Handle): { style: string; pages: { width: number; height: number; html: string }[] };
  release(handle: Handle): void;
  convertHtml(html: string, options?: { baseDir?: string; css?: string; header?: string; footer?: string }): { parts: string; buffers: Uint8Array[] };
  htmlNodes(html: string): string;
  version(): string;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Errors carry the native module's JSON message `{ code, message }`. */
const fail = (code: string, message: string) => new Error(JSON.stringify({ code, message }));

export function core(module: WebAssembly.Module, files?: Files): Core {
  let x!: Exports;
  /** baseDir of the call in progress: the engine asks for files during the call. */
  let base: string | undefined;
  /** Bumped when a crash replaces the instance: older handles are dead. */
  let generation = 0;

  const imports = {
    turbopdf: {
      host_read(ptr: number, len: number, out: number): number {
        const path = decoder.decode(new Uint8Array(x.memory.buffer, ptr, len));
        let status = 0;
        let data: Uint8Array;
        try {
          if (base === undefined) throw new Error(`${JSON.stringify(path)}: no baseDir was given, so files are not read`);
          if (!files) throw new Error(`${JSON.stringify(path)}: files are not available in this runtime. Pass the bytes instead (fonts: { data }, images: data: URLs)`);
          data = files.read(base, path);
        } catch (e) {
          status = 1;
          data = encoder.encode((e as Error).message);
        }
        const at = data.length ? x.turbopdf_alloc(data.length) : 0;
        // views are taken after the allocation: it may grow (and detach) the memory
        if (data.length) new Uint8Array(x.memory.buffer, at, data.length).set(data);
        new Uint32Array(x.memory.buffer, out, 2).set([at, data.length]);
        return status;
      },
    },
  };
  const start = () => {
    x = new WebAssembly.Instance(module, imports).exports as unknown as Exports;
    generation++;
  };
  start();

  /** Copy `data` in, run `call`, free, return the output (or throw the engine's error). */
  function run(data: Uint8Array | undefined, call: (ptr: number, len: number) => number): Uint8Array {
    // even empty input gets a real (non-null) pointer: Rust slices may not start at 0
    const len = data ? data.length : 0;
    const ptr = data ? x.turbopdf_alloc(len) : 0;
    if (data) new Uint8Array(x.memory.buffer, ptr, len).set(data);
    let status: number;
    try {
      status = call(ptr, len);
    } catch (e) {
      // a trap (panic) leaves the instance unusable: start a fresh one for the next call
      start();
      throw fail('render_failed', `The engine crashed: ${(e as Error).message}`);
    }
    if (data) x.turbopdf_free(ptr, len);
    const out = new Uint8Array(x.memory.buffer, x.turbopdf_output_ptr(), x.turbopdf_output_len()).slice();
    if (status !== 0) throw new Error(decoder.decode(out));
    return out;
  }

  function reader(out: Uint8Array) {
    const view = new DataView(out.buffer, out.byteOffset, out.byteLength);
    let at = 0;
    const u32 = () => ((at += 4), view.getUint32(at - 4, true));
    const bytes = () => {
      const n = u32();
      return out.subarray((at += n) - n, at);
    };
    // own copies: they can be transferred to another thread without a copy
    return { u32, bytes, list: () => Array.from({ length: u32() }, () => bytes().slice()) };
  }

  function within<T>(dir: string | undefined, f: () => T): T {
    if (dir !== undefined && files) {
      try {
        base = files.base(dir);
      } catch (e) {
        throw fail('invalid_option', `baseDir ${JSON.stringify(dir)}: ${(e as Error).message}`);
      }
    } else {
      base = dir;
    }
    try {
      return f();
    } finally {
      base = undefined;
    }
  }

  const live = (handle: Handle) => {
    if (handle.generation !== generation) throw fail('render_failed', 'the engine was restarted after a crash: create the document again');
    return handle.id;
  };

  return {
    render(tree, options = {}) {
      return within(options.baseDir, () => {
        for (const b of options.buffers ?? []) {
          // ownership moves to the engine: not freed here
          const ptr = x.turbopdf_alloc(b.length);
          new Uint8Array(x.memory.buffer, ptr, b.length).set(b);
          x.turbopdf_push_buffer(ptr, b.length);
        }
        const withFiles = options.baseDir === undefined ? 0 : 1;
        const r = reader(run(encoder.encode(tree), (p, l) => x.turbopdf_prepare(p, l, withFiles)));
        const handle = { id: r.u32(), generation };
        const pages = r.u32();
        const warnings = decoder.decode(r.bytes());
        return { handle, pages, warnings, pdf: r.bytes().slice() };
      });
    },
    png(handle, scale) {
      const id = live(handle);
      return reader(run(undefined, () => x.turbopdf_png(id, scale))).list();
    },
    html(handle) {
      const id = live(handle);
      return JSON.parse(decoder.decode(run(undefined, () => x.turbopdf_html(id))));
    },
    release(handle) {
      if (handle.generation === generation) x.turbopdf_release(handle.id);
    },
    convertHtml(html, options = {}) {
      return within(options.baseDir, () => {
        const request = { html, css: options.css, header: options.header, footer: options.footer, files: options.baseDir !== undefined };
        const r = reader(run(encoder.encode(JSON.stringify(request)), (p, l) => x.turbopdf_convert(p, l)));
        const parts = decoder.decode(r.bytes());
        return { parts, buffers: r.list() };
      });
    },
    htmlNodes(html) {
      return decoder.decode(run(encoder.encode(html), (p, l) => x.turbopdf_html_nodes(p, l)));
    },
    version() {
      return decoder.decode(run(undefined, () => x.turbopdf_version()));
    },
  };
}

/** The core as an async engine on this thread (edge runtimes, browsers, or Node without a worker). */
export function inThread(c: Core): import('./engine-api').NativeEngine {
  const released = typeof FinalizationRegistry === 'undefined' ? undefined : new FinalizationRegistry<Handle>((h) => c.release(h));
  return {
    async render(tree, options) {
      const r = c.render(tree, options);
      let pdf: Uint8Array | undefined = r.pdf;
      const doc = {
        pages: r.pages,
        warnings: r.warnings,
        takePdf() {
          if (!pdf) throw fail('render_failed', 'the PDF was already taken');
          const out = pdf;
          pdf = undefined;
          return out;
        },
        png: async (scale = 2) => c.png(r.handle, scale),
        html: async () => c.html(r.handle),
      };
      released?.register(doc, r.handle);
      return doc;
    },
    convertHtml: async (html, options) => c.convertHtml(html, options),
    htmlNodes: (html) => c.htmlNodes(html),
    version: () => c.version(),
  };
}
