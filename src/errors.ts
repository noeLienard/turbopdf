import type { Warning } from './types';

export type PapierErrorCode =
  | 'invalid_source'
  | 'render_timeout'
  | 'render_failed'
  | 'react_not_installed'
  | 'warnings_found' // strict mode: the render produced warnings
  | 'aborted' // the AbortSignal fired
  | 'engine_unavailable' // the Rust engine is not built for this platform
  | 'invalid_tree' // the document tree is invalid (`details` lists every problem with its path)
  | 'font_failed' // a font could not be loaded or parsed
  | 'not_accessible'; // `pdfua: true` and the document cannot conform (`details` lists the problems)

/** Every error thrown by Papier. `message` always says what to do. */
export class PapierError extends Error {
  readonly code: PapierErrorCode;
  /** Only set for `warnings_found`. */
  warnings?: Warning[];
  /** `invalid_tree`: `{ path, code, message }[]`. `not_accessible`: the problems, as sentences. */
  details?: unknown;

  constructor(code: PapierErrorCode, message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = 'PapierError';
    this.code = code;
  }
}
