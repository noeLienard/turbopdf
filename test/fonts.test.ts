import { mkdtempSync, readFileSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import turbopdf, { TurboPdfError } from '../src/index';
import { FONT_GLYPH, FONT_PATH, pdfHas } from './helpers';

describe('fonts', () => {
  it('Inter is built in: no font to install, accents and symbols included', async () => {
    const pdf = await turbopdf.create('<p>Été — 1 200,50 € ✓</p>');
    expect(pdfHas(pdf.toBuffer(), 'Inter')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('@font-face-free CSS families fall back to Inter without failing', async () => {
    const pdf = await turbopdf.create('<p style="font-family: Helvetica, Arial, sans-serif">texte</p>');
    expect(pdfHas(pdf.toBuffer(), 'Inter')).toBe(true);
  });

  it('`fonts` registers a local file with no network', async () => {
    const pdf = await turbopdf.create({
      html: `<p style="font-family:Brand;font-size:30px">${FONT_GLYPH}</p>`,
      fonts: [{ family: 'Brand', path: FONT_PATH }],
    });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('font-family lists: the first loaded family, then the next ones for missing characters', async () => {
    const fonts = [{ family: 'Icons', path: FONT_PATH }];
    // `Missing` is not loaded: `Icons` is the first family of the list that is
    const first = await turbopdf.create({ html: `<p style="font-family:'Missing', Icons, sans-serif">${FONT_GLYPH}</p>`, fonts });
    expect(pdfHas(first.toBuffer(), 'codicon')).toBe(true);
    expect(first.warnings).toEqual([]);
    // Inter first: the icon is not in Inter, so it comes from the next family instead of a missing glyph
    const next = await turbopdf.create({ html: `<p style="font-family: Inter, Icons">Total ${FONT_GLYPH}</p>`, fonts });
    expect(pdfHas(next.toBuffer(), 'codicon')).toBe(true);
    expect(next.warnings.filter((w) => w.type === 'missing_glyph')).toEqual([]);
    // without the list, the same text reports the missing glyph
    const alone = await turbopdf.create({ html: `<p style="font-family: Inter">Total ${FONT_GLYPH}</p>`, fonts });
    expect(alone.warnings.map((w) => w.type)).toContain('missing_glyph');
  });

  it('`fonts` accepts raw bytes', async () => {
    const pdf = await turbopdf.create({
      html: `<p style="font-family:Bytes;font-size:30px">${FONT_GLYPH}</p>`,
      fonts: [{ family: 'Bytes', data: readFileSync(FONT_PATH) }],
    });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
  });

  it('rejects a missing file, a file outside baseDir or an invalid declaration', async () => {
    const code = async (p: Promise<unknown>) => ((await p.then(() => null, (e: unknown) => e)) as TurboPdfError).code;
    expect(await code(turbopdf.create({ html: 'x', fonts: [{ family: 'A', path: '/nope/missing.ttf' }] }))).toBe('font_failed');
    expect(await code(turbopdf.create({ html: 'x', fonts: [{ family: 'A', path: 'font.exe' }] }))).toBe('invalid_source');
    expect(await code(turbopdf.create({ html: 'x', fonts: [{ family: 'A' }] }))).toBe('invalid_source');
    expect(await code(turbopdf.create({ html: 'x', fonts: [{ family: 'A', path: 'missing.woff2' }] }))).toBe('font_failed');
    const baseDir = mkdtempSync(join(tmpdir(), 'turbopdf-font-base-'));
    const linked = join(baseDir, 'linked.ttf');
    symlinkSync(FONT_PATH, linked);
    expect(await code(turbopdf.create({ html: 'x', baseDir, fonts: [{ family: 'A', path: linked }] }))).toBe('font_failed');
  });
});

describe('WOFF and WOFF2 fonts (@fontsource)', () => {
  const dir = join(__dirname, 'fixtures', 'pacifico');
  const woff2 = join(dir, 'pacifico-latin-400-normal.woff2');

  it('`fonts` accepts a .woff2 path and .woff / .woff2 bytes', async () => {
    for (const font of [
      { family: 'Script', path: woff2 },
      { family: 'Script', data: readFileSync(woff2) },
      { family: 'Script', data: readFileSync(join(dir, 'pacifico-latin-400-normal.woff')) },
    ]) {
      const pdf = await turbopdf.create({ html: '<p style="font-family: Script">Bonjour</p>', fonts: [font] });
      expect(pdfHas(pdf.toBuffer(), 'Pacifico')).toBe(true);
      expect(pdf.warnings).toEqual([]);
    }
  });

  it('@font-face loads local .woff2 / .woff files, as @fontsource CSS declares them', async () => {
    const css = `@font-face { font-family: 'Pacifico'; font-weight: 400;
      src: url(./pacifico-latin-400-normal.woff2) format('woff2'), url(./pacifico-latin-400-normal.woff) format('woff'); }
      p { font-family: 'Pacifico', cursive }`;
    const pdf = await turbopdf.create({ html: `<style>${css}</style><p>Bonjour</p>`, baseDir: dir });
    expect(pdfHas(pdf.toBuffer(), 'Pacifico')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('a corrupt WOFF2 file is a clear error', async () => {
    const bad = Buffer.concat([Buffer.from('wOF2'), Buffer.alloc(64)]);
    await expect(turbopdf.create({ html: '<p>x</p>', fonts: [{ family: 'Bad', data: bad }] })).rejects.toThrow(/WOFF data could not be decoded/);
  });
});
