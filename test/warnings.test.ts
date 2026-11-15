import { describe, expect, it } from 'vitest';
import papier, { PapierError } from '../src/index';

describe('warnings', () => {
  it('a clean document has none', async () => {
    const pdf = await papier.create('<h1>Hello</h1><p>fine</p>');
    expect(pdf.warnings).toEqual([]);
  });

  it('missing_glyph: a character the fonts do not have', async () => {
    const pdf = await papier.create('<p id="price">prix 漢</p>');
    const w = pdf.warnings.find((x) => x.type === 'missing_glyph');
    expect(w).toMatchObject({ path: 'price', page: 1 });
    expect(w?.message).toMatch(/U\+6F22.*`fonts` option/);
  });

  it('image_failed: a remote image is never fetched, and says so', async () => {
    const pdf = await papier.create('<img id="logo" src="https://example.com/logo.png" width="50" height="50">');
    const w = pdf.warnings.find((x) => x.type === 'image_failed');
    expect(w?.path).toBe('logo');
    expect(w?.message).toContain('https://example.com/logo.png');
    expect(w?.message).toMatch(/never uses the network/);
  });

  it('image_failed: a missing local file', async () => {
    const pdf = await papier.create('<img src="nope/missing.png" width="50" height="50">');
    expect(pdf.warnings.some((x) => x.type === 'image_failed')).toBe(true);
  });

  it('image_failed: corrupt image data', async () => {
    const pdf = await papier.create('<img src="data:image/png;base64,AAAA" width="50" height="50">');
    expect(pdf.warnings.some((x) => x.type === 'image_failed')).toBe(true);
  });

  it('stylesheet_failed: remote stylesheets are not fetched', async () => {
    const pdf = await papier.create('<link rel="stylesheet" href="https://cdn.example.com/a.css"><p>x</p>');
    expect(pdf.warnings).toEqual([expect.objectContaining({ type: 'stylesheet_failed', url: 'https://cdn.example.com/a.css' })]);
  });

  it('overflow: an element wider than the page, named by its id', async () => {
    const pdf = await papier.create('<div id="wide" style="width:3000px;height:20px;background:#eee">too wide</div>');
    const w = pdf.warnings.find((x) => x.type === 'overflow');
    expect(w?.path).toBe('wide');
    expect(w?.message).toContain('wide');
  });

  it('moved_to_next_page: a block kept together', async () => {
    const pdf = await papier.create('<div style="height:900px"></div><div id="sig" style="break-inside:avoid"><p>a</p><p>b</p><p>c</p><p>d</p><p>e</p><p>f</p><p>g</p></div>');
    expect(pdf.warnings).toEqual([expect.objectContaining({ type: 'moved_to_next_page', path: 'sig', page: 2 })]);
  });

  it('never makes the generation fail', async () => {
    const pdf = await papier.create('<img src="https://a.example/a.png"><div style="width:3000px">x</div>');
    expect(pdf.toBuffer().subarray(0, 5).toString()).toBe('%PDF-');
    expect(pdf.warnings.length).toBeGreaterThanOrEqual(2);
  });

  it('strict: turns warnings into a warnings_found error', async () => {
    const e = (await papier.create({ html: '<img src="https://a.example/a.png">', strict: true }).then(() => null, (x: unknown) => x)) as PapierError;
    expect(e.code).toBe('warnings_found');
    expect(e.warnings?.[0]?.type).toBe('image_failed');
    expect(e.message).toContain('image_failed');
  });
});
