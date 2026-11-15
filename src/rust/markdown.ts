// Markdown → document tree nodes (for the Rust engine), from markdown-it tokens.
import MarkdownIt from 'markdown-it';
import type { Cell, Column, Node, Row, Span, TextContent, TextStyle } from '../tree';
import { blocks, keepSpaces, parseMarkup } from './markup';

const md = new MarkdownIt({ html: true, linkify: true, breaks: false });
// `![Billet](qr:https://…)` is a QR code: keep its text exactly as written (no percent-encoding)
const normalizeLink = md.normalizeLink.bind(md);
md.normalizeLink = (url) => (/^qr:/i.test(url) ? url : normalizeLink(url));
type Token = ReturnType<typeof md.parse>[number];

const PAGEBREAK = /^<!--\s*pagebreak\s*-->\s*$/i;
const TOC = /^<!--\s*toc\s*-->\s*$/i;

/** GitHub-style heading anchor: `## Mes Tarifs 2026` → `mes-tarifs-2026`, so `[voir](#mes-tarifs-2026)` works. */
export function slug(text: string, used: Map<string, number>): string {
  const base = text.trim().toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu, '').replace(/\s/g, '-') || 'section';
  const n = used.get(base) ?? 0;
  used.set(base, n + 1);
  return n ? `${base}-${n}` : base;
}
const CODE: TextStyle = { color: '#be185d' };

/** markdown-it percent-encodes hrefs: `#résumé` arrives as `#r%C3%A9sum%C3%A9`; ids are plain text. */
function linkHref(href: string): string | undefined {
  if (!href) return undefined;
  if (!href.startsWith('#')) return href;
  try {
    return decodeURIComponent(href);
  } catch {
    return href;
  }
}

/** Inline tokens → text content. */
function inline(tokens: Token[] = []): TextContent {
  const out: Span[] = [];
  const stack: TextStyle[] = [{}];
  const links: (string | undefined)[] = [];
  const style = () => stack[stack.length - 1]!;
  const push = (text: string, extra?: TextStyle) => {
    const s = { ...style(), ...extra };
    const clean = Object.keys(s).length ? s : undefined;
    const link = links[links.length - 1];
    const last = out[out.length - 1];
    if (last && JSON.stringify(last.style) === JSON.stringify(clean) && last.link === link) last.text += text;
    else out.push({ text, ...(clean ? { style: clean } : {}), ...(link ? { link } : {}) });
  };
  for (const t of tokens) {
    switch (t.type) {
      case 'text':
        push(t.content);
        break;
      case 'code_inline':
        push(t.content, CODE);
        break;
      case 'softbreak':
        push(' ');
        break;
      case 'hardbreak':
        push('\n');
        break;
      case 'strong_open':
        stack.push({ ...style(), fontWeight: 700 });
        break;
      case 'em_open':
        stack.push({ ...style(), fontStyle: 'italic' });
        break;
      case 'link_open':
        stack.push({ ...style(), color: '#2563eb' });
        links.push(linkHref(String(t.attrGet('href') ?? '')));
        break;
      case 's_open':
        stack.push({ ...style() });
        break;
      case 'link_close':
        links.pop();
        if (stack.length > 1) stack.pop();
        break;
      case 'strong_close':
      case 'em_close':
      case 's_close':
        if (stack.length > 1) stack.pop();
        break;
      case 'image':
        push(`[${t.content || 'image'}]`);
        break;
    }
  }
  if (out.length === 1 && !out[0]!.style && !out[0]!.link) return out[0]!.text;
  return out;
}

const plain = (c: TextContent) => (typeof c === 'string' ? c : c.map((s) => s.text).join(''));

/** Index of the token closing the one at `i`. */
function closing(tokens: Token[], i: number): number {
  let depth = 0;
  for (let j = i; j < tokens.length; j++) {
    depth += tokens[j]!.nesting;
    if (depth === 0) return j;
  }
  return tokens.length - 1;
}

/** Code block: indentation kept (non-breaking spaces), no syntax colours. */
function codeBlock(text: string): Node {
  return {
    type: 'view',
    style: { backgroundColor: '#f3f4f6', padding: 8, borderRadius: 4, margin: { bottom: 8 } },
    children: [{ type: 'text', role: 'code', style: { fontSize: 9 }, content: keepSpaces(text.replace(/\n$/, '')) || ' ' }],
  };
}

function table(tokens: Token[]): Node {
  const header: Row[] = [];
  const rows: Row[] = [];
  const align: Column['align'][] = [];
  let section: Row[] = rows;
  let row: Cell[] | undefined;
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]!;
    if (t.type === 'thead_open') section = header;
    else if (t.type === 'tbody_open') section = rows;
    else if (t.type === 'tr_open') row = [];
    else if (t.type === 'tr_close' && row) section.push({ cells: row });
    else if ((t.type === 'th_open' || t.type === 'td_open') && row) {
      const a = /text-align:(left|center|right)/.exec(String(t.attrGet('style') ?? ''))?.[1] as Column['align'];
      if (section === header) align.push(a);
      const c = inline(tokens[i + 1]?.children ?? []);
      row.push({ children: [{ type: 'text', content: c, ...(t.type === 'th_open' ? { style: { fontWeight: 700 } } : {}) }] });
    }
  }
  const count = Math.max(1, ...[...header, ...rows].map((r) => r.cells.length));
  const columns: Column[] = Array.from({ length: count }, (_, i) => (align[i] ? { width: 'auto', align: align[i] } : { width: 'auto' }));
  return {
    type: 'table',
    columns,
    ...(header.length ? { header } : {}),
    rows,
    headerStyle: { backgroundColor: '#f3f4f6' },
    cellStyle: { padding: { top: 3, right: 5, bottom: 3, left: 5 }, border: { width: 0.5, color: '#e5e7eb' } },
    style: { margin: { bottom: 8 } },
  };
}

function walk(tokens: Token[], from: number, to: number, ids: Map<string, number>): Node[] {
  const out: Node[] = [];
  for (let i = from; i < to; i++) {
    const t = tokens[i]!;
    switch (t.type) {
      case 'heading_open': {
        const level = Number(t.tag.slice(1));
        const content = inline(tokens[i + 1]?.children ?? []);
        out.push({ type: 'heading', id: slug(plain(content), ids), level, content });
        i = closing(tokens, i);
        break;
      }
      case 'paragraph_open': {
        const children = tokens[i + 1]?.children ?? [];
        const only = children.filter((c) => !(c.type === 'text' && !c.content.trim()) && c.type !== 'softbreak');
        if (only.length === 1 && only[0]!.type === 'image') {
          const src = String(only[0]!.attrGet('src') ?? '');
          const attr = (v: string) => v.replace(/&/g, '&amp;').replace(/"/g, '&quot;');
          out.push(...blocks(parseMarkup(`<img src="${attr(src)}" alt="${attr(only[0]!.content)}">`)));
        } else if (!t.hidden) {
          out.push({ type: 'text', style: { margin: { bottom: 8 } }, content: inline(children) });
        } else {
          out.push({ type: 'text', content: inline(children) }); // tight list item
        }
        i = closing(tokens, i);
        break;
      }
      case 'bullet_list_open':
      case 'ordered_list_open': {
        const end = closing(tokens, i);
        let n = Number(t.attrGet('start') ?? 1);
        const items: Node[] = [];
        for (let j = i + 1; j < end; j++) {
          if (tokens[j]!.type !== 'list_item_open') continue;
          const close = closing(tokens, j);
          items.push({
            type: 'view',
            role: 'list_item',
            style: { flexDirection: 'row', margin: { bottom: 2 } },
            children: [
              { type: 'text', role: 'list_label', style: { width: 14, flexShrink: 0 }, content: t.type === 'ordered_list_open' ? `${n++}.` : '•' },
              { type: 'view', role: 'list_body', style: { flexGrow: 1, flexShrink: 1 }, children: walk(tokens, j + 1, close, ids) },
            ],
          });
          j = close;
        }
        out.push({ type: 'view', role: t.type === 'ordered_list_open' ? 'ordered_list' : 'list', style: { margin: { bottom: 8 } }, children: items });
        i = end;
        break;
      }
      case 'blockquote_open': {
        const end = closing(tokens, i);
        out.push({
          type: 'view',
          role: 'blockquote',
          style: { padding: { top: 4, right: 8, bottom: 4, left: 10 }, margin: { bottom: 8 }, color: '#4b5563', backgroundColor: '#f9fafb' },
          children: walk(tokens, i + 1, end, ids),
        });
        i = end;
        break;
      }
      case 'fence':
      case 'code_block':
        out.push(codeBlock(t.content));
        break;
      case 'hr':
        out.push({ type: 'view', role: 'artifact', style: { height: 0.75, backgroundColor: '#e5e7eb', margin: { top: 8, bottom: 8 } }, children: [] });
        break;
      case 'table_open': {
        const end = closing(tokens, i);
        out.push(table(tokens.slice(i, end + 1)));
        i = end;
        break;
      }
      case 'html_block':
        if (PAGEBREAK.test(t.content.trim())) out.push({ type: 'page_break' });
        else if (TOC.test(t.content.trim())) out.push({ type: 'table_of_contents' });
        else out.push(...blocks(parseMarkup(t.content)));
        break;
    }
  }
  return out;
}

/** Markdown source → body nodes. `<!-- pagebreak -->` forces a page break. */
export function markdownToNodes(source: string): Node[] {
  const tokens = md.parse(source, {});
  return walk(tokens, 0, tokens.length, new Map());
}
