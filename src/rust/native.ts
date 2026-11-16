import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { TurboPdfError } from '../errors';
import type { NativeEngine } from './engine-api';
import { wasmEngine } from './wasm';

export { nativeError, type NativeDocument, type NativeEngine } from './engine-api';

/** Set per output format by tsup (`define`): the CommonJS build has a real `require`. Unset from sources. */
declare const __TURBOPDF_CJS__: boolean;
const CJS = typeof __TURBOPDF_CJS__ !== 'undefined' && __TURBOPDF_CJS__;

let cached: NativeEngine | undefined;

/** Which engine renders, for logs: `turbopdf.engineInfo()`. */
export interface EngineInfo {
  kind: 'native' | 'wasm';
  /** The platform package for this machine, e.g. `linux-x64-gnu`. */
  target: string;
  /** The engine's version. */
  version: string;
  /** Why the native engine is not used, when it is WebAssembly without `TURBOPDF_ENGINE=wasm`. */
  fallbackReason?: string;
}

let info: Omit<EngineInfo, 'version'> | undefined;

const here = typeof __dirname === 'string' ? __dirname : dirname(fileURLToPath(import.meta.url));

/** Where the compiled engine may be: explicit override, next to the package, or a platform package. */
function candidates(): string[] {
  // an explicit override is the only native engine tried
  if (process.env.TURBOPDF_NATIVE) return [process.env.TURBOPDF_NATIVE];
  const file = `turbopdf.${process.platform}-${process.arch}.node`;
  return [
    join(here, '..', 'native', file), // dist/index.js → native/
    join(here, '..', '..', 'native', file), // src/rust/native.ts → native/ (tests)
  ];
}

/** The prebuilt package for this machine: `linux-x64-gnu`, `linux-x64-musl` (Alpine), `darwin-arm64`, `win32-x64-msvc`… */
export function platformTarget(): string {
  const base = `${process.platform}-${process.arch}`;
  if (process.platform === 'win32') return `${base}-msvc`;
  if (process.platform !== 'linux') return base;
  const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined;
  return `${base}-${report?.header?.glibcVersionRuntime ? 'gnu' : 'musl'}`;
}

/**
 * The package's WebAssembly engine and its worker, written as literal paths relative to this file so file
 * tracers (@vercel/nft, Next.js output tracing) deploy them: `new URL(…, import.meta.url)` in the ESM build,
 * `join(__dirname, …)` in the CommonJS one. Guarded by scripts/check-trace.mjs.
 */
function packaged(): { wasm: string; worker: string } {
  if (CJS) {
    return { wasm: join(__dirname, '..', 'wasm', 'turbopdf.wasm'), worker: join(__dirname, 'wasm-worker.cjs') };
  }
  return {
    wasm: fileURLToPath(new URL('../wasm/turbopdf.wasm', import.meta.url)),
    worker: fileURLToPath(new URL('./wasm-worker.cjs', import.meta.url)),
  };
}

const firstExisting = (paths: (string | undefined)[]) => paths.find((p): p is string => Boolean(p) && existsSync(p!));

function loadWasm(): NativeEngine | undefined {
  const own = packaged();
  const file = firstExisting([
    process.env.TURBOPDF_WASM,
    own.wasm, // dist/index.js → wasm/
    join(here, '..', '..', 'wasm', 'turbopdf.wasm'), // src/rust/native.ts → wasm/ (tests)
  ]);
  if (!file) return undefined;
  const worker = firstExisting([process.env.TURBOPDF_WASM_WORKER, own.worker, join(here, '..', '..', 'dist', 'wasm-worker.cjs')]);
  return wasmEngine(readFileSync(file), worker);
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


// ESM: a `require` made by `createRequire` at module level, which file tracers (@vercel/nft) recognise.
const esmRequire = createRequire(import.meta.url);

/**
 * The prebuilt package for `target`. Each name is a literal `require`: bundlers and file tracers (Vercel,
 * Netlify, Next.js standalone) only copy the packages they can see required, never a computed name.
 * Guarded by scripts/check-trace.mjs.
 */
function prebuilt(target: string): NativeEngine {
  if (CJS) {
    switch (target) {
      case 'darwin-arm64':
        return require('@turbopdf/engine-darwin-arm64') as NativeEngine;
      case 'darwin-x64':
        return require('@turbopdf/engine-darwin-x64') as NativeEngine;
      case 'linux-x64-gnu':
        return require('@turbopdf/engine-linux-x64-gnu') as NativeEngine;
      case 'linux-x64-musl':
        return require('@turbopdf/engine-linux-x64-musl') as NativeEngine;
      case 'linux-arm64-gnu':
        return require('@turbopdf/engine-linux-arm64-gnu') as NativeEngine;
      case 'win32-x64-msvc':
        return require('@turbopdf/engine-win32-x64-msvc') as NativeEngine;
      default:
        throw new Error(`no prebuilt engine for ${target}`);
    }
  }
  switch (target) {
    case 'darwin-arm64':
      return esmRequire('@turbopdf/engine-darwin-arm64') as NativeEngine;
    case 'darwin-x64':
      return esmRequire('@turbopdf/engine-darwin-x64') as NativeEngine;
    case 'linux-x64-gnu':
      return esmRequire('@turbopdf/engine-linux-x64-gnu') as NativeEngine;
    case 'linux-x64-musl':
      return esmRequire('@turbopdf/engine-linux-x64-musl') as NativeEngine;
    case 'linux-arm64-gnu':
      return esmRequire('@turbopdf/engine-linux-arm64-gnu') as NativeEngine;
    case 'win32-x64-msvc':
      return esmRequire('@turbopdf/engine-win32-x64-msvc') as NativeEngine;
    default:
      throw new Error(`no prebuilt engine for ${target}`);
  }
}

const PREBUILT = new Set(['darwin-arm64', 'darwin-x64', 'linux-x64-gnu', 'linux-x64-musl', 'linux-arm64-gnu', 'win32-x64-msvc']);

/** Why the native engine did not load, with the loader's own message (e.g. `GLIBC_2.35 not found`). */
function nativeFailure(target: string, e: unknown): string {
  const err = e as NodeJS.ErrnoException;
  if (!PREBUILT.has(target)) return `no prebuilt native engine for ${target}`;
  if (err?.code === 'MODULE_NOT_FOUND' && String(err.message).includes(`@turbopdf/engine-${target}`)) {
    return `@turbopdf/engine-${target} is not installed (optional dependencies were skipped: reinstall without --omit=optional / --no-optional)`;
  }
  return `@turbopdf/engine-${target} failed to load: ${err?.message ?? String(e)}`;
}

let warned = false;

/** The fastest engine available: a native binary for this platform, else the WebAssembly build. */
export function loadNative(): NativeEngine {
  if (cached) return cached;
  const mode = process.env.TURBOPDF_ENGINE; // `native` | `wasm` | unset (native, then wasm)
  const target = platformTarget();
  let reason: string | undefined;
  let cause: unknown;
  if (mode !== 'wasm') {
    const local = createRequire(join(here, 'noop.js'));
    for (const path of candidates()) {
      if (!existsSync(path)) continue;
      try {
        cached = native(local(path) as NativeEngine);
        info = { kind: 'native', target };
        return cached;
      } catch (e) {
        cause ??= e;
        reason ??= `${path} failed to load: ${(e as Error)?.message ?? String(e)}`;
      }
    }
    // an explicit TURBOPDF_NATIVE that failed is not replaced by the prebuilt package
    if (!process.env.TURBOPDF_NATIVE) {
      try {
        cached = native(prebuilt(target));
        info = { kind: 'native', target };
        return cached;
      } catch (e) {
        // fall back to WebAssembly below, and say why
        cause = e;
        reason = nativeFailure(target, e);
      }
    }
  }
  if (mode === 'native') {
    throw new TurboPdfError(
      'engine_unavailable',
      `The native engine could not be used: ${reason}. Unset TURBOPDF_ENGINE to use the WebAssembly build, or build it with \`npm run build:native\` (needs Rust).`,
      { cause },
    );
  }
  const wasm = loadWasm();
  if (!wasm) {
    throw new TurboPdfError(
      'engine_unavailable',
      `The TurboPDF engine is missing: ${reason ?? 'TURBOPDF_ENGINE=wasm'}, and wasm/turbopdf.wasm was not found. Reinstall turbopdf; on a serverless platform, make sure node_modules/turbopdf/wasm is deployed.`,
      { cause },
    );
  }
  info = { kind: 'wasm', target, ...(reason ? { fallbackReason: reason } : {}) };
  // a platform with a prebuilt engine should not silently run 5× slower
  if (reason && PREBUILT.has(target) && !warned && !process.env.TURBOPDF_NO_FALLBACK_WARNING) {
    warned = true;
    console.warn(`[turbopdf] using the slower WebAssembly engine: ${reason}. Set TURBOPDF_NO_FALLBACK_WARNING=1 to hide this.`);
  }
  return (cached = wasm);
}

/** Which engine renders (loads it if needed): native or WebAssembly, and why not native. */
export function engineInfo(): EngineInfo {
  const engine = loadNative();
  return { ...info!, version: engine.version() };
}
