import { describe, expect, it } from 'vitest';
import papier from '../src/index';
import { pageTexts } from './helpers';

/** A 200-row table whose rows are two lines tall: the worst case for a row split across pages. */
function bigTable(rows: number): string {
  const body = Array.from({ length: rows }, (_, i) => {
    const n = i + 1;
    return `<tr><td>R${n}-A<br>R${n}-L2</td><td>x</td><td>R${n}-D</td></tr>`;
  }).join('');
  return `<table style="width:100%;border-collapse:collapse"><thead><tr><th>HEADCOL</th><th>b</th><th>c</th></tr></thead><tbody>${body}</tbody></table>`;
}

describe('page breaks inside a long table', () => {
  it('never cuts a row, and repeats the header on every page', async () => {
    const pdf = await papier.create({ html: bigTable(200), css: 'td{padding:9px 6px}' });
    const texts = pageTexts(pdf.toBuffer());

    expect(pdf.pages).toBeGreaterThanOrEqual(4);
    expect(texts).toHaveLength(pdf.pages);

    // header repeated on every page
    for (const [i, t] of texts.entries()) expect(t, `page ${i + 1} misses the header`).toContain('HEADCOL');

    // every row is on exactly one page, with all its lines together
    for (let n = 1; n <= 200; n++) {
      const holders = texts.map((t, i) => (t.includes(`R${n}-A`) ? i : -1)).filter((i) => i >= 0);
      expect(holders, `row ${n} must appear on exactly one page`).toHaveLength(1);
      const page = texts[holders[0]!]!;
      expect(page, `row ${n} was cut: second line is on another page`).toContain(`R${n}-L2`);
      expect(page, `row ${n} was cut: last cell is on another page`).toContain(`R${n}-D`);
    }
  });
});

describe('utility classes', () => {
  it('.papier-break-before starts a new page', async () => {
    const pdf = await papier.create('<p>FIRST</p><p class="papier-break-before">SECOND</p>');
    const t = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(2);
    expect(t[0]).toContain('FIRST');
    expect(t[1]).toContain('SECOND');
  });

  it('.papier-break-after ends the page', async () => {
    const pdf = await papier.create('<p class="papier-break-after">FIRST</p><p>SECOND</p>');
    const t = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(2);
    expect(t[0]).toContain('FIRST');
    expect(t[1]).toContain('SECOND');
  });

  it('.papier-keep-together moves a block that would be cut', async () => {
    // a 700px filler, then a 400px block made of 10 lines: it does not fit in the rest of page 1
    const lines = Array.from({ length: 10 }, (_, i) => `<div style="height:40px">KEEP-${i}</div>`).join('');
    const pdf = await papier.create(`<div style="height:700px">filler</div><div class="papier-keep-together">${lines}</div>`);
    const t = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(2);
    for (let i = 0; i < 10; i++) {
      expect(t[0]).not.toContain(`KEEP-${i}`);
      expect(t[1]).toContain(`KEEP-${i}`);
    }
  });

  it('without the class the same block is cut across pages (control)', async () => {
    const lines = Array.from({ length: 10 }, (_, i) => `<div style="height:40px">CUT-${i}</div>`).join('');
    const pdf = await papier.create(`<div style="height:700px">filler</div><div>${lines}</div>`);
    const t = pageTexts(pdf.toBuffer());
    expect(t[0]).toContain('CUT-0');
    expect(t[1]).toContain('CUT-9');
  });
});
