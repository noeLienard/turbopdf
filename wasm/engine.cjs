// TurboPDF engine in WebAssembly: the same calls as the native module (render → prepared document,
// convertHtml, htmlNodes, version), for any OS or CPU without a prebuilt binary.
//
// Two uses: `require('./engine.cjs').core(module)` runs it on the calling thread, and the file is also a
// worker script (`new Worker(file, { workerData: { turbopdf: module } })`) so that renders never block the
// host's event loop. Plain JavaScript on purpose: it is shipped as is, not bundled.
'use strict';
const { readFileSync, realpathSync } = require('node:fs');
const { isAbsolute, normalize, resolve, sep } = require('node:path');

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Errors carry the native module's JSON message `{ code, message }`. */
const fail = (code, message) => new Error(JSON.stringify({ code, message }));

/** Files inside `base` only: same rules and messages as the native loader. */
function readInside(base, path) {
  if (!isAbsolute(path)) {
    let depth = 0;
    for (const part of normalize(path).split(/[\\/]/)) {
      if (part === '..') depth--;
      else if (part && part !== '.') depth++;
      if (depth < 0) throw new Error(`${JSON.stringify(path)} is outside baseDir (${base})`);
    }
  }
  let real;
  try {
    real = realpathSync(resolve(base, path));
  } catch {
    throw new Error(`${JSON.stringify(path)} was not found in baseDir (${base})`);
  }
  // symbolic links are followed: the file itself must be inside too
  if (real !== base && !real.startsWith(base.endsWith(sep) ? base : base + sep)) {
    throw new Error(`${JSON.stringify(path)} is outside baseDir (${base})`);
  }
  try {
    return readFileSync(real);
  } catch (e) {
    throw new Error(`cannot read ${JSON.stringify(path)}: ${e.message}`);
  }
}

function canonicalBase(dir) {
  try {
    return realpathSync(dir);
  } catch (e) {
    throw fail('invalid_option', `baseDir ${JSON.stringify(dir)}: ${e.message}`);
  }
}

/**
 * The engine on this thread. Documents are handles: `render` returns `{ handle, pages, warnings, pdf }`,
 * then `png(handle, scale)`, `html(handle)` and `release(handle)`.
 */
function core(module) {
  let x;
  let base; // baseDir of the call in progress: the engine asks for files during the call
  let generation = 0; // bumped when a crash replaces the instance: older handles are dead

  const imports = {
    turbopdf: {
      host_read(ptr, len, out) {
        const path = decoder.decode(new Uint8Array(x.memory.buffer, ptr, len));
        let status = 0;
        let data;
        try {
          if (base === undefined) throw new Error(`${JSON.stringify(path)}: no baseDir was given, so files are not read`);
          data = readInside(base, path);
        } catch (e) {
          status = 1;
          data = encoder.encode(e.message);
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
    x = new WebAssembly.Instance(module, imports).exports;
    generation++;
  };
  start();

  /** Copy `data` in, run `call`, free, return the output (or throw the engine's error). */
  function run(data, call) {
    // even empty input gets a real (non-null) pointer: Rust slices may not start at 0
    const len = data ? data.length : 0;
    const ptr = data ? x.turbopdf_alloc(len) : 0;
    if (data) new Uint8Array(x.memory.buffer, ptr, len).set(data);
    let status;
    try {
      status = call(ptr, len);
    } catch (e) {
      // a trap (panic) leaves the instance unusable: start a fresh one for the next call
      start();
      throw fail('render_failed', `The engine crashed: ${e.message}`);
    }
    if (data) x.turbopdf_free(ptr, len);
    const out = new Uint8Array(x.memory.buffer, x.turbopdf_output_ptr(), x.turbopdf_output_len()).slice();
    if (status !== 0) throw new Error(decoder.decode(out));
    return out;
  }

  function reader(out) {
    const view = new DataView(out.buffer, out.byteOffset, out.byteLength);
    let at = 0;
    const u32 = () => ((at += 4), view.getUint32(at - 4, true));
    const bytes = () => {
      const n = u32();
      return out.subarray((at += n) - n, at);
    };
    return { u32, bytes, list: () => Array.from({ length: u32() }, () => bytes().slice()) };
  }

  function within(dir, f) {
    base = dir === undefined ? undefined : canonicalBase(dir);
    try {
      return f();
    } finally {
      base = undefined;
    }
  }

  const live = (handle) => {
    if (handle.generation !== generation) throw fail('render_failed', 'the engine was restarted after a crash: create the document again');
    return handle.id;
  };

  return {
    render(tree, options = {}) {
      return within(options.baseDir, () => {
        for (const b of options.buffers || []) {
          // ownership moves to the engine: not freed here
          const ptr = x.turbopdf_alloc(b.length);
          new Uint8Array(x.memory.buffer, ptr, b.length).set(b);
          x.turbopdf_push_buffer(ptr, b.length);
        }
        const files = options.baseDir === undefined ? 0 : 1;
        const r = reader(run(encoder.encode(tree), (p, l) => x.turbopdf_prepare(p, l, files)));
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

module.exports = { core };

// ------------------------------------------------------------------ as a worker
const threads = require('node:worker_threads');
if (!threads.isMainThread && threads.workerData && threads.workerData.turbopdf) {
  const engine = core(threads.workerData.turbopdf);
  threads.parentPort.on('message', ({ id, op, args }) => {
    try {
      const result = engine[op](...args);
      // PDF / PNG bytes move to the caller without a copy
      const transfer = [];
      const own = (u8) => {
        if (u8 instanceof Uint8Array && u8.byteOffset === 0 && u8.byteLength === u8.buffer.byteLength) transfer.push(u8.buffer);
      };
      if (op === 'render') own(result.pdf);
      if (op === 'png') result.forEach(own);
      if (op === 'convertHtml') result.buffers.forEach(own);
      threads.parentPort.postMessage({ id, result }, transfer);
    } catch (e) {
      threads.parentPort.postMessage({ id, error: e.message });
    }
  });
}
