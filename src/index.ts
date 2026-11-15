import { create } from './create';
import { close, configure, warmup } from './queue';

export { create } from './create';
export { close, configure, warmup } from './queue';
export { TurboPdfError } from './errors';
export type { TurboPdfErrorCode } from './errors';
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

const turbopdf = { create, close, configure, warmup };
export default turbopdf;
