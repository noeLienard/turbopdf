// Reference benchmark of the whole SDK (source → PDF), through the public API.
//
//   npm run bench               # Markdown table on stdout
//   npm run bench -- --json     # machine-readable, e.g. to compare two branches
//
// Each case runs a few warm-up renders, then reports the median and the fastest of N runs. Numbers depend
// on the machine: compare runs made on the same one.
import { readFileSync } from 'node:fs';
import { cpus } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import turbopdf, { type CreateOptions } from '../src/index';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const showcase = join(root, 'examples/showcase');

const rows = (n: number) =>
  Array.from({ length: n }, (_, i) => `<tr><td>Item ${i + 1}</td><td>Consulting, ${i % 7} days</td><td>${(i * 37) % 900} €</td></tr>`).join('');
const table = (n: number) => `<style>table{width:100%}td,th{padding:4px}tr:nth-child(even){background:#f6f6f6}</style>
<h1>Invoice</h1><table><thead><tr><th>Item</th><th>Description</th><th>Price</th></tr></thead><tbody>${rows(n)}</tbody></table>`;

function frameworkCss(rules: number, elements: number): string {
  let css = '';
  for (let i = 0; i < rules; i++) css += `.c${i}{color:#123456}.row .c${i}>span{padding:1px}\n`;
  const body = Array.from({ length: elements }, (_, i) => `<div class="row c${i % rules}"><span>${i}</span></div>`).join('');
  return `<style>${css}</style>${body}`;
}

const markdown = `# Release notes\n\n${Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nSome **bold** text, a [link](https://example.com) and \`code\`.\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n`).join('\n')}`;

interface Case {
  name: string;
  options: CreateOptions;
  runs: number;
  /** Also time the HTML preview and the PNGs of the same document. */
  outputs?: boolean;
}

const cases: Case[] = [
  { name: 'report (showcase, 1 page, SVG chart)', options: { html: readFileSync(join(showcase, 'report.html'), 'utf8'), baseDir: showcase }, runs: 30 },
  { name: 'résumé (showcase, 1 page, custom font)', options: { html: readFileSync(join(showcase, 'resume.html'), 'utf8'), baseDir: showcase }, runs: 30 },
  { name: 'invoice, 200 table rows', options: { html: table(200) }, runs: 30, outputs: true },
  { name: 'invoice, 8,000 table rows', options: { html: table(8000) }, runs: 5 },
  { name: 'Markdown, 40 sections', options: { markdown }, runs: 30 },
  { name: 'framework CSS: 18,000 rules × 4,000 elements', options: { html: frameworkCss(9000, 2000) }, runs: 5 },
];

async function time(runs: number, f: () => Promise<unknown>): Promise<{ median: number; min: number }> {
  for (let i = 0; i < 2; i++) await f();
  const all: number[] = [];
  for (let i = 0; i < runs; i++) {
    const t = performance.now();
    await f();
    all.push(performance.now() - t);
  }
  all.sort((a, b) => a - b);
  return { median: all[Math.floor(all.length / 2)]!, min: all[0]! };
}

const results: { case: string; output: string; pages: number; medianMs: number; minMs: number }[] = [];
for (const c of cases) {
  const pdf = await turbopdf.create(c.options);
  const add = async (output: string, f: () => Promise<unknown>) => {
    const t = await time(c.runs, f);
    results.push({ case: c.name, output, pages: pdf.pages, medianMs: t.median, minMs: t.min });
  };
  await add('PDF', () => turbopdf.create(c.options));
  if (c.outputs) {
    await add('PDF + HTML preview', async () => (await turbopdf.create(c.options)).preview());
    await add('PDF + PNG (2×)', async () => (await turbopdf.create(c.options)).toPng());
  }
}

if (process.argv.includes('--json')) {
  console.log(JSON.stringify({ node: process.version, cpu: cpus()[0]?.model, results }, null, 2));
} else {
  console.log(`Node ${process.version}, ${cpus()[0]?.model ?? 'unknown CPU'}\n`);
  console.log('| Case | Output | Pages | Median | Fastest |\n|---|---|---:|---:|---:|');
  for (const r of results) console.log(`| ${r.case} | ${r.output} | ${r.pages} | ${r.medianMs.toFixed(1)} ms | ${r.minMs.toFixed(1)} ms |`);
}
