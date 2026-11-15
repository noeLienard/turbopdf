import { TurboPdfError } from './errors';
import type { Format, Length, Margin, Orientation } from './types';

/** Portrait size in millimetres. */
const SIZES_MM: Record<Format, [number, number]> = {
  A3: [297, 420],
  A4: [210, 297],
  A5: [148, 210],
  Letter: [215.9, 279.4],
  Legal: [215.9, 355.6],
};

export const FORMATS = Object.keys(SIZES_MM) as Format[];

const UNITS_PX: Record<string, number> = { px: 1, in: 96, cm: 96 / 2.54, mm: 96 / 25.4, pt: 96 / 72, pc: 16 };

/** '15mm' | '1in' | '20px' | 12 → CSS pixels. */
export function toPx(value: string | number): number {
  if (typeof value === 'number') return value;
  const m = /^\s*(-?\d*\.?\d+)\s*(px|in|cm|mm|pt|pc)?\s*$/i.exec(value);
  if (!m) throw new TurboPdfError('invalid_source', `Invalid length "${value}". Use a CSS length such as '15mm', '1in' or '20px'.`);
  return Number(m[1]) * UNITS_PX[(m[2] ?? 'px').toLowerCase()]!;
}

export interface Sides { top: Length; right: Length; bottom: Length; left: Length }

export function normalizeMargin(margin: Margin | undefined): Sides {
  if (margin === undefined) margin = '10mm';
  if (typeof margin === 'string' || typeof margin === 'number') return { top: margin, right: margin, bottom: margin, left: margin };
  return { top: margin.top ?? '10mm', right: margin.right ?? '10mm', bottom: margin.bottom ?? '10mm', left: margin.left ?? '10mm' };
}

export type PageGeometry = { format: Format; orientation: Orientation } | { width: number; height: number };

/** Page size in CSS pixels (96 dpi), orientation applied. */
export function pageSizePx(format: Format, orientation: Orientation): { width: number; height: number } {
  const [w, h] = SIZES_MM[format];
  const [pw, ph] = orientation === 'landscape' ? [h, w] : [w, h];
  return { width: Math.round((pw / 25.4) * 96), height: Math.round((ph / 25.4) * 96) };
}

/**
 * Printable area in CSS pixels: the page minus margins. The browser lays the
 * document out at exactly this width, so the PDF never switches to a mobile layout.
 */
export function contentSizePx(geometry: PageGeometry, margin: Sides) {
  const page = 'format' in geometry ? pageSizePx(geometry.format, geometry.orientation) : geometry;
  return {
    width: Math.max(100, Math.round(page.width - toPx(margin.left) - toPx(margin.right))),
    height: Math.max(100, Math.round(page.height - toPx(margin.top) - toPx(margin.bottom))),
  };
}
