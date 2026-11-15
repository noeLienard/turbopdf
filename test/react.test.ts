import { createElement } from 'react';
import { describe, expect, it } from 'vitest';
import turbopdf from '../src/index';
import { pageTexts } from './helpers';

function Invoice({ customer }: { customer: string }) {
  return createElement('div', null, createElement('h1', null, 'Facture'), createElement('p', null, `Client : ${customer}`));
}

describe('react', () => {
  it('create(<Element />) shorthand', async () => {
    const pdf = await turbopdf.create(createElement(Invoice, { customer: 'Acme' }));
    expect(pageTexts(pdf.toBuffer())[0]).toContain('Client : Acme');
  });

  it('create({ react })', async () => {
    const pdf = await turbopdf.create({ react: createElement(Invoice, { customer: 'Globex' }), format: 'A5' });
    expect(pageTexts(pdf.toBuffer())[0]).toContain('Client : Globex');
  });

  it('a component that throws gives render_failed', async () => {
    const Boom = () => {
      throw new Error('kaboom');
    };
    await expect(turbopdf.create({ react: createElement(Boom) })).rejects.toMatchObject({ code: 'render_failed' });
  });

  it('rejects something that is not an element', async () => {
    await expect(turbopdf.create({ react: { nope: true } as never })).rejects.toMatchObject({ code: 'invalid_source' });
  });
});
