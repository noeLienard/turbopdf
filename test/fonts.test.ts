import { mkdtempSync, readFileSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import papier, { PapierError } from '../src/index';
import { FONT_GLYPH, FONT_PATH, pdfHas } from './helpers';

describe('fonts', () => {
  it('Inter is built in: no font to install, accents and symbols included', async () => {
    const pdf = await papier.create('<p>Été — 1 200,50 € ✓</p>');
    expect(pdfHas(pdf.toBuffer(), 'Inter')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('@font-face-free CSS families fall back to Inter without failing', async () => {
    const pdf = await papier.create('<p style="font-family: Helvetica, Arial, sans-serif">texte</p>');
    expect(pdfHas(pdf.toBuffer(), 'Inter')).toBe(true);
  });

  it('`fonts` registers a local file with no network', async () => {
    const pdf = await papier.create({
      html: `<p style="font-family:Brand;font-size:30px">${FONT_GLYPH}</p>`,
      fonts: [{ family: 'Brand', path: FONT_PATH }],
    });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
    expect(pdf.warnings).toEqual([]);
  });

  it('`fonts` accepts raw bytes', async () => {
    const pdf = await papier.create({
      html: `<p style="font-family:Bytes;font-size:30px">${FONT_GLYPH}</p>`,
      fonts: [{ family: 'Bytes', data: readFileSync(FONT_PATH) }],
    });
    expect(pdfHas(pdf.toBuffer(), 'codicon')).toBe(true);
  });

  it('rejects a missing file or an invalid declaration', async () => {
    const code = async (p: Promise<unknown>) => ((await p.then(() => null, (e: unknown) => e)) as PapierError).code;
    expect(await code(papier.create({ html: 'x', fonts: [{ family: 'A', path: '/nope/missing.ttf' }] }))).toBe('invalid_source');
    expect(await code(papier.create({ html: 'x', fonts: [{ family: 'A', path: 'font.exe' }] }))).toBe('invalid_source');
    expect(await code(papier.create({ html: 'x', fonts: [{ family: 'A' }] }))).toBe('invalid_source');
    expect(await code(papier.create({ html: 'x', fonts: [{ family: 'A', path: 'brand.woff2' }] }))).toBe('invalid_source');
    const baseDir = mkdtempSync(join(tmpdir(), 'papier-font-base-'));
    const linked = join(baseDir, 'linked.ttf');
    symlinkSync(FONT_PATH, linked);
    expect(await code(papier.create({ html: 'x', baseDir, fonts: [{ family: 'A', path: linked }] }))).toBe('invalid_source');
  });
});
