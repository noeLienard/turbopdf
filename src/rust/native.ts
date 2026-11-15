import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { TurboPdfError } from '../errors';

/** The napi module built from `engine/crates/turbopdf-node`. */
export interface NativeEngine {
  render(tree: string, options?: { baseDir?: string }): Promise<{ pdf: Buffer; pages: number; warnings: string }>;
  preview(tree: string, options?: { baseDir?: string; scale?: number }): Promise<Buffer[]>;
  previewHtml(tree: string, options?: { baseDir?: string }): Promise<{ style: string; pages: { width: number; height: number; html: string }[] }>;
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

export function loadNative(): NativeEngine {
  if (cached) return cached;
  const require = createRequire(join(here, 'noop.js'));
  for (const path of candidates()) {
    if (existsSync(path)) return (cached = require(path) as NativeEngine);
  }
  try {
    return (cached = require(`@turbopdf/engine-${process.platform}-${process.arch}`) as NativeEngine);
  } catch {
    throw new TurboPdfError(
      'engine_unavailable',
      `The Rust engine is not available for ${process.platform}-${process.arch}. Build it with \`npm run build:native\` (needs Rust).`,
    );
  }
}

/** Errors thrown by the native module carry a JSON message `{ code, message, details? }`. */
export function nativeError(e: unknown): TurboPdfError {
  const raw = e instanceof Error ? e.message : String(e);
  try {
    const { code, message, details } = JSON.parse(raw) as { code: string; message: string; details?: unknown };
    const known = code === 'invalid_tree' || code === 'font_failed' || code === 'not_accessible' ? code : 'render_failed';
    const err = new TurboPdfError(known, message, { cause: e });
    if (details !== undefined) err.details = details;
    return err;
  } catch {
    return new TurboPdfError('render_failed', `The Rust engine failed: ${raw}`, { cause: e });
  }
}
