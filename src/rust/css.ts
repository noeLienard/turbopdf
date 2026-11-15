// Inline CSS (the `style` attribute React writes) → tree `Style`. A practical subset: flexbox, sizes,
// spacing, borders, colours and fonts. Unknown properties are ignored.
import type { Border, Dimension, Edges, Style, TextStyle } from '../tree';

const NAMED: Record<string, string> = {
  black: '#000000',
  white: '#ffffff',
  red: '#ff0000',
  green: '#008000',
  blue: '#0000ff',
  gray: '#808080',
  grey: '#808080',
  silver: '#c0c0c0',
  orange: '#ffa500',
  yellow: '#ffff00',
  purple: '#800080',
  navy: '#000080',
  teal: '#008080',
  transparent: 'transparent',
};

export function cssColor(value: string): string | undefined {
  const v = value.trim().toLowerCase();
  if (/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/.test(v)) return v;
  if (NAMED[v]) return NAMED[v];
  const m = /^rgba?\(([^)]+)\)$/.exec(v);
  if (!m) return undefined;
  const parts = m[1]!.split(/[\s,/]+/).filter(Boolean);
  const [r, g, b] = parts.slice(0, 3).map(Number.parseFloat);
  if (![r, g, b].every((n) => Number.isFinite(n))) return undefined;
  const hex = (n: number) => Math.round(Math.min(255, Math.max(0, n))).toString(16).padStart(2, '0');
  const alpha = parts[3] === undefined ? 1 : parts[3].endsWith('%') ? Number.parseFloat(parts[3]) / 100 : Number.parseFloat(parts[3]);
  return `#${hex(r!)}${hex(g!)}${hex(b!)}${alpha < 1 ? hex(alpha * 255) : ''}`;
}

const UNITS: Record<string, number> = { px: 0.75, pt: 1, mm: 72 / 25.4, cm: 72 / 2.54, in: 72, pc: 12 };

export interface CssLengthContext {
  /** Root font size in points. Default 11. */
  rootFontSize?: number;
  /** Page viewport in points. Defaults to A4 portrait. */
  viewportWidth?: number;
  viewportHeight?: number;
}

class LengthParser {
  private at = 0;
  constructor(private readonly source: string, private readonly fontSize: number, private readonly context: CssLengthContext) {}

  parse(): number | undefined {
    const value = this.sum();
    this.space();
    return value !== undefined && this.at === this.source.length && Number.isFinite(value) ? value : undefined;
  }

  private space() { while (/\s/.test(this.source[this.at] ?? '')) this.at++; }
  private take(char: string) { this.space(); if (this.source[this.at] !== char) return false; this.at++; return true; }

  private sum(): number | undefined {
    let left = this.product();
    if (left === undefined) return undefined;
    for (;;) {
      this.space();
      const op = this.source[this.at];
      if (op !== '+' && op !== '-') return left;
      this.at++;
      const right = this.product();
      if (right === undefined) return undefined;
      left = op === '+' ? left + right : left - right;
    }
  }

  private product(): number | undefined {
    let left = this.atom();
    if (left === undefined) return undefined;
    for (;;) {
      this.space();
      const op = this.source[this.at];
      if (op !== '*' && op !== '/') return left;
      this.at++;
      const right = this.atom();
      if (right === undefined || (op === '/' && right === 0)) return undefined;
      left = op === '*' ? left * right : left / right;
    }
  }

  private atom(): number | undefined {
    this.space();
    if (this.take('(')) {
      const value = this.sum();
      return value !== undefined && this.take(')') ? value : undefined;
    }
    const fn = /^(calc|min|max|clamp)\(/i.exec(this.source.slice(this.at));
    if (fn) {
      const name = fn[1]!.toLowerCase();
      this.at += fn[0].length;
      const args: number[] = [];
      for (;;) {
        const value = this.sum();
        if (value === undefined) return undefined;
        args.push(value);
        if (this.take(')')) break;
        if (!this.take(',')) return undefined;
      }
      if (name === 'calc') return args.length === 1 ? args[0] : undefined;
      if (name === 'min') return args.length ? Math.min(...args) : undefined;
      if (name === 'max') return args.length ? Math.max(...args) : undefined;
      return args.length === 3 ? Math.min(args[2]!, Math.max(args[0]!, args[1]!)) : undefined;
    }
    const m = /^(-?\d*\.?\d+)(px|pt|mm|cm|in|pc|em|rem|vw|vh)?/i.exec(this.source.slice(this.at));
    if (!m) return undefined;
    this.at += m[0].length;
    const n = Number(m[1]);
    const unit = (m[2] ?? '').toLowerCase();
    if (!unit) return n;
    if (unit === 'em') return n * this.fontSize;
    if (unit === 'rem') return n * (this.context.rootFontSize ?? 11);
    if (unit === 'vw') return n * (this.context.viewportWidth ?? 595.28) / 100;
    if (unit === 'vh') return n * (this.context.viewportHeight ?? 841.89) / 100;
    return n * UNITS[unit]!;
  }
}

/** CSS length → points. Supports calc/min/max/clamp and em/rem/vw/vh. Bare numbers are CSS pixels. */
export function cssLength(value: string, fontSize = 11, context: CssLengthContext = {}): number | undefined {
  const source = value.trim();
  // A bare number is a CSS pixel at the public HTML/CSS boundary. Inside arithmetic it is a scalar.
  const simple = /^(-?\d*\.?\d+)$/.exec(source);
  if (simple) return Number(simple[1]) * UNITS.px!;
  return new LengthParser(source, fontSize, context).parse();
}

function dimension(value: string, fontSize: number): Dimension | undefined {
  const v = value.trim();
  if (v === 'auto') return 'auto';
  if (/^\d*\.?\d+%$/.test(v)) return v as `${number}%`;
  const n = cssLength(v, fontSize);
  return n === undefined ? undefined : Math.max(0, n);
}

function cssTokens(value: string): string[] {
  const out: string[] = [];
  let start = 0;
  let depth = 0;
  for (let i = 0; i <= value.length; i++) {
    const c = value[i];
    if (c === '(') depth++;
    else if (c === ')') depth--;
    if ((i === value.length || /\s/.test(c ?? '')) && depth === 0) {
      const token = value.slice(start, i).trim();
      if (token) out.push(token);
      start = i + 1;
    }
  }
  return out;
}

function gridTracks(value: string, fontSize: number): string[] | undefined {
  const expanded: string[] = [];
  for (const token of cssTokens(value)) {
    const repeat = /^repeat\(\s*(\d+)\s*,([\s\S]*)\)$/i.exec(token);
    if (repeat) {
      const count = Number(repeat[1]);
      const inner = gridTracks(repeat[2]!, fontSize);
      if (!inner || count < 1 || count > 1000) return undefined;
      for (let i = 0; i < count; i++) expanded.push(...inner);
      continue;
    }
    if (/^(auto|min-content|max-content)$/i.test(token) || /^\d*\.?\d+fr$/i.test(token) || /^\d*\.?\d+%$/.test(token)) {
      expanded.push(token.toLowerCase());
      continue;
    }
    const length = cssLength(token, fontSize);
    if (length === undefined || length < 0) return undefined;
    expanded.push(`${length}pt`);
  }
  return expanded.length ? expanded : undefined;
}

function edges(value: string, fontSize: number): Edges | undefined {
  const parts = cssTokens(value).map((p) => (p === 'auto' ? 0 : cssLength(p, fontSize)));
  if (parts.length === 0 || parts.length > 4 || parts.some((p) => p === undefined)) return undefined;
  const [t, r = t, b = t, l = r] = parts as number[];
  return t === r && r === b && b === l ? t! : { top: t!, right: r!, bottom: b!, left: l! };
}

function border(value: string, fontSize: number): Border | undefined {
  if (/^(none|0)$/.test(value.trim())) return undefined;
  let width = 0.75;
  let color = '#000000';
  let style: Border['style'] = 'solid';
  for (const token of value.match(/rgba?\([^)]*\)|\S+/g) ?? []) {
    if (token === 'solid' || token === 'dashed' || token === 'dotted') style = token;
    else if (cssLength(token, fontSize) !== undefined) width = cssLength(token, fontSize)!;
    else if (cssColor(token)) color = cssColor(token)!;
  }
  return { width, color, style };
}

const ALIGN: Record<string, Style['alignItems']> = { 'flex-start': 'flex-start', start: 'flex-start', 'flex-end': 'flex-end', end: 'flex-end', center: 'center', stretch: 'stretch', baseline: 'baseline' };
const JUSTIFY: Record<string, Style['justifyContent']> = {
  'flex-start': 'flex-start',
  start: 'flex-start',
  left: 'flex-start',
  'flex-end': 'flex-end',
  end: 'flex-end',
  right: 'flex-end',
  center: 'center',
  'space-between': 'space-between',
  'space-around': 'space-around',
  'space-evenly': 'space-evenly',
};

function fontWeight(v: string): number | undefined {
  if (v === 'bold' || v === 'bolder') return 700;
  if (v === 'normal' || v === 'lighter') return 400;
  const n = Number.parseInt(v, 10);
  return n >= 100 && n <= 900 ? n : undefined;
}

/** Parse a `style="…"` attribute. `inheritedFontSize` resolves `em`. */
export function parseCss(css: string | undefined, inheritedFontSize = 11): Style {
  const s: Style = {};
  if (!css) return s;
  const decls = css
    .split(';')
    .map((d) => d.split(/:(.*)/s))
    .filter((d) => d.length >= 2)
    .map(([k, v]) => [k!.trim().toLowerCase(), v!.trim().replace(/\s*!important$/, '')] as const);
  const fs = decls.find(([k]) => k === 'font-size');
  const fontSize = (fs && cssLength(fs[1], inheritedFontSize)) || inheritedFontSize;
  let display: string | undefined;

  for (const [k, v] of decls) {
    switch (k) {
      case 'display':
        display = v;
        if (v === 'grid' || v === 'inline-grid') s.display = 'grid';
        else if (v === 'flex' || v === 'inline-flex') s.display = 'flex';
        break;
      case 'grid-template-columns':
        s.gridTemplateColumns = gridTracks(v, fontSize);
        break;
      case 'grid-template-rows':
        s.gridTemplateRows = gridTracks(v, fontSize);
        break;
      case 'flex-direction':
        if (['row', 'row-reverse', 'column', 'column-reverse'].includes(v)) s.flexDirection = v as Style['flexDirection'];
        break;
      case 'flex-wrap':
        s.flexWrap = v === 'wrap' ? 'wrap' : 'nowrap';
        break;
      case 'align-items':
        if (ALIGN[v]) s.alignItems = ALIGN[v];
        break;
      case 'align-self':
        if (ALIGN[v]) s.alignSelf = ALIGN[v];
        break;
      case 'align-content':
        if (JUSTIFY[v]) s.alignContent = JUSTIFY[v];
        break;
      case 'justify-content':
        if (JUSTIFY[v]) s.justifyContent = JUSTIFY[v];
        break;
      case 'gap': {
        const values = cssTokens(v).map((x) => cssLength(x, fontSize));
        if (values[0] !== undefined) {
          if (values[1] === undefined) s.gap = Math.max(0, values[0]);
          else {
            s.rowGap = Math.max(0, values[0]);
            s.columnGap = Math.max(0, values[1]);
          }
        }
        break;
      }
      case 'row-gap':
      case 'column-gap': {
        const n = cssLength(cssTokens(v)[0]!, fontSize);
        if (n !== undefined) s[k === 'row-gap' ? 'rowGap' : 'columnGap'] = Math.max(0, n);
        break;
      }
      case 'flex': {
        const [grow, shrink, basis] = v.split(/\s+/);
        if (v === 'none') s.flexGrow = 0;
        else if (v === 'auto') s.flexGrow = 1;
        else {
          if (grow !== undefined && !Number.isNaN(Number(grow))) s.flexGrow = Number(grow);
          if (shrink !== undefined && !Number.isNaN(Number(shrink))) s.flexShrink = Number(shrink);
          if (basis !== undefined) s.flexBasis = dimension(basis, fontSize);
        }
        break;
      }
      case 'flex-grow':
        s.flexGrow = Math.max(0, Number(v) || 0);
        break;
      case 'flex-shrink':
        s.flexShrink = Math.max(0, Number(v) || 0);
        break;
      case 'flex-basis':
        s.flexBasis = dimension(v, fontSize);
        break;
      case 'position':
        // `absolute` / `fixed`: placed in the parent box; `relative` / `static` keep the flow
        if (/^(absolute|fixed)$/.test(v.trim())) s.position = 'absolute';
        else if (v.trim() === 'relative') s.position = 'relative';
        break;
      case 'top':
      case 'right':
      case 'bottom':
      case 'left': {
        const t = v.trim();
        const d = t === 'auto' ? 'auto' : /^-?\d*\.?\d+%$/.test(t) ? (t as `${number}%`) : cssLength(t, fontSize);
        if (d !== undefined) s[k] = d;
        break;
      }
      case 'inset': {
        const e = edges(v, fontSize);
        if (e !== undefined) {
          const [t, r, b, l] = typeof e === 'number' ? [e, e, e, e] : [e.top!, e.right!, e.bottom!, e.left!];
          Object.assign(s, { top: t, right: r, bottom: b, left: l });
        }
        break;
      }
      case 'width':
        s.width = dimension(v, fontSize);
        break;
      case 'height':
        s.height = dimension(v, fontSize);
        break;
      case 'min-width':
        s.minWidth = dimension(v, fontSize);
        break;
      case 'max-width':
        s.maxWidth = dimension(v, fontSize);
        break;
      case 'min-height':
        s.minHeight = dimension(v, fontSize);
        break;
      case 'max-height':
        s.maxHeight = dimension(v, fontSize);
        break;
      case 'aspect-ratio': {
        const [a, b = '1'] = v.split('/').map((x) => x.trim());
        const ratio = Number(a) / Number(b);
        if (ratio > 0 && Number.isFinite(ratio)) s.aspectRatio = ratio;
        break;
      }
      case 'padding':
        s.padding = edges(v, fontSize);
        if (s.padding !== undefined) s.padding = clampEdges(s.padding);
        break;
      case 'margin':
        s.margin = edges(v, fontSize);
        break;
      case 'padding-top':
      case 'padding-right':
      case 'padding-bottom':
      case 'padding-left':
        s.padding = setSide(s.padding, k.slice(8), Math.max(0, cssLength(v, fontSize) ?? 0));
        break;
      case 'margin-top':
      case 'margin-right':
      case 'margin-bottom':
      case 'margin-left':
        s.margin = setSide(s.margin, k.slice(7), v === 'auto' ? 0 : (cssLength(v, fontSize) ?? 0));
        break;
      case 'border':
        s.border = border(v, fontSize);
        break;
      case 'border-color':
        if (s.border && cssColor(v)) s.border.color = cssColor(v)!;
        break;
      case 'border-width':
        if (s.border && cssLength(v, fontSize) !== undefined) s.border.width = cssLength(v, fontSize)!;
        break;
      case 'border-radius': {
        const n = cssLength(cssTokens(v)[0]!, fontSize);
        if (n !== undefined) s.borderRadius = Math.max(0, n);
        break;
      }
      case 'background':
      case 'background-color': {
        const c = cssColor(v.split(/\s+/)[0]!) ?? cssColor(v);
        if (c) s.backgroundColor = c;
        break;
      }
      case 'color': {
        const c = cssColor(v);
        if (c) s.color = c;
        break;
      }
      case 'font-family': {
        const first = v.split(',')[0]!.trim().replace(/^["']|["']$/g, '');
        if (first) s.fontFamily = first;
        break;
      }
      case 'font-size':
        s.fontSize = fontSize;
        break;
      case 'font-weight': {
        const w = fontWeight(v);
        if (w) s.fontWeight = w;
        break;
      }
      case 'font-style':
        s.fontStyle = v === 'italic' || v === 'oblique' ? 'italic' : 'normal';
        break;
      case 'text-align':
        if (['left', 'center', 'right', 'justify'].includes(v)) s.textAlign = v as Style['textAlign'];
        else if (v === 'start') s.textAlign = 'left';
        else if (v === 'end') s.textAlign = 'right';
        break;
      case 'line-height': {
        if (/^\d*\.?\d+$/.test(v)) s.lineHeight = Number(v);
        else if (v.endsWith('%')) s.lineHeight = Number.parseFloat(v) / 100;
        else {
          const n = cssLength(v, fontSize);
          if (n) s.lineHeight = n / fontSize;
        }
        break;
      }
    }
  }
  // `display: flex` lays out in a row unless told otherwise (a tree view is a column by default)
  if ((display === 'flex' || display === 'inline-flex') && !s.flexDirection) s.flexDirection = 'row';
  for (const key of Object.keys(s) as (keyof Style)[]) if (s[key] === undefined) delete s[key];
  return s;
}

function clampEdges(e: Edges): Edges {
  if (typeof e === 'number') return Math.max(0, e);
  return { top: Math.max(0, e.top ?? 0), right: Math.max(0, e.right ?? 0), bottom: Math.max(0, e.bottom ?? 0), left: Math.max(0, e.left ?? 0) };
}

function setSide(e: Edges | undefined, side: string, v: number): Edges {
  const base = typeof e === 'number' ? { top: e, right: e, bottom: e, left: e } : { ...(e ?? {}) };
  return { ...base, [side]: v };
}

/** The part of a style that applies to a span of text. */
export function textStyleOf(s: Style): TextStyle | undefined {
  const t: TextStyle = {};
  if (s.fontFamily) t.fontFamily = s.fontFamily;
  if (s.fontSize) t.fontSize = s.fontSize;
  if (s.fontWeight) t.fontWeight = s.fontWeight;
  if (s.fontStyle) t.fontStyle = s.fontStyle;
  if (s.color) t.color = s.color;
  return Object.keys(t).length ? t : undefined;
}
