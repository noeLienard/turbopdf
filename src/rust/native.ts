import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { TurboPdfError } from '../errors';
import type { NativeEngine } from './engine-api';
import { wasmEngine } from './wasm';

export { nativeError, type NativeDocument, type NativeEngine } from './engine-api';

let cached: NativeEngine | undefined;

const here = typeof __dirname === 'string' ? __dirname : dirname(fileURLToPath(import.meta.url));

/** Where the compiled engine may be: explicit override, next to the package, or a platform package. */
function candidates(): string[] {
  const file = `turbopdf.${process.platform}-${process.arch}.node`;
  return [
    process.env.TURBOPDF_NATIVE,
    join(here, '..', 'native', file), // dist/index.js → native/
    join(here, '..', '..', 'native', file), // src/rust/native.ts → native/ (tests)
  ].filter((p): p is string => Boolean(p));
}

/** The prebuilt package for this machine: `linux-x64-gnu`, `linux-x64-musl` (Alpine), `darwin-arm64`, `win32-x64-msvc`… */
export function platformTarget(): string {
  const base = `${process.platform}-${process.arch}`;
  if (process.platform === 'win32') return `${base}-msvc`;
  if (process.platform !== 'linux') return base;
  const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined;
  return `${base}-${report?.header?.glibcVersionRuntime ? 'gnu' : 'musl'}`;
}

/** `wasm/turbopdf.wasm` of the package. */
function wasmFile(): string | undefined {
  return [
    process.env.TURBOPDF_WASM,
    join(here, '..', 'wasm', 'turbopdf.wasm'), // dist/index.js → wasm/
    join(here, '..', '..', 'wasm', 'turbopdf.wasm'), // src/rust/native.ts → wasm/ (tests)
  ].find((p): p is string => Boolean(p) && existsSync(p!));
}

function loadWasm(): NativeEngine | undefined {
  const file = wasmFile();
  return file ? wasmEngine(readFileSync(file), here) : undefined;
}

/** The napi module takes `Buffer`s: Uint8Arrays are viewed as Buffers, without a copy. */
function native(m: NativeEngine): NativeEngine {
  const asBuffer = (u8: Uint8Array) => (Buffer.isBuffer(u8) ? u8 : Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength));
  return {
    render: (tree, options) => m.render(tree, options && { ...options, buffers: options.buffers?.map(asBuffer) }),
    convertHtml: (html, options) => m.convertHtml(html, options),
    htmlNodes: (html) => m.htmlNodes(html),
    version: () => m.version(),
  };
}

/** The fastest engine available: a native binary for this platform, else the WebAssembly build. */
export function loadNative(): NativeEngine {
  if (cached) return cached;
  const mode = process.env.TURBOPDF_ENGINE; // `native` | `wasm` | unset (native, then wasm)
  if (mode !== 'wasm') {
    const require = createRequire(join(here, 'noop.js'));
    for (const path of candidates()) {
      if (existsSync(path)) return (cached = native(require(path) as NativeEngine));
    }
    try {
      return (cached = native(require(`@turbopdf/engine-${platformTarget()}`) as NativeEngine));
    } catch {
      // no native binary for this platform: fall back to WebAssembly below
    }
  }
  const wasm = mode === 'native' ? undefined : loadWasm();
  if (wasm) return (cached = wasm);
  throw new TurboPdfError(
    'engine_unavailable',
    mode === 'native'
      ? `No native engine for ${process.platform}-${process.arch}. Unset TURBOPDF_ENGINE to use the WebAssembly build, or build it with \`npm run build:native\` (needs Rust).`
      : `The TurboPDF engine is missing (no native binary for ${process.platform}-${process.arch}, no wasm/turbopdf.wasm). Reinstall turbopdf, or build it with \`npm run build:native\` (needs Rust).`,
  );
}
