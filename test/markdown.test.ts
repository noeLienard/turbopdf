import { describe, expect, it } from 'vitest';
import papier from '../src/index';
import { toTree } from '../src/rust/engine';
import { pageTexts, pdfHas } from './helpers';

const REPORT = `# Rapport mensuel

Voici le **résumé** du mois, avec du \`code\` et un lien https://example.com

## Chiffres

| Mois | CA |
|---|---|
| Octobre | 12 000 € |
| Novembre | 14 500 € |

- [x] Fermer les comptes
- [ ] Revoir le budget
- ~~ancien tarif~~

1. premier
2. second

\`\`\`ts
const total: number = 12;
\`\`\`

> Une citation importante

<!-- pagebreak -->

## Suite

Contenu après le saut de page.`;

describe('markdown → document tree', () => {
  const nodes = async () => (await toTree({ markdown: REPORT })).children[0]!.children;

  it('headings, emphasis and inline code', async () => {
    const n = await nodes();
    expect(n[0]).toEqual({ type: 'heading', id: 'rapport-mensuel', level: 1, content: 'Rapport mensuel' });
    expect(n).toContainEqual({ type: 'heading', id: 'chiffres', level: 2, content: 'Chiffres' });
    const intro = n[1]!;
    expect(intro.type === 'text' && intro.content).toEqual(
      expect.arrayContaining([{ text: 'résumé', style: { fontWeight: 700 } }, { text: 'code', style: { color: '#be185d' } }, { text: 'https://example.com', style: { color: '#2563eb' }, link: 'https://example.com' }]),
    );
  });

  it('GFM table with a header', async () => {
    const table = (await nodes()).find((x) => x.type === 'table');
    expect(table).toMatchObject({ header: [{ cells: [{ children: [{ content: 'Mois' }] }, { children: [{ content: 'CA' }] }] }] });
    expect(table?.type === 'table' && table.rows).toHaveLength(2);
  });

  it('<!-- pagebreak --> becomes a page break', async () => {
    expect((await nodes()).some((x) => x.type === 'page_break')).toBe(true);
  });

  it('raw HTML in Markdown is converted too', async () => {
    const [div] = (await toTree({ markdown: '<div style="color:red">raw</div>\n\ntext' })).children[0]!.children;
    expect(div).toMatchObject({ type: 'view', style: { color: '#ff0000' }, children: [{ type: 'text', content: 'raw' }] });
  });
});

describe('markdown → PDF', () => {
  it('renders a clean report on two pages split at the page break', async () => {
    const pdf = await papier.create({ markdown: REPORT });
    const [p1, p2] = pageTexts(pdf.toBuffer());
    expect(pdf.pages).toBe(2);
    expect(p1).toContain('Rapport mensuel');
    expect(p1).toContain('Octobre');
    expect(p1).toContain('const total');
    expect(p1).toContain('Une citation importante');
    expect(p1).not.toContain('Contenu après');
    expect(p2).toContain('Contenu après le saut de page');
    expect(pdf.warnings).toEqual([]);
  });

  it('a plain string is still HTML, never Markdown', async () => {
    const pdf = await papier.create('# not a heading');
    expect(pageTexts(pdf.toBuffer())[0]).toContain('# not a heading');
  });

  it('`css` is for html / react sources', async () => {
    await expect(papier.create({ markdown: '# Title', css: 'h1{display:none}' })).rejects.toThrow(/`css` applies to `html` and `react`/);
  });

  it('repeats table headers across pages', async () => {
    const rows = Array.from({ length: 120 }, (_, i) => `| Row ${i + 1} | ${i} |`).join('\n');
    const pdf = await papier.create({ markdown: `| NAMECOL | n |\n|---|---|\n${rows}` });
    expect(pdf.pages).toBeGreaterThan(1);
    for (const t of pageTexts(pdf.toBuffer())) expect(t).toContain('NAMECOL');
    expect(pdfHas(pdf.toBuffer(), '%PDF-')).toBe(true);
  });
});
