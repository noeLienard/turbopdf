import { describe, expect, it } from 'vitest';
import turbopdf, { TurboPdfError } from '../src/index';
import { firstTextLeft, pageSizes, pageTexts, pixelAt, pagesOf } from './helpers';

const near = (a: number, b: number, tol = 3) => expect(Math.abs(a - b)).toBeLessThanOrEqual(tol);
const errorCode = async (p: Promise<unknown>) => (await p.then(() => null, (e: unknown) => e)) as TurboPdfError | null;

describe('create(): sources', () => {
  it('accepts an HTML string directly', async () => {
    const pdf = await turbopdf.create('<h1>Facture</h1>');
    expect(pdf.toBuffer().subarray(0, 5).toString()).toBe('%PDF-');
    expect(pdf.pages).toBe(1);
    expect(pageTexts(pdf.toBuffer())[0]).toContain('Facture');
  });

  it('accepts { html }', async () => {
    const pdf = await turbopdf.create({ html: '<p>option html</p>' });
    expect(pageTexts(pdf.toBuffer())[0]).toContain('option html');
  });

  it('rejects zero or several sources with invalid_source', async () => {
    expect((await errorCode(turbopdf.create({} as never)))?.code).toBe('invalid_source');
    expect((await errorCode(turbopdf.create({ html: '<p>a</p>', markdown: '# b' } as never)))?.code).toBe('invalid_source');
    expect((await errorCode(turbopdf.create(42 as never)))?.code).toBe('invalid_source');
    expect((await errorCode(turbopdf.create({ html: '  ' })))?.code).toBe('invalid_source');
  });

  it('options of the browser-based versions are refused with what to do instead', async () => {
    for (const [opts, hint] of [
      [{ url: 'https://example.com' }, /Fetch the page yourself/],
      [{ html: 'x', engine: 'chromium' }, /single engine/],
      [{ html: 'x', printBackground: false }, /always printed/],
      [{ html: 'x', javascript: false }, /never run/],
      [{ html: 'x', cookies: [] }, /network/],
    ] as const) {
      const e = await errorCode(turbopdf.create(opts as never));
      expect(e?.code).toBe('invalid_source');
      expect(e?.message).toMatch(hint);
    }
  });
});

describe('option: format / orientation / width / height', () => {
  const sizes: Array<['A3' | 'A4' | 'A5' | 'Letter' | 'Legal', number, number]> = [
    ['A3', 842, 1191],
    ['A4', 595, 842],
    ['A5', 420, 595],
    ['Letter', 612, 792],
    ['Legal', 612, 1008],
  ];
  for (const [format, w, h] of sizes) {
    it(`format ${format}`, async () => {
      const [size] = pageSizes((await turbopdf.create({ html: '<p>x</p>', format })).toBuffer());
      near(size!.width, w, 2);
      near(size!.height, h, 2);
    });
  }

  it('defaults to A4 portrait', async () => {
    const [size] = pageSizes((await turbopdf.create('<p>x</p>')).toBuffer());
    near(size!.width, 595, 2);
  });

  it('landscape swaps width and height', async () => {
    const [size] = pageSizes((await turbopdf.create({ html: '<p>x</p>', orientation: 'landscape' })).toBuffer());
    near(size!.width, 842, 2);
    near(size!.height, 595, 2);
  });

  it('custom width and height', async () => {
    const [size] = pageSizes((await turbopdf.create({ html: '<p>x</p>', width: '100mm', height: '200mm' })).toBuffer());
    near(size!.width, 283.5, 2);
    near(size!.height, 567, 2);
  });

  it('rejects a bad format, orientation, or a single custom side', async () => {
    expect((await errorCode(turbopdf.create({ html: 'x', format: 'B7' as never })))?.code).toBe('invalid_source');
    expect((await errorCode(turbopdf.create({ html: 'x', orientation: 'diagonal' as never })))?.code).toBe('invalid_source');
    expect((await errorCode(turbopdf.create({ html: 'x', width: '100mm' })))?.code).toBe('invalid_source');
  });
});

describe('option: margin', () => {
  const flat = 'body{margin:0}';

  it('string margin (20mm ≈ 56.7pt)', async () => {
    const pdf = await turbopdf.create({ html: '<p>m</p>', margin: '20mm', css: flat });
    near(firstTextLeft(pdf.toBuffer()), 56.7);
  });

  it('object margin (left 30mm ≈ 85pt)', async () => {
    const pdf = await turbopdf.create({ html: '<p>m</p>', margin: { left: '30mm', top: '10mm', right: '10mm', bottom: '10mm' }, css: flat });
    near(firstTextLeft(pdf.toBuffer()), 85);
  });

  it('default margin is 10mm (≈ 28pt)', async () => {
    const pdf = await turbopdf.create({ html: '<p>m</p>', css: flat });
    near(firstTextLeft(pdf.toBuffer()), 28.3);
  });

  it('rejects an invalid length', async () => {
    expect((await errorCode(turbopdf.create({ html: 'x', margin: 'wide' })))?.code).toBe('invalid_source');
  });
});

describe('backgrounds', () => {
  it('are always printed', async () => {
    const html = '<div style="background:#ff0000;height:800px">x</div>';
    expect(pixelAt((await turbopdf.create({ html })).toBuffer(), 300, 300)).toEqual([255, 0, 0]);
  });
});

describe('@media', () => {
  it('print and all rules apply, screen-only rules do not', async () => {
    const html = '<p class="a">ONLY-SCREEN</p><p class="b">ONLY-PRINT</p><p class="c">ALWAYS</p>';
    const css = '@media print{.a{display:none}} @media screen{.b{display:none}} @media all{.c{color:red}}';
    const t = pageTexts((await turbopdf.create({ html, css })).toBuffer())[0]!;
    expect(t).toContain('ONLY-PRINT');
    expect(t).toContain('ALWAYS');
    expect(t).not.toContain('ONLY-SCREEN');
  });
});

describe('option: css', () => {
  it('adds styles to the document', async () => {
    const t = pageTexts((await turbopdf.create({ html: '<h1>HIDDEN</h1><p>shown</p>', css: 'h1{display:none}' })).toBuffer())[0]!;
    expect(t).not.toContain('HIDDEN');
    expect(t).toContain('shown');
  });
});

describe('option: timeout', () => {
  it('rejects with render_timeout when the render takes too long', async () => {
    const rows = Array.from({ length: 3000 }, (_, i) => `<tr><td>row ${i}</td><td>${i}</td></tr>`).join('');
    const e = await errorCode(turbopdf.create({ html: `<table>${rows}</table>`, timeout: 1 }));
    expect(e?.code).toBe('render_timeout');
    expect(e?.message).toContain('timeout');
  });

  it('rejects an invalid timeout', async () => {
    expect((await errorCode(turbopdf.create({ html: 'x', timeout: 0 })))?.code).toBe('invalid_source');
  });
});

describe('option: header / footer', () => {
  it('footer repeats on every page with {page} / {pages}', async () => {
    const pdf = await turbopdf.create({ html: pagesOf(2), footer: '<div style="text-align:center">Page {page} / {pages}</div>' });
    const texts = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(2);
    expect(texts[0]).toContain('Page 1 / 2');
    expect(texts[1]).toContain('Page 2 / 2');
  });

  it('header works too, with no date or title added', async () => {
    const pdf = await turbopdf.create({ html: '<p>body</p>', header: '<span>My header</span>' });
    const t = pageTexts(pdf.toBuffer())[0]!;
    expect(t).toContain('My header');
    expect(t).not.toMatch(/\d{1,2}\/\d{1,2}\/\d{2,4}/);
  });
});

