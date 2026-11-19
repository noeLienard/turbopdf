import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import turbopdf, { type FieldBox } from '../src/index';
import { Document, Page, View } from '../src/components';

const form = `<h1>Commande</h1>
<label>Nom <input name="nom" value="Jeanne"></label>
<label><input type="checkbox" name="cgv"> J'accepte</label>
<select name="pays"><option value="FR">France</option><option value="BE" selected>Belgique</option></select>`;

describe('form fields', () => {
  it('reports the HTML controls and where they were laid out', async () => {
    const pdf = await turbopdf.create({ html: form });
    expect(pdf.fields.map((f) => [f.name, f.kind, f.value ?? null, f.page])).toEqual([
      ['nom', 'text', 'Jeanne', 1],
      ['cgv', 'checkbox', 'on', 1],
      ['pays', 'select', 'BE', 1],
    ]);
    const [nom, cgv, pays] = pdf.fields as [FieldBox, FieldBox, FieldBox];
    expect(nom.label).toBe('Nom');
    expect(nom.width).toBeCloseTo(160);
    expect(cgv.checked).toBe(false);
    expect(cgv.y).toBeGreaterThan(nom.y);
    expect(pays.options).toEqual(['FR', 'BE']);
    expect(pays.optionLabels).toEqual(['France', 'Belgique']);
  });

  it('a View with a field, from components', async () => {
    const pdf = await turbopdf.create({
      react: createElement(Document, null, createElement(Page, null,
        createElement(View, { style: { width: 200, height: 60 }, field: { name: 'signature', kind: 'signature' } }))),
    });
    expect(pdf.fields).toMatchObject([{ name: 'signature', kind: 'signature', width: 200, height: 60, page: 1 }]);
  });

  it('no fields: an empty list', async () => {
    expect((await turbopdf.create({ html: '<p>Bonjour</p>' })).fields).toEqual([]);
  });
});
