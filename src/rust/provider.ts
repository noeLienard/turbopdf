// The engine the SDK renders with, set by the entry point: `turbopdf` (Node, Bun, Deno: native module, or
// WebAssembly in a worker) or `turbopdf/edge` (WebAssembly on the calling thread, after `init()`).
import { TurboPdfError } from '../errors';
import type { NativeEngine } from './engine-api';

interface Host {
  engine(): NativeEngine;
  /** `baseDir` when the options give none: the working directory in Node, none (no files) on the edge. */
  baseDir(): string | undefined;
  /**
   * `renderToStaticMarkup` from `react-dom/server`, loaded on first use. Node imports it from the project;
   * the edge entry never imports it (an unresolved import would stop a Worker from loading): it is passed
   * to `init()` instead, or `undefined`.
   */
  react(): Promise<ReactRenderer | undefined>;
}

export type ReactRenderer = (element: unknown) => string;

let host: Host | undefined;

export function setHost(h: Host): void {
  host = h;
}

function current(): Host {
  if (!host) throw new TurboPdfError('engine_unavailable', 'No TurboPDF engine is set up. Import `turbopdf` (Node, Bun, Deno), or call `init()` from `turbopdf/edge` first.');
  return host;
}

export const engine = (): NativeEngine => current().engine();
export const defaultBaseDir = (): string | undefined => current().baseDir();
export const reactRenderer = (): Promise<ReactRenderer | undefined> => current().react();
