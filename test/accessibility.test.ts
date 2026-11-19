import { createElement as h } from 'react';
import { describe, expect, it } from 'vitest';
import { Document, Image, Page, Text, View } from '../src/components';
import turbopdf from '../src/index';

const GIF_1X1 = 'R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==';
const raw = (pdf: { toBuffer(): Buffer }) => pdf.toBuffer().toString('latin1');
const count = (pdf: { toBuffer(): Buffer }, needle: string) => raw(pdf).split(needle).length - 1;

const REPORT = `<!doctype html><html lang="fr"><head><title>Rapport</title></head><body>
  <section>
    <h1>Rapport</h1>
    <p>Voir <a href="https://example.com">le site</a>.</p>
    <ul><li>Un</li><li>Deux</li></ul>
    <ol><li>Premier</li></ol>
    <table><thead><tr><th>Mois</th><th>CA</th></tr></thead><tbody><tr><td>Oct</td><td>12</td></tr></tbody></table>
    <blockquote>Une citation</blockquote>
    <pre>const a = 1;</pre>
    <img src="data:image/gif;base64,${GIF_1X1}" alt="Logo Acme" width="20">
    <img src="data:image/gif;base64,${GIF_1X1}" alt="" width="20">
    <div aria-hidden="true"><img src="data:image/gif;base64,${GIF_1X1}" alt="ornement" width="20"></div>
    <hr>
  </section>
</body></html>`;

describe('tagged PDF', () => {
  it('HTML semantics become the structure tree, by default', async () => {
    const pdf = await turbopdf.create(REPORT);
    expect(pdf.warnings).toEqual([]);
    expect(raw(pdf)).toContain('/StructTreeRoot');
    expect(raw(pdf)).toContain('/Marked true');
    for (const [role, n] of [
      ['/S/Sect', 1],
      ['/S/H1', 1],
      ['/S/Link', 1],
      ['/S/LI', 3],
      ['/S/Table', 1],
      ['/S/TH', 2],
      ['/S/TD', 2],
      ['/S/BlockQuote', 1],
      ['/S/Code', 1],
    ] as const) {
      expect(count(pdf, role), role).toBe(n);
    }
    // the decorative and aria-hidden images are artifacts: one figure only
    expect(count(pdf, '/S/Figure')).toBe(1);
    expect(raw(pdf)).toContain('/Alt(Logo Acme)');
    expect(raw(pdf)).toContain('/Lang(fr)');
  });

  it('Markdown lists, quotes and code blocks are tagged too', async () => {
    const pdf = await turbopdf.create({ markdown: '# Notes\n\n- un\n- deux\n\n> cité\n\n```\ncode\n```\n' });
    expect(count(pdf, '/S/LI')).toBe(2);
    expect(count(pdf, '/S/BlockQuote')).toBe(1);
    expect(count(pdf, '/S/Code')).toBe(1);
  });

  it('components take a role', async () => {
    const pdf = await turbopdf.create(
      h(Document, { title: 'Liste' },
        h(Page, null,
          h(View, { role: 'list' }, h(View, { role: 'list_item' }, h(Text, { role: 'list_label' }, '•'), h(View, { role: 'list_body' }, h(Text, null, 'Un')))),
          h(Image, { src: `data:image/gif;base64,${GIF_1X1}`, alt: 'Photo', style: { width: 20 } }))),
    );
    expect(count(pdf, '/S/LI')).toBe(1);
    expect(count(pdf, '/S/Lbl')).toBe(1);
    expect(raw(pdf)).toContain('/Alt(Photo)');
  });

  it('tagged: false writes no structure (smaller files)', async () => {
    const tagged = await turbopdf.create(REPORT);
    const plain = await turbopdf.create({ html: REPORT, tagged: false });
    expect(raw(plain)).not.toContain('/StructTreeRoot');
    expect(plain.toBuffer().length).toBeLessThan(tagged.toBuffer().length);
  });
});

describe('PDF/UA', () => {
  it('pdfua: true gives a PDF/UA-1 document', async () => {
    const pdf = await turbopdf.create({ html: REPORT, pdfua: true });
    expect(pdf.warnings).toEqual([]);
    expect(raw(pdf)).toContain('pdfuaid:part');
    expect(raw(pdf)).toContain('/DisplayDocTitle true');
  });

  it('needs a title and a language, and says where to put them', async () => {
    await expect(turbopdf.create({ html: '<p>x</p>', pdfua: true })).rejects.toMatchObject({
      code: 'invalid_source',
      message: expect.stringMatching(/title.*<title>.*language.*<html lang/),
    });
    const pdf = await turbopdf.create({ html: '<p>x</p>', pdfua: true, title: 'Note', lang: 'fr' });
    expect(raw(pdf)).toContain('pdfuaid:part');
  });

  it('Markdown: the first # heading is the title', async () => {
    const pdf = await turbopdf.create({ markdown: '# Compte rendu\n\ntexte', pdfua: true, lang: 'fr' });
    expect(raw(pdf)).toContain('Compte rendu');
    expect(raw(pdf)).toContain('pdfuaid:part');
  });

  it('an image without alt is made decorative, with a missing_alt warning', async () => {
    const pdf = await turbopdf.create({ html: `<title>T</title><img id="photo" src="data:image/gif;base64,${GIF_1X1}" width="20">`, pdfua: true, lang: 'fr' });
    expect(pdf.warnings).toEqual([expect.objectContaining({ type: 'missing_alt', path: 'photo', page: 1, message: expect.stringContaining('alt=') })]);
    expect(count(pdf, '/S/Figure')).toBe(0);
  });

  it('a character no font has makes conformance impossible: not_accessible', async () => {
    await expect(turbopdf.create({ html: '<title>T</title><p>漢字</p>', pdfua: true, lang: 'fr' })).rejects.toMatchObject({
      code: 'not_accessible',
      details: [expect.stringContaining('characters in no font: 漢 字')],
    });
  });
});

describe('PDF/A', () => {
  it('pdfa: a PDF/A document, with the date and the sRGB output intent', async () => {
    for (const [level, part, conformance] of [['2b', '2', 'B'], ['3b', '3', 'B'], ['3a', '3', 'A'], ['2u', '2', 'U']] as const) {
      const pdf = await turbopdf.create({ html: REPORT, pdfa: level });
      expect(pdf.warnings).toEqual([]);
      expect(raw(pdf)).toContain(`<pdfaid:part>${part}</pdfaid:part>`);
      expect(raw(pdf)).toContain(`<pdfaid:conformance>${conformance}</pdfaid:conformance>`);
      expect(raw(pdf)).toContain('/OutputIntents');
    }
  });

  it('a given creation date gives the same bytes on every run; without one, it is now', async () => {
    const make = () => turbopdf.create(h(Document, { pdfa: '3b', creationDate: '2026-11-19T10:00:00Z', title: 'F' }, h(Page, null, h(Text, null, 'Facture'))));
    expect((await make()).toBuffer().equals((await make()).toBuffer())).toBe(true);
    expect(raw(await make())).toContain('2026-11-19T10:00:00');
    const year = String(new Date().getUTCFullYear());
    expect(raw(await turbopdf.create({ html: '<p>x</p>', pdfa: '2b' }))).toContain(`<xmp:CreateDate>${year}`);
  });

  it('PDF/A and PDF/UA together', async () => {
    const pdf = await turbopdf.create({ html: REPORT, pdfa: '3a', pdfua: true });
    expect(raw(pdf)).toContain('pdfuaid:part');
    expect(raw(pdf)).toContain('<pdfaid:part>3</pdfaid:part>');
  });

  it('wrong combinations are refused before rendering', async () => {
    await expect(turbopdf.create({ html: '<p>x</p>', pdfa: '5z' as never })).rejects.toMatchObject({ code: 'invalid_source', message: expect.stringContaining("'3b'") });
    await expect(turbopdf.create({ html: '<p>x</p>', pdfa: '3a', tagged: false })).rejects.toMatchObject({ code: 'invalid_source', message: expect.stringContaining('tagged') });
    await expect(turbopdf.create({ html: REPORT, pdfa: '1b', pdfua: true })).rejects.toMatchObject({ code: 'invalid_source', message: expect.stringContaining('PDF/A-1') });
  });

  it('what PDF/A forbids: not_pdfa, with the problems', async () => {
    await expect(turbopdf.create({ html: '<div style="height:20px;background:rgba(255,0,0,.5)"></div>', pdfa: '1b' })).rejects.toMatchObject({
      code: 'not_pdfa',
      message: expect.stringContaining('PDF/A-1b'),
      details: [expect.stringContaining('transparency')],
    });
    await expect(turbopdf.create({ html: '<p>漢字</p>', pdfa: '2b' })).rejects.toMatchObject({
      code: 'not_pdfa',
      details: [expect.stringContaining('characters in no font: 漢 字')],
    });
  });
});
