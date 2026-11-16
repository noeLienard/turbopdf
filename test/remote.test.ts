// `allowRemote`: images downloaded by the SDK from allowed origins only, against a local HTTP server.
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import turbopdf, { TurboPdfError } from '../src/index';
import type { Document } from '../src/tree';

// 1×1 PNG
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');

let server: Server;
let other: Server;
let base = '';
let otherBase = '';
const hits: string[] = [];

const listen = (s: Server) => new Promise<string>((r) => s.listen(0, '127.0.0.1', () => r(`http://127.0.0.1:${(s.address() as AddressInfo).port}`)));

beforeAll(async () => {
  server = createServer((req, res) => {
    hits.push(req.url!);
    if (req.url === '/img/logo.png') return res.writeHead(200, { 'content-type': 'image/png' }).end(PNG);
    if (req.url === '/img/hop.png') return res.writeHead(302, { location: '/img/logo.png' }).end();
    if (req.url === '/img/away.png') return res.writeHead(302, { location: `${otherBase}/secret.png` }).end();
    if (req.url === '/img/page.png') return res.writeHead(200, { 'content-type': 'text/html' }).end('<h1>login</h1>');
    if (req.url === '/img/huge.png') {
      res.writeHead(200, { 'content-type': 'image/png' }); // no content-length: streamed
      const chunk = Buffer.alloc(1024 * 1024);
      for (let i = 0; i < 21; i++) res.write(chunk);
      return res.end();
    }
    if (req.url === '/private.png') return res.writeHead(200, { 'content-type': 'image/png' }).end(PNG);
    res.writeHead(404).end();
  });
  other = createServer((req, res) => {
    hits.push(`other${req.url}`);
    res.writeHead(200, { 'content-type': 'image/png' }).end(PNG);
  });
  base = await listen(server);
  otherBase = await listen(other);
});
afterAll(() => {
  server.close();
  other.close();
});

const img = (src: string) => `<img src="${src}" alt="logo" width="20">`;

describe('allowRemote', () => {
  it('downloads images from an allowed origin, following redirects inside it', async () => {
    const pdf = await turbopdf.create({ html: img(`${base}/img/logo.png`) + img(`${base}/img/hop.png`), allowRemote: [base] });
    expect(pdf.warnings).toEqual([]);
  });

  it('never downloads without allowRemote, or outside the allowed origins and paths', async () => {
    hits.length = 0;
    const none = await turbopdf.create({ html: img(`${base}/img/logo.png`) });
    expect(none.warnings.map((w) => w.message).join()).toMatch(/allowRemote/);
    const outside = await turbopdf.create({ html: img(`${otherBase}/x.png`) + img(`${base}/private.png`) + img(`${base}/img/../private.png`), allowRemote: [`${base}/img`] });
    expect(outside.warnings.filter((w) => w.type === 'image_failed')).toHaveLength(3);
    expect(hits).toEqual([]);
  });

  it('refuses a redirect out of the list, a page that is not an image and a body over 20 MB', async () => {
    hits.length = 0;
    const pdf = await turbopdf.create({ html: ['away', 'page', 'huge'].map((n) => img(`${base}/img/${n}.png`)).join(''), allowRemote: [`${base}/img/`] });
    const msgs = pdf.warnings.filter((w) => w.type === 'image_failed').map((w) => `${w.url} ${w.message}`);
    expect(msgs).toHaveLength(3);
    expect(msgs.join('\n')).toMatch(/away\.png.*redirects to/);
    expect(msgs.join('\n')).toMatch(/page\.png.*text\/html, not an image/);
    expect(msgs.join('\n')).toMatch(/huge\.png.*larger than 20 MB/);
    expect(hits.some((h) => h.startsWith('other'))).toBe(false);
  });

  it('works for Markdown and trees, without changing the tree passed in', async () => {
    const md = await turbopdf.create({ markdown: `![logo](${base}/img/logo.png)`, allowRemote: [base] });
    expect(md.warnings).toEqual([]);
    const tree: Document = { type: 'document', version: 1, children: [{ type: 'page', children: [{ type: 'image', src: { file: `${base}/img/logo.png` }, alt: 'logo' }] }] };
    const before = JSON.stringify(tree);
    const t = await turbopdf.create({ tree, allowRemote: [base] });
    expect(t.warnings).toEqual([]);
    expect(JSON.stringify(tree)).toBe(before);
  });

  it('rejects malformed entries', async () => {
    for (const allowRemote of ['https://x.fr', ['not a url'], ['ftp://x.fr'], ['https://u:p@x.fr'], ['https://x.fr/?q=1']]) {
      const e = await turbopdf.create({ html: 'x', allowRemote: allowRemote as string[] }).catch((e: unknown) => e);
      expect(e).toBeInstanceOf(TurboPdfError);
      expect((e as TurboPdfError).code).toBe('invalid_source');
    }
  });
});
