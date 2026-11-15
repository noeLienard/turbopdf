import { create } from './create';
import { close, configure, warmup } from './queue';

export { create } from './create';
export { close, configure, warmup } from './queue';
export { PapierError } from './errors';
export type { PapierErrorCode } from './errors';
export type { Watermark } from './tree';
export type {
  ConfigureOptions,
  CreateOptions,
  FontSource,
  Format,
  Margin,
  Orientation,
  Pdf,
  PngOptions,
  PngPage,
  Preview,
  PreviewPage,
  ReactElementLike,
  ResponseOptions,
  Warning,
} from './types';

const papier = { create, close, configure, warmup };
export default papier;
