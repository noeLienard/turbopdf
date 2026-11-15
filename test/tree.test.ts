import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { Document, Node, Warning } from '../src/tree';

const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../engine/fixtures/${name}`, import.meta.url), 'utf8'));

describe('document tree types (generated from Rust)', () => {
  it('a typed tree equals the Rust fixture hello.json', () => {
    const doc: Document = {
      type: 'document',
      version: 1,
      meta: { title: 'Hello', lang: 'en' },
      children: [
        {
          type: 'page',
          children: [
            { type: 'heading', level: 1, content: 'Hello Papier' },
            { type: 'text', content: 'What you see is what you download.' },
          ],
        },
      ],
    };
    expect(doc).toEqual(fixture('hello.json'));
  });

  it('covers every node type', () => {
    const nodes: Node[] = [
      { type: 'view', children: [] },
      { type: 'text', content: [{ text: 'a', style: { fontWeight: 700 } }] },
      { type: 'heading', level: 2, content: 'h' },
      { type: 'image', src: { data: 'AAAA' }, fit: 'cover', style: { width: '50%', height: 'auto' } },
      { type: 'table', columns: [{ width: 120 }, {}], rows: [{ cells: [{ children: [] }, { colSpan: 1, children: [] }] }] },
      { type: 'page_break' },
      { type: 'page_number', format: 'Page {page} / {pages}' },
    ];
    expect(nodes.map((n) => n.type)).toEqual(['view', 'text', 'heading', 'image', 'table', 'page_break', 'page_number']);
  });

  it('models every warning', () => {
    const warnings: Warning[] = [
      { type: 'overflow', path: '/children/0', page: 1, dx: 12, dy: 0 },
      { type: 'moved_to_next_page', path: '/children/0/children/4', fromPage: 1, toPage: 2, reason: 'keep_together' },
      { type: 'missing_glyph', path: '/x', page: 1, character: '𓀀', codepoint: 0x13000, fontFamily: 'Inter' },
      { type: 'image_failed', path: '/y', reason: 'unsupported format' },
    ];
    expect(warnings).toHaveLength(4);
  });

  it('rejects a typo at compile time', () => {
    // @ts-expect-error `fontsize` is not a style property
    const bad: Node = { type: 'text', content: 'x', style: { fontsize: 12 } };
    expect(bad).toBeDefined();
  });
});
