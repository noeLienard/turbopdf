// TurboPDF engine in WebAssembly — Node and browsers, no dependency.
//
//   import { load } from './turbopdf.mjs';
//   const engine = await load(wasmBytes);            // ArrayBuffer / Uint8Array / Response / URL
//   const { pdf, pages, warnings, previews } = engine.render(tree, { previews: 2 });
//
// `file` sources are not available here (no filesystem): use `{ data: base64 }`.

const decoder = new TextDecoder();
const encoder = new TextEncoder();

async function compile(source) {
  if (source instanceof WebAssembly.Module) return source;
  if (typeof Response !== 'undefined' && source instanceof Response) return WebAssembly.compileStreaming(source);
  if (source instanceof URL || typeof source === 'string') {
    if (typeof process !== 'undefined' && process.versions?.node && !/^https?:/.test(String(source))) {
      const { readFile } = await import('node:fs/promises');
      return WebAssembly.compile(await readFile(source));
    }
    return WebAssembly.compileStreaming(fetch(source));
  }
  return WebAssembly.compile(source);
}

export class TurboPdfWasmError extends Error {
  constructor({ code, message, details }) {
    super(message);
    this.name = 'TurboPdfWasmError';
    this.code = code;
    this.details = details;
  }
}

export async function load(source = new URL('../target/wasm32-unknown-unknown/release/turbopdf_wasm.wasm', import.meta.url)) {
  const module = await compile(source);
  let instance = await WebAssembly.instantiate(module, {});

  function render(tree, options = {}) {
    const json = encoder.encode(typeof tree === 'string' ? tree : JSON.stringify(tree));
    const x = instance.exports;
    const ptr = x.turbopdf_alloc(json.length);
    new Uint8Array(x.memory.buffer, ptr, json.length).set(json);
    let status;
    try {
      status = x.turbopdf_render(ptr, json.length, options.previews ?? 0);
    } catch (e) {
      // a trap (panic) leaves the instance unusable: start a fresh one for the next call
      instance = new WebAssembly.Instance(module, {});
      throw new TurboPdfWasmError({ code: 'render_failed', message: `The engine crashed: ${e.message}` });
    } finally {
      try {
        x.turbopdf_free(ptr, json.length);
      } catch {}
    }
    const out = new Uint8Array(x.memory.buffer, x.turbopdf_output_ptr(), x.turbopdf_output_len()).slice();
    if (status !== 0) throw new TurboPdfWasmError(JSON.parse(decoder.decode(out)));

    const view = new DataView(out.buffer);
    let at = 0;
    const u32 = () => ((at += 4), view.getUint32(at - 4, true));
    const bytes = () => {
      const n = u32();
      return out.subarray((at += n) - n, at);
    };
    const pages = u32();
    const warnings = JSON.parse(decoder.decode(bytes()));
    const pdf = bytes();
    const previews = Array.from({ length: u32() }, bytes);
    return { pdf, pages, warnings, previews };
  }

  return { render };
}
