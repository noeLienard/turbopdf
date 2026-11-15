// Renders the showcase documents of the README: one PDF and one PNG of the first page each.
//   npx tsx examples/showcase/render.tsx
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import turbopdf, { type CreateOptions } from '../../src/index';
import Invoice from './invoice';
import Ticket from './ticket';

const here = import.meta.dirname;
const out = join(here, '../../.github/assets');
const html = (file: string) => readFile(join(here, file), 'utf8');

const docs: Record<string, CreateOptions> = {
  invoice: { react: <Invoice />, margin: 0, title: 'Invoice NW-2026-1142', lang: 'en' },
  report: { html: await html('report.html'), baseDir: here },
  resume: { html: await html('resume.html'), baseDir: here },
  ticket: { react: <Ticket /> },
};

const only = process.argv[2];
for (const [name, options] of Object.entries(docs)) {
  if (only && name !== only) continue;
  const t = performance.now();
  const pdf = await turbopdf.create(options);
  const ms = (performance.now() - t).toFixed(0);
  await pdf.save(join(out, `${name}.pdf`));
  const [first] = await pdf.toPng({ scale: 1.5 });
  await first!.save(join(out, `${name}.png`));
  console.log(`${name}: ${pdf.pages} page(s), ${ms} ms`);
  for (const w of pdf.warnings) console.log(`  ⚠ ${w.type}: ${w.message}`);
}
await turbopdf.close();
