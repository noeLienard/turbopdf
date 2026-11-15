import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import turbopdf from '../src/index';

describe('output', () => {
  it('save() writes the file and creates missing folders', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'turbopdf-'));
    const pdf = await turbopdf.create('<h1>Saved</h1>');
    const file = join(dir, 'a', 'b', 'facture.pdf');
    await pdf.save(file);
    expect(readFileSync(file).equals(pdf.toBuffer())).toBe(true);
  });

  it('toBuffer() is a real PDF', async () => {
    const buf = (await turbopdf.create('<p>x</p>')).toBuffer();
    expect(Buffer.isBuffer(buf)).toBe(true);
    expect(buf.subarray(0, 5).toString()).toBe('%PDF-');
    expect(buf.subarray(-6).toString()).toContain('%%EOF');
  });

  it('toResponse() builds a web Response with the right headers', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    const res = pdf.toResponse({ filename: 'Facture été.pdf', disposition: 'attachment' });
    expect(res).toBeInstanceOf(Response);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('application/pdf');
    expect(res.headers.get('content-length')).toBe(String(pdf.toBuffer().length));
    const cd = res.headers.get('content-disposition')!;
    expect(cd).toContain('attachment');
    expect(cd).toContain('filename="Facture _t_.pdf"');
    expect(cd).toContain("filename*=UTF-8''Facture%20%C3%A9t%C3%A9.pdf");
    expect(Buffer.from(await res.arrayBuffer()).equals(pdf.toBuffer())).toBe(true);
  });

  it('toResponse() defaults to inline and accepts extra headers', async () => {
    const res = (await turbopdf.create('<p>x</p>')).toResponse({ headers: { 'Cache-Control': 'no-store' } });
    expect(res.headers.get('content-disposition')).toContain('inline; filename="document.pdf"');
    expect(res.headers.get('cache-control')).toBe('no-store');
  });

  it('toStream() streams the same bytes', async () => {
    const pdf = await turbopdf.create('<p>x</p>');
    const chunks: Buffer[] = [];
    for await (const c of pdf.toStream()) chunks.push(c as Buffer);
    expect(Buffer.concat(chunks).equals(pdf.toBuffer())).toBe(true);
  });
});

describe('deterministic (always)', () => {
  const html = '<h1>Same</h1><p>input</p>';
  it('gives byte-identical PDFs for the same input', async () => {
    const a = await turbopdf.create({ html });
    await new Promise((r) => setTimeout(r, 1100)); // the clock moves between the two renders
    const b = await turbopdf.create({ html });
    expect(a.toBuffer().equals(b.toBuffer())).toBe(true);
    expect(a.toBuffer().toString('latin1')).not.toMatch(/D:2026/);
  });

  it('stays a valid, readable PDF', async () => {
    const pdf = await turbopdf.create({ html });
    expect(pdf.pages).toBe(1);
    expect(await pdf.toPng({ scale: 1 })).toHaveLength(1);
  });
});
