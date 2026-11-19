// What the SDK needs from the Rust engine, whichever build runs it: the native module (napi), WebAssembly
// in a worker thread (Node, Bun, Deno), or WebAssembly on the calling thread (edge runtimes, browsers).
import { TurboPdfError } from '../errors';

/** A document laid out by the engine: PNGs and the HTML preview are drawn from the same layout. */
export interface NativeDocument {
  readonly pages: number;
  /** JSON array of the engine's warnings. */
  readonly warnings: string;
  /** JSON array of the form fields and where they were laid out (absent from engines older than 0.3). */
  readonly fields?: string;
  /** The PDF bytes, handed over without a copy: only once. */
  takePdf(): Uint8Array;
  png(scale?: number): Promise<Uint8Array[]>;
  html(): Promise<{ style: string; pages: { width: number; height: number; html: string }[] }>;
}

export interface NativeEngine {
  render(tree: string, options?: { baseDir?: string; buffers?: Uint8Array[] }): Promise<NativeDocument>;
  /** HTML → document parts (JSON), with the CSS cascade; stylesheets and fonts read inside `baseDir`. */
  convertHtml(html: string, options?: { baseDir?: string; css?: string; header?: string; footer?: string }): Promise<{ parts: string; buffers: Uint8Array[] }>;
  /** An HTML fragment → JSON array of tree nodes (inline styles only, no files). */
  htmlNodes(html: string): string;
  version(): string;
}

/** Errors thrown by the engine carry a JSON message `{ code, message, details? }`. */
export function nativeError(e: unknown): TurboPdfError {
  const raw = e instanceof Error ? e.message : String(e);
  try {
    const { code, message, details } = JSON.parse(raw) as { code: string; message: string; details?: unknown };
    const known = code === 'invalid_tree' || code === 'font_failed' || code === 'not_accessible' || code === 'not_pdfa' || code === 'invalid_option' ? (code === 'invalid_option' ? 'invalid_source' : code) : 'render_failed';
    const err = new TurboPdfError(known, message, { cause: e });
    if (details !== undefined) err.details = details;
    return err;
  } catch {
    return new TurboPdfError('render_failed', `The Rust engine failed: ${raw}`, { cause: e });
  }
}
