import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { TurboPdfError } from '../errors';
import { wasmEngine } from './wasm';

/** A document laid out by the engine: PNGs and the HTML preview are drawn from the same layout. */
export interface NativeDocument {
  readonly pages: number;
  /** JSON array of the engine's warnings. */
  readonly warnings: string;
  /** The PDF bytes, handed over without a copy: only once. */
  takePdf(): Buffer;
  png(scale?: number): Promise<Buffer[]>;
  html(): Promise<{ style: string; pages: { width: number; height: number; html: string }[] }>;
}

/** The napi module built from `engine/crates/turbopdf-node`. */
export interface NativeEngine {
  render(tree: string, options?: { baseDir?: string; buffers?: Buffer[] }): Promise<NativeDocument>;
  /** HTML → document parts (JSON), with the CSS cascade; stylesheets and fonts read inside `baseDir`. */
  convertHtml(html: string, options?: { baseDir?: string; css?: string; header?: string; footer?: string }): Promise<{ parts: string; buffers: Buffer[] }>;
  /** An HTML fragment → JSON array of tree nodes (inline styles only, no files). */
  htmlNodes(html: string): string;
  version(): string;
}

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

/** The folder with `engine.cjs` and `turbopdf.wasm`. */
function wasmDir(): string | undefined {
  return [
    process.env.TURBOPDF_WASM_DIR,
    join(here, '..', 'wasm'), // dist/index.js → wasm/
    join(here, '..', '..', 'wasm'), // src/rust/native.ts → wasm/ (tests)
  ].find((d): d is string => Boolean(d) && existsSync(join(d!, 'turbopdf.wasm')) && existsSync(join(d!, 'engine.cjs')));
}

function loadWasm(): NativeEngine | undefined {
  const dir = wasmDir();
  return dir ? wasmEngine(dir, readFileSync(join(dir, 'turbopdf.wasm'))) : undefined;
}

/** The fastest engine available: a native binary for this platform, else the WebAssembly build. */
export function loadNative(): NativeEngine {
  if (cached) return cached;
  const mode = process.env.TURBOPDF_ENGINE; // `native` | `wasm` | unset (native, then wasm)
  if (mode !== 'wasm') {
    const require = createRequire(join(here, 'noop.js'));
    for (const path of candidates()) {
      if (existsSync(path)) return (cached = require(path) as NativeEngine);
    }
    try {
      return (cached = require(`@turbopdf/engine-${platformTarget()}`) as NativeEngine);
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

/** Errors thrown by the native module carry a JSON message `{ code, message, details? }`. */
export function nativeError(e: unknown): TurboPdfError {
  const raw = e instanceof Error ? e.message : String(e);
  try {
    const { code, message, details } = JSON.parse(raw) as { code: string; message: string; details?: unknown };
    const known = code === 'invalid_tree' || code === 'font_failed' || code === 'not_accessible' || code === 'invalid_option' ? (code === 'invalid_option' ? 'invalid_source' : code) : 'render_failed';
    const err = new TurboPdfError(known, message, { cause: e });
    if (details !== undefined) err.details = details;
    return err;
  } catch {
    return new TurboPdfError('render_failed', `The Rust engine failed: ${raw}`, { cause: e });
  }
}
