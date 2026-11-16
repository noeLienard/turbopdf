import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import turbopdf, { type CreateOptions } from '../src/index';
import { pagesOf } from './helpers';
const PNG = '\x89PNG';

describe('toPng()', () => {
  it('returns one PNG per page', async () => {
    const pdf = await turbopdf.create(pagesOf(3));
    const pages = await pdf.toPng();
    expect(pages).toHaveLength(3);
    expect(pages.map((p) => p.page)).toEqual([1, 2, 3]);
    for (const p of pages) expect(p.toBuffer().subarray(0, 4).toString('latin1')).toBe(PNG);
  });

  it('scale defaults to 2 and changes the image size', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    const [x2] = await pdf.toPng();
    const [x1] = await pdf.toPng({ scale: 1 });
    // A4 is 595.28 × 841.89 pt: each scale rounds up on its own, so allow 1 px
    expect(Math.abs(x2!.width - x1!.width * 2)).toBeLessThanOrEqual(1);
    expect(Math.abs(x2!.height - x1!.height * 2)).toBeLessThanOrEqual(1);
    expect(x2!.width).toBeGreaterThan(1100); // A4 at 2x
  });

  it('save() writes the PNG and creates folders', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'turbopdf-'));
    const [page] = await (await turbopdf.create('<p>x</p>')).toPng({ scale: 1 });
    const file = join(dir, 'deep', 'apercu.png');
    await page!.save(file);
    expect(readFileSync(file).equals(page!.toBuffer())).toBe(true);
  });

  it('toDataUrl() is ready for <img src>', async () => {
    const [page] = await (await turbopdf.create('<p>x</p>')).toPng({ scale: 1 });
    const url = page!.toDataUrl();
    expect(url.startsWith('data:image/png;base64,')).toBe(true);
    expect(Buffer.from(url.split(',')[1]!, 'base64').equals(page!.toBuffer())).toBe(true);
  });

  it('is cached per scale', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    expect(await pdf.toPng({ scale: 1 })).toBe(await pdf.toPng({ scale: 1 }));
  });

  it('rejects an absurd scale', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    await expect(pdf.toPng({ scale: 0 })).rejects.toMatchObject({ code: 'invalid_source' });
  });
});

describe('preview(): the document as HTML/CSS', () => {
  it('one <section> per page, real text, the size of the PDF page', async () => {
    const pdf = await turbopdf.create(pagesOf(3));
    const preview = await pdf.preview();
    expect(preview.pages.map((p) => p.page)).toEqual([1, 2, 3]);
    expect(preview.pages[0]).toMatchObject({ width: 595.28, height: 841.89 });
    expect(preview.pages[0]!.html).toMatch(/^<section class="turbopdf-page" style="width:595.28pt;height:841.89pt"/);
    expect(preview.pages[1]!.html).toMatch(/<span class="pw pf\d+"[^>]*>block <\/span>/);
    expect(preview.style).toContain('@font-face');
  });

  it('works the same for every source', async () => {
    const sources: CreateOptions[] = [
      { html: '<h1>Bonjour</h1><p>Un <a href="https://exemple.fr">lien</a>.</p>' },
      { markdown: '# Bonjour\n\nUn [lien](https://exemple.fr).' },
      { tree: { type: 'document', version: 1, children: [{ type: 'page', children: [{ type: 'text', content: [{ text: 'Bonjour ' }, { text: 'lien', link: 'https://exemple.fr' }] }] }] } },
    ];
    for (const s of sources) {
      const { pages } = await (await turbopdf.create(s)).preview();
      expect(pages[0]!.html).toMatch(/>Bonjour ?<\/span>/);
      expect(pages[0]!.html).toContain('<a class="pl" href="https://exemple.fr" target="_blank"');
    }
  });

  it('internal links jump inside the preview', async () => {
    const { pages } = await (await turbopdf.create({ markdown: '[Aller au résumé](#résumé)\n\n<!-- pagebreak -->\n\n## Résumé' })).preview();
    expect(pages[0]!.html).toContain('href="#turbopdf-résumé"');
    expect(pages[1]!.html).toContain('id="turbopdf-résumé"');
  });

  it('text is escaped, colours and backgrounds are CSS', async () => {
    const { pages } = await (await turbopdf.create('<div style="background:#fde68a;border:1px dashed #b45309;padding:8px"><p style="color:#1d4ed8">a &lt;b&gt; &amp; c</p></div>')).preview();
    const html = pages[0]!.html;
    expect(html).toContain('>&lt;b&gt; </span>');
    expect(html).toContain('>&amp; </span>');
    expect(html).toContain('color:#1d4ed8');
    expect(html).toMatch(/background:#fde68a;border:0?\.?75pt dashed #b45309/);
  });

  it('toHtml() is a complete document; toResponse() serves it; save() writes it', async () => {
    const preview = await (await turbopdf.create({ html: '<p>x</p>', title: 'Mon <titre>', lang: 'fr' })).preview();
    const doc = preview.toHtml();
    expect(doc).toMatch(/^<!DOCTYPE html><html lang="fr">/);
    expect(doc).toContain('<title>Mon &lt;titre&gt;</title>');
    expect(doc).toContain(preview.style);
    const res = preview.toResponse();
    expect(res.headers.get('content-type')).toBe('text/html; charset=utf-8');
    expect(await res.text()).toBe(doc);
    const file = join(mkdtempSync(join(tmpdir(), 'turbopdf-')), 'a', 'apercu.html');
    await preview.save(file);
    expect(readFileSync(file, 'utf8')).toBe(doc);
  });

  it('only the fonts and images used are embedded, once', async () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>';
    const src = `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;
    const { style, pages } = await (await turbopdf.create(`<p>texte</p><img src="${src}" alt="carré"><img src="${src}" alt="carré">`)).preview();
    expect(style.match(/@font-face/g)).toHaveLength(1);
    expect(style.match(/image\/svg\+xml/g)).toHaveLength(1);
    expect(pages[0]!.html.match(/role="img" aria-label="carré"/g)).toHaveLength(2);
  });

  it('is cached', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    expect(await pdf.preview()).toBe(await pdf.preview());
  });
});

describe('preview({ scaleToFit })', () => {
  it('scales every length from one variable, capped at the real size, and keeps the default unchanged', async () => {
    const pdf = await turbopdf.create({ html: '<h1 style="border-bottom:2px solid red">Title</h1><p>Hello <a href="https://example.com">link</a></p>', format: 'A4' });
    const plain = await pdf.preview();
    const fit = await pdf.preview({ scaleToFit: true });
    expect(await pdf.preview({ scaleToFit: true })).toBe(fit); // cached per option
    expect(plain.pages[0]!.html).not.toContain('turbopdf-fit');
    expect(plain.style).not.toContain('turbopdf-fit');

    const html = fit.pages[0]!.html;
    expect(html.startsWith('<div class="turbopdf-fit"><section class="turbopdf-page" style="--u:min(1pt,100cqw / ')).toBe(true);
    expect(fit.style).toContain('.turbopdf-fit{container-type:inline-size');
    const styles = [...html.matchAll(/style="([^"]*)"/g)].map((m) => m[1]!);
    expect(styles.length).toBeGreaterThan(3);
    for (const css of styles) expect(css.replace('min(1pt,', '')).not.toMatch(/\dpt\b/); // no fixed size left
    expect(html).toContain('font-size:calc(var(--u)*');
    expect(html).toContain('Title'); // text untouched
    expect(fit.toHtml()).toContain('<div class="turbopdf-fit">');
  });
});
