// Typography from CSS: decorations, letter spacing, text-transform, super / subscript, tabular digits,
// white-space and text-indent. Checked on the HTML preview, which carries the engine's exact positions.
import { describe, expect, it } from 'vitest';
import turbopdf from '../src/index';

interface Word {
  left: number;
  top: number;
  size: number;
  style: string;
  text: string;
}

async function words(html: string, css = ''): Promise<{ words: Word[]; html: string; warnings: string[] }> {
  const pdf = await turbopdf.create({ html, css, format: 'A4' });
  const page = (await pdf.preview()).pages[0]!.html;
  const out: Word[] = [];
  for (const m of page.matchAll(/<span class="pw[^"]*" style="left:([\d.-]+)pt;top:([\d.-]+)pt;font-size:([\d.]+)pt;([^"]*)">([^<]*)<\/span>/g)) {
    out.push({ left: +m[1]!, top: +m[2]!, size: +m[3]!, style: m[4]!, text: m[5]!.replace(/&nbsp;| /g, ' ') });
  }
  return { words: out, html: page, warnings: pdf.warnings.map((w) => `${w.type}: ${w.message}`) };
}

const word = (ws: Word[], text: string) => {
  const w = ws.find((x) => x.text.trim() === text);
  if (!w) throw new Error(`no word "${text}" in ${ws.map((x) => x.text).join('|')}`);
  return w;
};

/** Thin filled boxes of the preview (decoration lines): their height in pt. */
const lines = (html: string) => [...html.matchAll(/<div aria-hidden="true" style="left:[\d.-]+pt;top:[\d.-]+pt;width:([\d.]+)pt;height:([\d.]+)pt;background:/g)].map((m) => ({ w: +m[1]!, h: +m[2]! }));

describe('CSS typography', () => {
  it('underlines links, <u> and text-decoration, and strikes <s>; text-decoration: none removes it', async () => {
    const plain = await words('<p>Hello world</p>');
    expect(lines(plain.html).filter((l) => l.h < 2)).toHaveLength(0);
    const deco = await words('<p><a href="https://example.com">link</a> <u>under</u> <s>gone</s> <span class="x">css</span> <a class="n" href="#a">no</a></p><p id="a">x</p>', '.x { text-decoration: underline } .n { text-decoration: none }');
    const thin = lines(deco.html).filter((l) => l.h > 0 && l.h < 2);
    expect(thin).toHaveLength(4); // link, under, gone, css
    expect(deco.warnings).toEqual([]);
  });

  it('letter-spacing widens words; text-transform changes case', async () => {
    const a = await words('<p>abcd efgh</p>');
    const b = await words('<p style="letter-spacing: 4pt; text-transform: uppercase">abcd efgh</p>');
    expect(word(b.words, 'EFGH').left - word(a.words, 'efgh').left).toBeGreaterThan(14); // 4 letters × 4pt
    expect(word(b.words, 'EFGH').style).toContain('letter-spacing:4pt');
    const c = await words('<p style="text-transform: capitalize">hello big-world</p>');
    expect(c.words.map((w) => w.text.trim())).toEqual(['Hello', 'Big-World']);
  });

  it('raises <sup> and lowers <sub>, smaller', async () => {
    const { words: ws } = await words('<p>E = mc<sup>2</sup> and H<sub>2</sub>O</p>');
    const base = word(ws, 'mc');
    const sup = ws.find((w) => w.text.startsWith('2') && w.top < base.top)!;
    expect(sup).toBeDefined();
    expect(sup.size).toBeLessThan(base.size);
    const sub = ws.filter((w) => w.text.startsWith('2')).find((w) => w.top > base.top);
    expect(sub).toBeDefined();
  });

  it('tabular-nums gives every digit the same width', async () => {
    const width = async (digits: string, css: string) => {
      const { words: ws } = await words(`<p style="${css}">${digits} end</p>`);
      return word(ws, 'end').left - ws[0]!.left;
    };
    const tab = 'font-variant-numeric: tabular-nums';
    expect(Math.abs((await width('1111', tab)) - (await width('0000', tab)))).toBeLessThan(0.01);
    expect(Math.abs((await width('1111', '')) - (await width('0000', '')))).toBeGreaterThan(0.5);
  });

  it('white-space: nowrap keeps a line whole, pre keeps spaces and newlines', async () => {
    const long = 'word '.repeat(60);
    const wrap = await words(`<div style="width: 100pt">${long}</div>`);
    const nowrap = await words(`<div style="width: 100pt; white-space: nowrap">${long}</div>`);
    expect(new Set(wrap.words.map((w) => w.top)).size).toBeGreaterThan(5);
    expect(new Set(nowrap.words.map((w) => w.top)).size).toBe(1);
    const pre = await words('<div style="white-space: pre">a    b\n  c</div>');
    expect(new Set(pre.words.map((w) => w.top)).size).toBe(2);
    expect(word(pre.words, 'c').left).toBeGreaterThan(word(pre.words, 'a').left + 5); // indented
  });

  it('text-indent moves the first line only', async () => {
    const { words: ws } = await words(`<p style="width: 200pt; text-indent: 30pt">${'lorem ipsum '.repeat(20)}</p>`);
    const tops = [...new Set(ws.map((w) => w.top))];
    const firstOf = (top: number) => ws.find((w) => w.top === top)!.left;
    expect(firstOf(tops[0]!) - firstOf(tops[1]!)).toBeCloseTo(30, 1);
  });
});
