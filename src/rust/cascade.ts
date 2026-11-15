// A small CSS cascade: stylesheets (<style>, local <link>, the `css` option) are matched against the parsed
// markup and the winning declarations are written into each element's `style` attribute, where the
// HTML → tree converter reads them. Inherited properties (fonts, colours, alignment) are inherited by the
// engine itself.
//
// Supported selectors: type, `*`, `.class`, `#id`, `[attr]`, `[attr=value]`, descendant and `>` combinators,
// `:first-child`, `:last-child`, `:nth-child(odd | even | N | An+B)`, selector lists. Rules using anything
// else (`+`, `~`, `:hover`, `::before`…) never match. At-rules: `@media` (print / all applied, screen-only
// skipped), `@page { size; margin }`, `@font-face` (local files, collected for the engine), the rest ignored.
import type { Child, El } from './markup';

type Combinator = ' ' | '>';
interface Compound {
  tag?: string;
  id?: string;
  root?: boolean;
  classes: string[];
  attrs: { name: string; value?: string }[];
  pseudo: ((el: El, index: number, count: number) => boolean)[];
}
interface Selector {
  parts: Compound[]; // left → right
  combinators: Combinator[]; // between parts
  specificity: number;
  pseudoElement?: 'before' | 'after';
}
interface Rule {
  selectors: Selector[];
  declarations: [prop: string, value: string, important: boolean][];
  order: number;
}

export interface PageRule {
  size?: string;
  margin?: string;
}

/** An `@font-face` rule. `src` lists the `url(…)`s in order, resolved against the stylesheet's folder. */
export interface FontFaceRule {
  family: string;
  src: { url: string; format?: string }[];
  weight?: string;
  style?: string;
}

export interface Stylesheet {
  rules: Rule[];
  page: PageRule;
  fontFaces: FontFaceRule[];
}

/** A selector filed under the most selective key of its rightmost compound (`#id`, `.class`, `tag` or `*`). */
interface Entry {
  rule: Rule;
  selector: Selector;
}
type RuleIndex = Map<string, Entry[]>;

/**
 * Browsers' rule hash: an element only tests the selectors filed under its id, its classes, its tag and `*`,
 * instead of every rule of the stylesheet (a Bootstrap-size stylesheet has thousands).
 */
const indexes = new WeakMap<Stylesheet, RuleIndex>();
function indexOf(sheet: Stylesheet): RuleIndex {
  let index = indexes.get(sheet);
  if (index) return index;
  index = new Map();
  for (const rule of sheet.rules) {
    for (const selector of rule.selectors) {
      const last = selector.parts[selector.parts.length - 1]!;
      const key = last.id !== undefined ? `#${last.id}` : last.classes.length ? `.${last.classes[0]}` : (last.tag ?? '*');
      let list = index.get(key);
      if (!list) index.set(key, (list = []));
      list.push({ rule, selector });
    }
  }
  indexes.set(sheet, index);
  return index;
}

const classLists = new WeakMap<El, string[]>();
function classesOf(el: El): string[] {
  let list = classLists.get(el);
  if (!list) {
    list = (el.attrs.class ?? el.attrs.classname ?? '').split(/\s+/).filter(Boolean);
    classLists.set(el, list);
  }
  return list;
}

/** Selectors that may match `el`, from the index: each one once, in no particular order. */
function candidates(el: El, index: RuleIndex): Entry[] {
  const keys = ['*', el.tag, ...classesOf(el).map((c) => `.${c}`)];
  if (el.attrs.id) keys.push(`#${el.attrs.id}`);
  const out: Entry[] = [];
  for (const key of new Set(keys)) {
    const list = index.get(key);
    if (list) for (const e of list) out.push(e);
  }
  return out;
}

/** A stylesheet's text, and the folder its relative `url(…)`s are relative to (`''`: the document's). */
export type StyleSource = string | { css: string; base: string };

const unquote = (v: string) => v.trim().replace(/^(['"])(.*)\1$/, '$2');

function fontFace(body: string, base: string): FontFaceRule | undefined {
  const rule: FontFaceRule = { family: '', src: [] };
  for (const [k, v] of declarations(body)) {
    if (k === 'font-family') rule.family = unquote(v);
    else if (k === 'font-weight') rule.weight = v;
    else if (k === 'font-style') rule.style = v;
    else if (k === 'src') {
      for (const m of v.matchAll(/url\(\s*(['"]?)(.*?)\1\s*\)(?:\s*format\(\s*['"]?([\w-]+)['"]?\s*\))?/gi)) {
        const url = m[2]!;
        const relative = !/^[a-z][\w+.-]*:/i.test(url) && !url.startsWith('/');
        rule.src.push({ url: relative && base ? `${base.replace(/\/$/, '')}/${url}` : url, format: m[3]?.toLowerCase() });
      }
    }
  }
  return rule.family && rule.src.length ? rule : undefined;
}

// ------------------------------------------------------------------ parsing

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Split at top-level `{ … }` blocks: [prelude, body][] */
function blocksOf(css: string): [string, string][] {
  const out: [string, string][] = [];
  let i = 0;
  while (i < css.length) {
    const open = css.indexOf('{', i);
    const semi = css.indexOf(';', i);
    // statement at-rules without a block (`@import …;`, `@charset …;`)
    if (semi !== -1 && (open === -1 || semi < open) && css.slice(i, semi).trim().startsWith('@')) {
      i = semi + 1;
      continue;
    }
    if (open === -1) break;
    let depth = 1;
    let j = open + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === '{') depth++;
      else if (css[j] === '}') depth--;
      j++;
    }
    out.push([css.slice(i, open).trim(), css.slice(open + 1, j - 1)]);
    i = j;
  }
  return out;
}

export function declarations(body: string): [string, string, boolean][] {
  return body
    .split(';')
    .map((d) => d.split(/:(.*)/s))
    .filter((d) => d.length >= 2 && d[0]!.trim())
    .map(([k, v]) => {
      const important = /!\s*important\s*$/i.test(v!);
      return [k!.trim().toLowerCase(), v!.replace(/!\s*important\s*$/i, '').trim(), important];
    });
}

function nth(expr: string): ((i: number) => boolean) | undefined {
  const e = expr.replace(/\s+/g, '').toLowerCase();
  if (e === 'odd') return (i) => i % 2 === 1;
  if (e === 'even') return (i) => i % 2 === 0;
  if (/^\d+$/.test(e)) return (i) => i === Number(e);
  const m = /^([+-]?\d*)n([+-]\d+)?$/.exec(e);
  if (!m) return undefined;
  const a = m[1] === '' || m[1] === '+' ? 1 : m[1] === '-' ? -1 : Number(m[1]);
  const b = Number(m[2] ?? 0);
  return (i) => (a === 0 ? i === b : (i - b) / a >= 0 && (i - b) % a === 0);
}

function compound(src: string): Compound | undefined {
  const c: Compound = { classes: [], attrs: [], pseudo: [] };
  const re = /^(\*|[a-zA-Z][\w-]*)|\.([\w-]+)|#([\w-]+)|\[\s*([\w-]+)\s*(?:=\s*(?:"([^"]*)"|'([^']*)'|([^\]\s]+)))?\s*\]|:([\w-]+)(?:\(([^)]*)\))?|(.)/g;
  for (const m of src.matchAll(re)) {
    if (m[10] !== undefined) return undefined; // unsupported syntax
    if (m[1]) {
      if (m[1] !== '*') c.tag = m[1].toLowerCase();
    } else if (m[2]) c.classes.push(m[2]);
    else if (m[3]) c.id = m[3];
    else if (m[4]) c.attrs.push({ name: m[4].toLowerCase(), value: m[5] ?? m[6] ?? m[7] });
    else if (m[8]) {
      const name = m[8].toLowerCase();
      if (name === 'first-child') c.pseudo.push((_e, i) => i === 1);
      else if (name === 'last-child') c.pseudo.push((_e, i, n) => i === n);
      else if (name === 'nth-child' && m[9] !== undefined && nth(m[9])) {
        const f = nth(m[9])!;
        c.pseudo.push((_e, i) => f(i));
      } else if (name === 'root') { c.root = true; c.pseudo.push((e) => e.tag === 'html'); }
      else return undefined; // :hover, ::before… never match on paper
    }
  }
  return c;
}

function selector(src: string): Selector | undefined {
  const generated = /::(before|after)\s*$/i.exec(src);
  const pseudoElement = generated?.[1]?.toLowerCase() as Selector['pseudoElement'];
  if (generated) src = src.slice(0, generated.index);
  const tokens = src
    .trim()
    .replace(/\(([^)]*)\)/g, (m) => m.replace(/\s+/g, '')) // :nth-child( 2n + 1 )
    .replace(/\s*>\s*/g, ' > ')
    .split(/\s+/);
  // sibling combinators are not supported; a `+` / `~` inside a compound is rejected by `compound()`
  if (tokens.some((t) => t === '+' || t === '~')) return undefined;
  const parts: Compound[] = [];
  const combinators: Combinator[] = [];
  let pending: Combinator = ' ';
  let a = 0;
  let b = 0;
  let c = 0;
  for (const t of tokens) {
    if (t === '>') {
      pending = '>';
      continue;
    }
    const cp = compound(t);
    if (!cp) return undefined;
    if (parts.length) combinators.push(pending);
    pending = ' ';
    parts.push(cp);
    if (cp.id) a++;
    b += cp.classes.length + cp.attrs.length + cp.pseudo.length;
    if (cp.tag) c++;
  }
  return parts.length ? { parts, combinators, specificity: a * 10000 + b * 100 + c, ...(pseudoElement ? { pseudoElement } : {}) } : undefined;
}

function walk(css: string, out: Stylesheet, counter: { n: number }, base: string): void {
  for (const [prelude, body] of blocksOf(stripComments(css))) {
    if (prelude.startsWith('@')) {
      const name = /^@([\w-]+)/.exec(prelude)?.[1]?.toLowerCase();
      if (name === 'media') {
        const q = prelude.slice(6).toLowerCase();
        if (/print|all/.test(q) || !/screen/.test(q)) walk(body, out, counter, base);
      } else if (name === 'font-face') {
        const f = fontFace(body, base);
        if (f) out.fontFaces.push(f);
      } else if (name === 'page') {
        for (const [k, v] of declarations(body)) {
          if (k === 'size') out.page.size = v;
          if (k === 'margin') out.page.margin = v;
        }
      }
      continue;
    }
    const selectors = prelude.split(',').map(selector).filter((s): s is Selector => Boolean(s));
    if (selectors.length) out.rules.push({ selectors, declarations: declarations(body), order: counter.n++ });
  }
}

export function parseStylesheets(sources: StyleSource[]): Stylesheet {
  const sheet: Stylesheet = { rules: [], page: {}, fontFaces: [] };
  const counter = { n: 0 };
  for (const src of sources) {
    if (typeof src === 'string') walk(src, sheet, counter, '');
    else walk(src.css, sheet, counter, src.base);
  }
  return sheet;
}

// ------------------------------------------------------------------ matching

interface Ctx {
  el: El;
  index: number; // 1-based among element siblings
  count: number;
  parent?: Ctx;
}

function matchesCompound(c: Compound, x: Ctx): boolean {
  const el = x.el;
  if (c.tag && c.tag !== el.tag) return false;
  if (c.id && el.attrs.id !== c.id) return false;
  if (c.classes.length) {
    const cls = classesOf(el);
    if (!c.classes.every((k) => cls.includes(k))) return false;
  }
  for (const a of c.attrs) {
    if (!(a.name in el.attrs)) return false;
    if (a.value !== undefined && el.attrs[a.name] !== a.value) return false;
  }
  return c.pseudo.every((p) => p(el, x.index, x.count));
}

function matches(s: Selector, x: Ctx): boolean {
  const match = (i: number, node: Ctx | undefined): boolean => {
    if (!node || !matchesCompound(s.parts[i]!, node)) return false;
    if (i === 0) return true;
    if (s.combinators[i - 1] === '>') return match(i - 1, node.parent);
    for (let up = node.parent; up; up = up.parent) if (match(i - 1, up)) return true;
    return false;
  };
  return match(s.parts.length - 1, x);
}

/** Expand inherited CSS custom properties. Cycles and missing variables invalidate only that declaration. */
function expandVariables(value: string, vars: Map<string, string>, stack: Set<string> = new Set()): string | undefined {
  let out = '';
  for (let i = 0; i < value.length;) {
    const at = value.indexOf('var(', i);
    if (at < 0) return out + value.slice(i);
    out += value.slice(i, at);
    let depth = 1;
    let end = at + 4;
    for (; end < value.length && depth; end++) {
      if (value[end] === '(') depth++;
      else if (value[end] === ')') depth--;
    }
    if (depth) return undefined;
    const body = value.slice(at + 4, end - 1);
    let comma = -1;
    let innerDepth = 0;
    for (let j = 0; j < body.length; j++) {
      if (body[j] === '(') innerDepth++;
      else if (body[j] === ')') innerDepth--;
      else if (body[j] === ',' && innerDepth === 0) { comma = j; break; }
    }
    const name = (comma < 0 ? body : body.slice(0, comma)).trim();
    if (!/^--[\w-]+$/.test(name) || stack.has(name)) return undefined;
    const raw = vars.get(name) ?? (comma < 0 ? undefined : body.slice(comma + 1).trim());
    if (raw === undefined) return undefined;
    const next = new Set(stack);
    next.add(name);
    const expanded = expandVariables(raw, vars, next);
    if (expanded === undefined) return undefined;
    out += expanded;
    i = end;
  }
  return out;
}

function resolvedStyle(style: string, inherited: Map<string, string>): { style: string; vars: Map<string, string> } {
  const vars = new Map(inherited);
  const decls = declarations(style);
  for (const [name, value] of decls) if (name.startsWith('--')) vars.set(name, value);
  const resolved: string[] = [];
  for (const [name, value] of decls) {
    if (name.startsWith('--')) continue;
    const expanded = expandVariables(value, vars);
    if (expanded !== undefined) resolved.push(`${name}:${expanded}`);
  }
  return { style: resolved.join(';'), vars };
}

function generatedContent(value: string | undefined, el: El): string | undefined {
  if (!value || /^(none|normal)$/i.test(value.trim())) return undefined;
  const pieces: string[] = [];
  const token = /"((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)'|attr\(\s*([\w-]+)\s*\)/gi;
  for (const m of value.matchAll(token)) {
    if (m[3]) pieces.push(el.attrs[m[3].toLowerCase()] ?? '');
    else pieces.push((m[1] ?? m[2] ?? '').replace(/\\([\\"'])/g, '$1'));
  }
  return pieces.length ? pieces.join('') : undefined;
}

/** Write cascaded declarations into each element, resolve variables, and materialize generated content. */
export function applyStylesheet(children: Child[], sheet: Stylesheet, parent?: Ctx, inheritedVars: Map<string, string> = new Map()): void {
  // HTML fragments have an implicit document root. Make :root custom properties available to every
  // top-level element even when the caller did not include an explicit <html> element.
  if (!parent) {
    type RootHit = { decl: [string, string, boolean]; spec: number; order: number };
    const roots: RootHit[] = [];
    for (const rule of sheet.rules) {
      const spec = Math.max(-1, ...rule.selectors.filter((s) => !s.pseudoElement && s.parts.length === 1 && s.parts[0]!.root).map((s) => s.specificity));
      if (spec >= 0) for (const decl of rule.declarations) if (decl[0].startsWith('--')) roots.push({ decl, spec, order: rule.order });
    }
    roots.sort((a, b) => Number(a.decl[2]) - Number(b.decl[2]) || a.spec - b.spec || a.order - b.order);
    inheritedVars = new Map(inheritedVars);
    for (const { decl } of roots) inheritedVars.set(decl[0], decl[1]);
  }
  const index = indexOf(sheet);
  const els = children.filter((c): c is El => typeof c !== 'string');
  els.forEach((el, i) => {
    const ctx: Ctx = { el, index: i + 1, count: els.length, parent };
    type Hit = { decl: [string, string, boolean]; spec: number; order: number };
    const hits: Hit[] = [];
    const generated: Record<'before' | 'after', Hit[]> = { before: [], after: [] };
    // per rule and pseudo-element, the highest specificity among its matching selectors
    const best = new Map<string, { rule: Rule; kind: 'before' | 'after' | undefined; spec: number }>();
    for (const { rule, selector: s } of candidates(el, index)) {
      const key = `${rule.order}:${s.pseudoElement ?? ''}`;
      const seen = best.get(key);
      if (seen && seen.spec >= s.specificity) continue;
      if (matches(s, ctx)) best.set(key, { rule, kind: s.pseudoElement, spec: s.specificity });
    }
    for (const { rule, kind, spec } of best.values()) {
      for (const d of rule.declarations) (kind ? generated[kind] : hits).push({ decl: d, spec, order: rule.order });
    }
    if (hits.length) {
      hits.sort((a, b) => a.spec - b.spec || a.order - b.order);
      const normal = hits.filter((h) => !h.decl[2]).map((h) => `${h.decl[0]}:${h.decl[1]}`);
      const important = hits.filter((h) => h.decl[2]).map((h) => `${h.decl[0]}:${h.decl[1]}`);
      el.attrs.style = [...normal, el.attrs.style ?? '', ...important].filter(Boolean).join(';');
    }
    const own = resolvedStyle(el.attrs.style ?? '', inheritedVars);
    el.attrs.style = own.style;

    const pseudo = (kind: 'before' | 'after'): El | undefined => {
      const list = generated[kind];
      if (!list.length) return undefined;
      list.sort((a, b) => a.spec - b.spec || a.order - b.order);
      const normal = list.filter((h) => !h.decl[2]).map((h) => `${h.decl[0]}:${h.decl[1]}`);
      const important = list.filter((h) => h.decl[2]).map((h) => `${h.decl[0]}:${h.decl[1]}`);
      const r = resolvedStyle([...normal, ...important].join(';'), own.vars);
      const content = generatedContent(declarations(r.style).find(([k]) => k === 'content')?.[1], el);
      if (content === undefined) return undefined;
      const style = declarations(r.style).filter(([k]) => k !== 'content').map(([k, v]) => `${k}:${v}`).join(';');
      return { tag: 'span', attrs: { ...(style ? { style } : {}), 'aria-hidden': 'true' }, children: [content] };
    };
    const before = pseudo('before');
    const after = pseudo('after');
    if (before) el.children.unshift(before);
    if (after) el.children.push(after);
    applyStylesheet(el.children, sheet, ctx, own.vars);
  });
}

/** Every `<style>` element's text, in document order. */
export function styleElements(children: Child[], out: string[] = []): string[] {
  for (const c of children) {
    if (typeof c === 'string') continue;
    if (c.tag === 'style') out.push(c.children.filter((x): x is string => typeof x === 'string').join(''));
    else styleElements(c.children, out);
  }
  return out;
}

/** `href`s of `<link rel="stylesheet">` elements. */
export function stylesheetLinks(children: Child[], out: string[] = []): string[] {
  for (const c of children) {
    if (typeof c === 'string') continue;
    if (c.tag === 'link' && /\bstylesheet\b/i.test(c.attrs.rel ?? '') && c.attrs.href) out.push(c.attrs.href);
    else stylesheetLinks(c.children, out);
  }
  return out;
}
