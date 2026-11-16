// A Cloudflare Worker that renders with turbopdf/edge (checked in workerd by scripts/edge-smoke/run.mjs).
import { create, init } from 'turbopdf/edge';
import wasm from 'turbopdf/turbopdf.wasm';

export default {
  async fetch(request) {
    await init(wasm);
    const kind = new URL(request.url).searchParams.get('kind') ?? 'html';
    const pdf =
      kind === 'markdown'
        ? await create({ markdown: '# Edge\n\n| a | b |\n|---|---|\n| 1 | 2 |' })
        : await create({ html: '<h1>Hello from the edge</h1>' + '<p>row</p>'.repeat(80), footer: '{page}/{pages}' });
    if (new URL(request.url).searchParams.has('png')) {
      const [page] = await pdf.toPng({ scale: 1 });
      return new Response(page.toUint8Array(), { headers: { 'content-type': 'image/png' } });
    }
    return pdf.toResponse({ filename: 'edge.pdf' });
  },
};
