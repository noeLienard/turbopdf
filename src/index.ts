import { create } from './create';
import { close, configure, warmup } from './queue';
import './node-host';
import { engineInfo } from './rust/native';

export { create } from './create';
export { close, configure, warmup } from './queue';
export { engineInfo, type EngineInfo } from './rust/native';
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
  PreviewOptions,
  PreviewPage,
  ReactElementLike,
  ResponseOptions,
  Warning,
} from './types';

const turbopdf = { create, close, configure, warmup, engineInfo };
export default turbopdf;
