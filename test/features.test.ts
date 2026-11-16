import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { describe, expect, it } from 'vitest';
import { Document, Heading, Image, Link, Page, PageBreak, PageNumber, QR, TableOfContents, Text, View } from '../src/components';
import turbopdf from '../src/index';
import { toTree } from '../src/rust/engine';
import { FONT_GLYPH, FONT_PATH, firstTextLeft, outline, pageLinks, pageTexts, pdfHas, pixelAt } from './helpers';

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const WEBP_1X1 = 'UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==';
const GIF_1X1 = 'R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==';
const SVG = '<svg width="120" height="40" viewBox="0 0 120 40"><rect width="120" height="40" fill="#2563eb"/></svg>';

const near = (got: number[], want: number[], tol = 12) => got.forEach((v, i) => expect(Math.abs(v - want[i]!), `channel ${i}: ${got} vs ${want}`).toBeLessThanOrEqual(tol));
const tmp = () => mkdtempSync(join(tmpdir(), 'turbopdf-feat-'));

describe('links', () => {
  it('<a href> to a URL and to an #id are clickable', async () => {
    const pdf = await turbopdf.create(
      '<p>Voir <a href="https://example.com/doc">le site</a> et <a href="#annexe">l\'annexe</a>.</p><h2 id="annexe" style="break-before:page">Annexe</h2>',
    );
    const [first] = pageLinks(pdf.toBuffer());
    expect(first).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ uri: 'https://example.com/doc', external: true }),
        expect.objectContaining({ external: false, target: 1 }),
      ]),
    );
    expect(pdf.warnings).toEqual([]);
  });

  it('a link around a block or an image makes the whole box clickable', async () => {
    const tree = await toTree({
      html: `<a href="https://example.com"><div class="card"><h3>Offre</h3><p>Détails</p></div></a><a href="https://logo.example"><img src="data:image/gif;base64,${GIF_1X1}" width="20" height="20"></a>`,
    });
    const [card, logo] = tree.children[0]!.children;
    expect(card).toMatchObject({ type: 'view', link: 'https://example.com' });
    expect(logo).toMatchObject({ type: 'image', link: 'https://logo.example', format: 'gif' });
  });

  it('a link to a missing id is a broken_link warning; javascript: links are never clickable', async () => {
    const pdf = await turbopdf.create('<p><a href="#nope">perdu</a> <a href="javascript:alert(1)">script</a></p>');
    expect(pdf.warnings).toEqual([expect.objectContaining({ type: 'broken_link', path: '#nope', page: 1, message: expect.stringContaining('id="nope"') })]);
    expect(pageLinks(pdf.toBuffer())[0]).toEqual([]);
  });

  it('Markdown links work, including #anchors to headings (GitHub-style ids)', async () => {
    const pdf = await turbopdf.create({ markdown: '[le site](https://example.com) · [les tarifs](#mes-tarifs-2026)\n\n<!-- pagebreak -->\n\n## Mes Tarifs 2026\n' });
    const links = pageLinks(pdf.toBuffer())[0]!;
    expect(links.map((l) => l.external)).toEqual([true, false]);
    expect(links[1]!.target).toBe(1);
    expect(pdf.warnings).toEqual([]);
  });

  it('Markdown links to accented headings work (markdown-it percent-encodes them)', async () => {
    const pdf = await turbopdf.create({ markdown: '[voir](#résumé-détaillé)\n\n<!-- pagebreak -->\n\n## Résumé détaillé\n' });
    expect(pdf.warnings).toEqual([]);
    expect(pageLinks(pdf.toBuffer())[0]![0]!.target).toBe(1);
  });
});

describe('bookmarks and table of contents', () => {
  const report = '<h1>Rapport</h1><h2>Ventes</h2><h2 style="break-before:page">Coûts</h2><h3>Détail</h3><h4>trop fin</h4>';

  it('h1–h3 become nested PDF bookmarks', async () => {
    const pdf = await turbopdf.create(report);
    expect(outline(pdf.toBuffer())).toEqual([
      {
        title: 'Rapport',
        page: 0,
        children: [
          { title: 'Ventes', page: 0, children: [] },
          { title: 'Coûts', page: 1, children: [{ title: 'Détail', page: 1, children: [] }] },
        ],
      },
    ]);
  });

  it('bookmarks: false turns them off', async () => {
    expect(outline((await turbopdf.create({ html: report, bookmarks: false })).toBuffer())).toEqual([]);
  });

  it('<!-- toc --> in Markdown: headings with page numbers, each line linked', async () => {
    const pdf = await turbopdf.create({ markdown: '# Sommaire\n\n<!-- toc -->\n\n<!-- pagebreak -->\n\n## Introduction\n\ntexte\n\n<!-- pagebreak -->\n\n## Conclusion\n' });
    const [p1] = pageTexts(pdf.toBuffer());
    expect(p1).toMatch(/Introduction\s*2/);
    expect(p1).toMatch(/Conclusion\s*3/);
    const targets = pageLinks(pdf.toBuffer())[0]!.map((l) => l.target);
    expect(targets).toEqual([0, 1, 2]); // Sommaire, Introduction, Conclusion
  });

  it('components: <TableOfContents>, <PageNumber target>, <Link>', async () => {
    const pdf = await turbopdf.create(
      h(Document, { title: 'Guide' },
        h(Page, null,
          h(TableOfContents, { levels: 1 }),
          h(Text, null, 'Les prix sont ', h(Link, { href: '#prix' }, 'plus loin'), '.'),
          h(PageNumber, { target: 'prix', format: 'voir page {page}' }),
          h(PageBreak),
          h(Heading, null, 'Présentation'),
          h(PageBreak),
          h(Heading, { id: 'prix' }, 'Prix'),
          h(Heading, { level: 2 }, 'pas dans le sommaire'))),
    );
    const [p1] = pageTexts(pdf.toBuffer());
    expect(p1).toMatch(/Présentation\s*2/);
    expect(p1).toMatch(/Prix\s*3/);
    expect(p1).not.toContain('pas dans le sommaire');
    expect(p1).toContain('voir page 3');
    expect(pageLinks(pdf.toBuffer())[0]!.map((l) => l.target)).toEqual([1, 2, 2]);
    expect(pdf.warnings).toEqual([]);
  });
});

describe('SVG, WebP, GIF', () => {
  it('inline <svg> is drawn as vectors, keeping viewBox', async () => {
    const pdf = await turbopdf.create(`<div>${SVG}</div>`);
    expect(pdf.warnings).toEqual([]);
    expect(pdfHas(pdf.toBuffer(), '/Subtype/Image')).toBe(false);
    near(pixelAt(pdf.toBuffer(), 60, 40), [0x25, 0x63, 0xeb]); // 120 × 40 px from the 10 mm margin
    near(pixelAt(pdf.toBuffer(), 130, 40), [255, 255, 255]);
  });

  it('SVG, WebP and GIF files and data: URLs', async () => {
    const dir = tmp();
    writeFileSync(join(dir, 'logo.svg'), SVG);
    writeFileSync(join(dir, 'photo.webp'), Buffer.from(WEBP_1X1, 'base64'));
    const svgUrl = `data:image/svg+xml;utf8,${encodeURIComponent(SVG)}`;
    const pdf = await turbopdf.create({
      html: `<img src="logo.svg"><img src="photo.webp" width="30" height="30"><img src="data:image/gif;base64,${GIF_1X1}" width="30"><img src="${svgUrl}" width="60">`,
      baseDir: dir,
    });
    expect(pdf.warnings).toEqual([]);
    expect((await pdf.toPng({ scale: 1 })).length).toBe(1);
  });

  it('<Image> accepts SVG bytes', async () => {
    const pdf = await turbopdf.create(h(Image, { src: Buffer.from(SVG), style: { width: 90 } }));
    expect(pdf.warnings).toEqual([]);
  });
});

describe('flex containers', () => {
  it('each child of display:flex is its own item, like in CSS', async () => {
    const tree = await toTree({ html: '<div style="display:flex;justify-content:space-between"><span>Gauche</span> <b>Droite</b></div>' });
    const [box] = tree.children[0]!.children;
    expect(box).toMatchObject({
      type: 'view',
      children: [
        { type: 'text', content: 'Gauche' },
        { type: 'text', content: [{ text: 'Droite', style: { fontWeight: 700 } }] },
      ],
    });
    // a flex item keeps its box styles
    const badge = await toTree({ html: '<style>.b{position:absolute;top:4px;background:#b45309;padding:4px}</style><div style="display:flex"><span class="b">NEUF</span></div>' });
    expect(badge.children[0]!.children[0]).toMatchObject({ children: [{ type: 'text', style: { position: 'absolute', backgroundColor: '#b45309', padding: 3 } }] });
    const [text] = pageTexts((await turbopdf.create('<div style="display:flex;justify-content:space-between"><span>Gauche</span><span>Droite</span></div>')).toBuffer());
    expect(text).toMatch(/Gauche\s+Droite/);
  });
});

describe('position: absolute and watermark', () => {
  it('position:absolute places a stamp in its parent, out of the flow', async () => {
    const html = `<div style="position:relative;height:120px;background:#f3f4f6">
      <p>Facture 42</p>
      <div style="position:absolute;top:0;right:0;width:80px;height:30px;background:#dc2626;color:#fff">PAYÉ</div>
      <p>Ligne suivante</p>
    </div>`;
    const pdf = await turbopdf.create(html);
    near(pixelAt(pdf.toBuffer(), 540, 35), [0xdc, 0x26, 0x26]); // top-right corner of the page body
    const tree = await toTree({ html });
    const [box] = tree.children[0]!.children;
    expect(box).toMatchObject({ children: [{}, { style: { position: 'absolute', top: 0, right: 0 } }, {}] });
  });

  it('watermark is stamped on every page', async () => {
    const pdf = await turbopdf.create({ html: '<p>un</p><p style="break-before:page">deux</p>', watermark: 'BROUILLON' });
    for (const t of pageTexts(pdf.toBuffer())) expect(t).toContain('BROUILLON');
    await expect(turbopdf.create({ html: 'x', watermark: '  ' })).rejects.toMatchObject({ code: 'invalid_source' });
  });

  it('<Page watermark> with settings', async () => {
    const tree = await toTree({ react: h(Document, null, h(Page, { watermark: { text: 'PAYÉ', color: '#16a34a40', angle: 30 } }, h(View, null))) });
    expect(tree.children[0]!.watermark).toEqual({ text: 'PAYÉ', color: '#16a34a40', angle: 30 });
  });
});

describe('@font-face with local files', () => {
  it('is loaded from baseDir', async () => {
    const pdf = await turbopdf.create({
      html: `<style>@font-face { font-family: "Icons"; src: url(fixtures/codicon.ttf) format("truetype"); } p { font-family: Icons; font-size: 30px }</style><p>${FONT_GLYPH}</p>`,
      baseDir: TEST_DIR,
    });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('url(…) in a linked stylesheet is relative to that stylesheet', async () => {
    const dir = tmp();
    mkdirSync(join(dir, 'css'));
    mkdirSync(join(dir, 'fonts'));
    copyFileSync(FONT_PATH, join(dir, 'fonts', 'icons.ttf'));
    writeFileSync(join(dir, 'css', 'style.css'), '@font-face { font-family: Icons; src: url("../fonts/icons.ttf"); font-weight: 400 } p { font-family: Icons }');
    const pdf = await turbopdf.create({ html: `<link rel="stylesheet" href="css/style.css"><p>${FONT_GLYPH}</p>`, baseDir: dir });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('web fonts and WOFF2 give a font_failed warning that says what to do', async () => {
    const pdf = await turbopdf.create(
      '<style>@font-face{font-family:A;src:url(https://fonts.example/a.woff2) format("woff2")} @font-face{font-family:B;src:url(b.woff2) format("woff2")}</style><p>x</p>',
    );
    expect(pdf.warnings.map((w) => [w.type, w.url])).toEqual([
      ['font_failed', 'https://fonts.example/a.woff2'],
      ['font_failed', 'b.woff2'],
    ]);
    expect(pdf.warnings[0]!.message).toMatch(/never downloaded/);
    expect(pdf.warnings[1]!.message).toMatch(/WOFF/);
  });
});

describe('QR codes', () => {
  const url = 'https://exemple.fr/payer?facture=2026-042&montant=37735.20';
  const qrNode = (tree: { children: { children: unknown[] }[] }) => JSON.stringify(tree).match(/"src":\{"qr":[^}]*\}/)?.[0];

  it('HTML: <img src="qr:…">, colours from CSS, data-ecc', async () => {
    const tree = await toTree({ html: `<img src="qr:${url.replace(/&/g, '&amp;')}" alt="Payer" data-ecc="h" style="width:80px;color:#1e3a8a">` });
    expect(tree.children[0]!.children[0]).toMatchObject({ type: 'image', alt: 'Payer', src: { qr: url, ecc: 'H', color: '#1e3a8a' }, style: { width: 60 } });
  });

  it('Markdown: ![alt](qr:…), text kept exactly', async () => {
    const tree = await toTree({ markdown: `![Payer](qr:${url})` });
    expect(qrNode(tree as never)).toBe(`"src":{"qr":"${url}"}`);
  });

  it('React: <QR value size color />', async () => {
    const tree = await toTree({ react: h(Document, null, h(Page, null, h(QR, { value: url, size: 72, color: '#0f766e', alt: 'Payer' }))) });
    expect(JSON.stringify(tree)).toContain(`"src":{"qr":"${url}","color":"#0f766e"}`);
    expect(JSON.stringify(tree)).toContain('"width":72,"height":72');
  });

  it('renders with no warning in the PDF, the PNG and the HTML preview', async () => {
    const pdf = await turbopdf.create(`<p>Scannez :</p><img src="qr:${url}" alt="Payer" width="100">`);
    expect(pdf.warnings).toEqual([]);
    expect((await pdf.toPng({ scale: 1 })).length).toBe(1);
    const { style, pages } = await pdf.preview();
    expect(style).toContain('image/svg+xml');
    expect(pages[0]!.html).toContain('role="img" aria-label="Payer"');
  });

  it('an empty QR code is a clear error', async () => {
    await expect(turbopdf.create('<img src="qr:">')).rejects.toMatchObject({ code: 'invalid_tree' });
  });
});

describe('borders', () => {
  it('draws each side with its own width, colour and style, in the PDF and the preview', async () => {
    const html = `<div style="width:100pt;height:50pt;border-bottom:6pt solid #ff0000;border-right:6pt solid #0000ff"></div>`;
    const pdf = await turbopdf.create({ html, margin: 0 });
    const bytes = pdf.toBuffer();
    near(pixelAt(bytes, 50, 47), [255, 0, 0]); // bottom
    near(pixelAt(bytes, 97, 25), [0, 0, 255]); // right (border-box: inside the 100pt)
    near(pixelAt(bytes, 50, 2), [255, 255, 255]); // no top
    near(pixelAt(bytes, 2, 25), [255, 255, 255]); // no left
    const { pages } = await pdf.preview();
    expect(pages[0]!.html).toContain('border-bottom:6pt solid #ff0000');
    expect(pages[0]!.html).not.toContain('border-top');
  });

  it('a later side overrides the shorthand from a stylesheet', async () => {
    const pdf = await turbopdf.create({ html: '<div class="b"></div>', css: '.b { width: 100pt; height: 50pt; border: 6pt solid #0000ff } .b { border-top: none }', margin: 0 });
    const bytes = pdf.toBuffer();
    near(pixelAt(bytes, 50, 2), [255, 255, 255]);
    near(pixelAt(bytes, 50, 47), [0, 0, 255]);
  });
});

describe('float', () => {
  const page = { margin: 0, width: '200pt', height: '200pt' } as const;

  it('floated columns sit side by side, right floats go to the right', async () => {
    const html = `<div style="float:left;width:50pt;height:40pt;background:#ff0000"></div>
      <div style="float:left;width:50pt;height:40pt;background:#00ff00"></div>
      <div style="float:right;width:50pt;height:40pt;background:#0000ff"></div>`;
    const bytes = (await turbopdf.create({ html, ...page })).toBuffer();
    near(pixelAt(bytes, 25, 20), [255, 0, 0]);
    near(pixelAt(bytes, 75, 20), [0, 255, 0]); // beside the first, not under it
    near(pixelAt(bytes, 175, 20), [0, 0, 255]); // at the right edge
    near(pixelAt(bytes, 125, 20), [255, 255, 255]);
  });

  it('text after a float flows beside it, and clear goes below', async () => {
    const html = `<img src="qr:hello" style="float:left;width:80pt;height:80pt">
      <p>Beside the code</p><p style="clear:both">Below</p>`;
    const pdf = await turbopdf.create({ html, ...page });
    expect(firstTextLeft(pdf.toBuffer())).toBeGreaterThan(79);
    expect(pageTexts(pdf.toBuffer())[0]).toMatch(/Beside the code[\s\S]*Below/);
    expect(pdf.warnings).toEqual([]);
  });
});
