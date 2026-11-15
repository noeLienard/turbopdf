import { createElement as h } from 'react';
import { describe, expect, it } from 'vitest';
import { Cell, Document, Heading, Image, Page, PageBreak, PageNumber, Row, Span, Table, Text, View } from '../src/components';
import papier, { PapierError } from '../src/index';
import { parseCss } from '../src/rust/css';
import { toTree } from '../src/rust/engine';
import type { Document as Tree } from '../src/tree';
import { pageTexts } from './helpers';

const PNG_1X1 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
const producer = (pdf: Buffer) => pdf.toString('latin1').match(/\/Producer\s*\(([^)]*)\)/)?.[1];
const squash = (s: string) => s.replace(/\s+/g, '');

describe('one engine, no browser', () => {
  it('every source is rendered by the Rust engine', async () => {
    for (const input of [h(Text, null, 'Bonjour'), { markdown: '# Bonjour' }, '<p>Bonjour</p>'] as const) {
      expect(producer((await papier.create(input as never)).toBuffer())).toBe('Papier');
    }
  });

  it('the package does not depend on a browser', async () => {
    const pkg = JSON.parse(await import('node:fs').then((fs) => fs.readFileSync(new URL('../package.json', import.meta.url), 'utf8'))) as { dependencies: Record<string, string> };
    expect(Object.keys(pkg.dependencies)).not.toContain('playwright');
  });
});

describe('HTML + CSS cascade', () => {
  it('applies <style> rules: classes, ids, descendants, nth-child, specificity, !important', async () => {
    const html = `<style>
      .card { padding: 8px; background: #eff6ff }
      #main .card p { color: blue }
      tr:nth-child(even) td { background: #f3f4f6 }
      p { color: red }
      .loud { font-weight: bold !important }
    </style>
    <div id="main"><div class="card"><p class="loud" style="font-weight:400">A</p></div></div>
    <table><tr><td>1</td></tr><tr><td>2</td></tr></table>`;
    const [main, table] = (await toTree({ html })).children[0]!.children;
    expect(main).toMatchObject({ id: 'main', children: [{ style: { padding: 6, backgroundColor: '#eff6ff' }, children: [{ type: 'text', style: { color: '#0000ff', fontWeight: 700 } }] }] });
    expect(table).toMatchObject({ rows: [{ cells: [{}] }, { cells: [{ style: { backgroundColor: '#f3f4f6' } }] }] });
  });

  it('resolves inherited CSS variables, fallbacks and nested references', async () => {
    const html = `<style>
      :root { --brand: #2563eb; --space: 8px; --double: calc(var(--space) * 2) }
      .card { color: var(--brand); padding: var(--double); margin-bottom: var(--missing, 1rem) }
      .override { --brand: #dc2626; color: var(--brand) }
    </style><div class="card"><p>A</p><p class="override">B</p></div>`;
    const root = (await toTree({ html })).children[0]!.children[0]!;
    expect(root).toMatchObject({ style: { color: '#2563eb', padding: 12, margin: { bottom: 11 } } });
    expect(root.type === 'view' && root.children[1]).toMatchObject({ style: { color: '#dc2626' } });
  });

  it('materializes ::before and ::after generated content', async () => {
    const tree = await toTree({
      html: '<style>.price::before{content:"€ ";font-weight:bold}.price::after{content:" " attr(data-unit);color:blue}</style><p class="price" data-unit="TTC">42</p>',
    });
    expect(tree.children[0]!.children[0]).toMatchObject({
      type: 'text',
      content: [
        { text: '€ ', style: { fontWeight: 700 } },
        { text: '42' },
        { text: ' TTC', style: { color: '#0000ff' } },
      ],
    });
  });

  it('@page sets size and margins, explicit options win', async () => {
    const html = '<style>@page { size: A5 landscape; margin: 1cm }</style><p>x</p>';
    expect((await toTree({ html })).children[0]).toMatchObject({ size: 'A5', orientation: 'landscape', margin: 72 / 2.54 });
    expect((await toTree({ html, format: 'A4', margin: '0' })).children[0]).toMatchObject({ size: 'A4', orientation: 'landscape', margin: { top: 0 } });
  });

  it('<title> and <html lang> fill the metadata', async () => {
    const tree = await toTree({ html: '<html lang="fr"><head><title>Facture 42</title></head><body><p>x</p></body></html>' });
    expect(tree.meta).toEqual({ lang: 'fr', title: 'Facture 42' });
  });

  it('CSS pagination: break-before / break-after / break-inside / display:none', async () => {
    const pdf = await papier.create('<p>UN</p><p style="break-before:page">DEUX</p><p style="page-break-after:always">TROIS</p><p>QUATRE</p><p style="display:none">CACHÉ</p>');
    const t = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(3);
    expect(t[1]).toContain('TROIS');
    expect(t[2]).toContain('QUATRE');
    expect(t.join('')).not.toContain('CACHÉ');
  });

  it('maps CSS Grid and advanced flex sizing to the native layout tree', async () => {
    const html = `<div class="grid" style="display:grid;grid-template-columns:repeat(2, 1fr) 72pt;gap:8px 16px;aspect-ratio:2">
      <span>A</span><span>B</span><span>C</span>
    </div>`;
    const tree = await toTree({ html });
    expect(tree.children[0]!.children[0]).toMatchObject({
      type: 'view',
      style: {
        display: 'grid',
        gridTemplateColumns: ['1fr', '1fr', '72pt'],
        rowGap: 6,
        columnGap: 12,
        aspectRatio: 2,
      },
      children: [{ content: 'A' }, { content: 'B' }, { content: 'C' }],
    });
    await expect(papier.create({ tree })).resolves.toMatchObject({ pages: 1, warnings: [] });
  });

  it('local stylesheets are read from baseDir, never outside', async () => {
    const { mkdtempSync, symlinkSync, writeFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const { tmpdir } = await import('node:os');
    const dir = mkdtempSync(join(tmpdir(), 'papier-css-'));
    writeFileSync(join(dir, 'style.css'), 'p { color: #00ff00 }');
    const tree = await toTree({ html: '<link rel="stylesheet" href="style.css"><p>x</p>', baseDir: dir });
    expect(tree.children[0]!.children[0]).toMatchObject({ style: { color: '#00ff00' } });
    const pdf = await papier.create({ html: '<link rel="stylesheet" href="../../etc/x.css"><p>x</p>', baseDir: dir });
    expect(pdf.warnings[0]).toMatchObject({ type: 'stylesheet_failed', message: expect.stringContaining('outside baseDir') });
    const outside = mkdtempSync(join(tmpdir(), 'papier-css-outside-'));
    writeFileSync(join(outside, 'secret.css'), 'p { color: #ff0000 }');
    symlinkSync(join(outside, 'secret.css'), join(dir, 'linked.css'));
    const linked = await papier.create({ html: '<link rel="stylesheet" href="linked.css"><p>x</p>', baseDir: dir });
    expect(linked.warnings[0]).toMatchObject({ type: 'stylesheet_failed', message: expect.stringContaining('outside baseDir') });
  });
});

describe('components', () => {
  it('document, pages, header / footer and page numbers', async () => {
    const pdf = await papier.create(
      h(Document, { title: 'Rapport', lang: 'fr' },
        h(Page, { header: h(Text, null, 'En-tête {page}/{pages}'), footer: h(PageNumber, { format: 'Page {page} / {pages}' }) },
          h(Heading, null, 'Un'), h(PageBreak), h(Heading, { level: 2 }, 'Deux'))),
    );
    expect(pdf.pages).toBe(2);
    const t = pageTexts(pdf.toBuffer());
    expect(t[0]).toContain('En-tête 1/2');
    expect(t[1]).toContain('Page 2 / 2');
    expect(t[1]).toContain('Deux');
    expect(pdf.toBuffer().toString('latin1')).toContain('Rapport');
  });

  it('a long table repeats its header and never splits a row', async () => {
    const rows = Array.from({ length: 150 }, (_, i) => h(Row, { key: i }, h(Cell, null, h(Text, null, `R${i}x`)), h(Cell, null, h(Text, null, `T${i}y`))));
    const pdf = await papier.create(
      h(Table, { columns: [{ width: 'auto' }, { width: 80, align: 'right' }], cellStyle: { padding: 4 } }, h(Row, { header: true }, h(Cell, null, h(Text, null, 'Article')), h(Cell, null, h(Text, null, 'Prix'))), rows),
    );
    expect(pdf.pages).toBeGreaterThan(2);
    const pages = pageTexts(pdf.toBuffer()).map(squash);
    for (const p of pages) expect(p.startsWith('ArticlePrix')).toBe(true);
    for (let i = 0; i < 150; i++) {
      const where = pages.flatMap((p, n) => (p.includes(`R${i}xT${i}y`) ? [n] : []));
      expect(where, `row ${i}`).toHaveLength(1);
    }
  });

  it('spans, views, images and keepTogether', async () => {
    const tree = await toTree({
      react: h(View, { style: { flexDirection: 'row', gap: 8 }, keepTogether: true },
        h(Text, null, 'Du texte ', h(Span, { style: { fontWeight: 700 } }, 'gras'), ' ici.'),
        h(Image, { src: `data:image/png;base64,${PNG_1X1}`, style: { width: 20, height: 20 } })),
    });
    const view = tree.children[0]!.children[0]!;
    expect(view).toMatchObject({ type: 'view', keepTogether: true, style: { flexDirection: 'row', gap: 8 } });
    expect(view.type === 'view' && view.children).toEqual([
      { type: 'text', content: [{ text: 'Du texte ' }, { text: 'gras', style: { fontWeight: 700 } }, { text: ' ici.' }] },
      { type: 'image', src: { data: PNG_1X1 }, format: 'png', style: { width: 20, height: 20 } },
    ]);
    const pdf = await papier.create({ tree });
    expect(pdf.warnings).toEqual([]);
  });
});

describe('plain React (HTML + inline styles)', () => {
  it('maps common HTML to the tree', async () => {
    const tree = await toTree({
      react: h('div', { style: { display: 'flex', padding: 8, fontSize: '12pt' } },
        h('h2', { style: { color: 'red' } }, 'Titre'),
        h('p', null, 'ligne 1', h('br'), 'ligne ', h('b', null, 'deux')),
        h('table', null, h('thead', null, h('tr', null, h('th', null, 'A'), h('th', null, 'B'))), h('tbody', null, h('tr', null, h('td', { colSpan: 2 }, 'AB'))))),
    });
    const root = tree.children[0]!.children[0]!;
    expect(root).toMatchObject({ type: 'view', style: { flexDirection: 'row', padding: 6, fontSize: 12 } });
    const [heading, para, table] = root.type === 'view' ? root.children : [];
    expect(heading).toMatchObject({ type: 'heading', level: 2, style: { color: '#ff0000' }, content: 'Titre' });
    expect(para).toMatchObject({ type: 'text', content: [{ text: 'ligne 1\nligne ' }, { text: 'deux', style: { fontWeight: 700 } }] });
    expect(table).toMatchObject({ type: 'table', columns: [{ width: 'auto' }, { width: 'auto' }], header: [{ cells: [{ style: { fontWeight: 700 } }, {}] }], rows: [{ cells: [{ colSpan: 2 }] }] });
  });

  it('parses inline CSS units and shorthands to points', () => {
    expect(parseCss('padding: 8px 16px; margin-top: 1cm; border: 1px solid #ccc; width: 50%; line-height: 18px; font-size: 12px')).toEqual({
      padding: { top: 6, right: 12, bottom: 6, left: 12 },
      margin: { top: 72 / 2.54 },
      border: { width: 0.75, color: '#ccc', style: 'solid' },
      width: '50%',
      lineHeight: 1.5,
      fontSize: 9,
    });
    expect(parseCss('background: rgba(255, 0, 0, 0.5); font-weight: bold; flex: 1')).toEqual({ backgroundColor: '#ff000080', fontWeight: 700, flexGrow: 1 });
    expect(parseCss('display:flex;align-content:space-between;align-self:center;gap:4pt 8pt;position:relative;left:2pt;aspect-ratio:16/9')).toMatchObject({
      display: 'flex', alignContent: 'space-between', alignSelf: 'center', rowGap: 4, columnGap: 8,
      position: 'relative', left: 2, aspectRatio: 16 / 9,
    });
  });

  it('never fetches remote images: warns instead', async () => {
    const pdf = await papier.create(h('img', { src: 'https://example.com/logo.png' }));
    expect(pdf.warnings[0]).toMatchObject({ type: 'image_failed', message: expect.stringContaining('never uses the network') });
  });
    expect(parseCss('width:calc(2in - 12pt); height:min(50vh, 300pt); padding:clamp(4pt, 1rem, 20pt)')).toMatchObject({
      width: 132,
      height: 300,
      padding: 11,
    });
});

describe('markdown', () => {
  it('headings, lists, tables, code and page breaks', async () => {
    const md = ['# Titre', '', '- un', '- deux', '', '| A | B |', '|---|--:|', '| 1 | 2 |', '', '```', 'if (x) {', '    y();', '}', '```', '', '<!-- pagebreak -->', '', 'fin'].join('\n');
    const pdf = await papier.create({ markdown: md });
    expect(pdf.pages).toBe(2);
    const [p1, p2] = pageTexts(pdf.toBuffer());
    expect(p1).toContain('Titre');
    expect(p1).toMatch(/•\s*un/);
    expect(p1).toContain('if (x) {');
    expect(p1).toMatch(/\u00a0{4}y\(\);/); // indentation kept
    expect(p2).toContain('fin');
    const tree = await toTree({ markdown: md });
    const table = tree.children[0]!.children.find((n) => n.type === 'table');
    expect(table).toMatchObject({ columns: [{ width: 'auto' }, { width: 'auto', align: 'right' }] });
  });
});

describe('results', () => {
  it('previews come from the engine, at the requested scale', async () => {
    const pdf = await papier.create(h(Text, null, 'Aperçu'));
    const [page] = await pdf.toPng({ scale: 1 });
    expect(page!.width).toBe(596); // 595.28pt rounded up: the whole page is covered
    expect(page!.height).toBe(842);
    expect(page!.toBuffer().subarray(1, 4).toString()).toBe('PNG');
    await expect(pdf.toPng({ scale: 0 })).rejects.toThrow(/Invalid PNG scale/);
  });

  it('the same input gives the same bytes', async () => {
    const make = () => papier.create({ markdown: '# Même\n\nentrée' });
    expect((await make()).toBuffer().equals((await make()).toBuffer())).toBe(true);
  });

  it('engine warnings are explained, and strict turns them into an error', async () => {
    const pdf = await papier.create(h(Text, { id: 'prix' }, 'prix 漢'));
    expect(pdf.warnings).toEqual([expect.objectContaining({ type: 'missing_glyph', path: 'prix', page: 1, message: expect.stringContaining('U+6F22') })]);
    const err = await papier.create({ react: h(Text, null, '漢'), strict: true }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(PapierError);
    expect((err as PapierError).code).toBe('warnings_found');
  });

  it('an invalid tree lists every problem with its path', async () => {
    const tree = { type: 'document', version: 1, children: [{ type: 'page', children: [{ type: 'heading', level: 9, content: 'x' }] }] } as Tree;
    const err = (await papier.create({ tree }).catch((e: unknown) => e)) as PapierError;
    expect(err.code).toBe('invalid_tree');
    expect(err.details).toEqual([expect.objectContaining({ path: '/children/0/children/0/level', code: 'invalid_heading_level' })]);
  });

  it('renders a 200-row table quickly', async () => {
    const rows = Array.from({ length: 200 }, (_, i) => h(Row, { key: i }, h(Cell, null, h(Text, null, `Ligne ${i}`)), h(Cell, null, h(Text, null, String(i * 3)))));
    const doc = h(Table, { columns: [{ width: 'auto' }, { width: 60 }] }, rows);
    await papier.create(doc); // warm (module load)
    const t = performance.now();
    await papier.create(doc);
    expect(performance.now() - t).toBeLessThan(250);
  });
});
