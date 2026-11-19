// PDF/A and PDF/UA conformance, checked by veraPDF (the reference validator, https://verapdf.org): renders
// representative documents at every level with the built package and validates each file. Run after
// `npm run build` and `npm run build:native`, with VERAPDF pointing to the veraPDF command line
// (scripts/install-verapdf.sh installs it in CI).
//
//   VERAPDF=/opt/verapdf/verapdf node scripts/check-pdfa.mjs
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const verapdf = process.env.VERAPDF;
if (!verapdf) throw new Error('Set VERAPDF to the veraPDF command line (scripts/install-verapdf.sh installs it).');

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const { default: turbopdf } = await import(pathToFileURL(join(root, 'dist', 'index.js')).href);
const { createElement: h } = await import('react');
const { Document, Page, Table, Row, Cell, Text, Heading, Link, PageNumber } = await import(pathToFileURL(join(root, 'dist', 'components.js')).href);

const GIF = 'R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==';
const font = readFileSync(join(root, 'test', 'fixtures', 'pacifico', 'pacifico-latin-400-normal.woff2'));
const fonts = [{ family: 'Script', data: font }];
const date = '2026-11-19T10:00:00Z';

/** An invoice with what PDF/A-1 forbids: transparency (rgba, watermark, an image with alpha). */
const invoice = (transparency) => `<!doctype html><html lang="fr"><head><title>Facture 2026-042</title><style>
  h1 { color: #1d4ed8 } td, th { padding: 4px; border-bottom: 1px solid #e5e7eb } .total { font-weight: 700 }
  ${transparency ? '.badge { background: rgba(22, 163, 74, .25) }' : '.badge { background: #dcfce7 }'}
</style></head><body>
  <h1>Facture 2026-042 ✔</h1>
  <p style="font-family: Script">Merci pour votre confiance</p>
  <p class="badge">Payée le 19/11/2026 — <a href="https://example.com/factures/42">voir en ligne</a></p>
  <table><thead><tr><th>Désignation</th><th>Montant</th></tr></thead><tbody>
  ${Array.from({ length: 30 }, (_, i) => `<tr><td>Prestation ${i + 1}</td><td>${(i + 1) * 70},00 €</td></tr>`).join('')}
  </tbody></table>
  <p class="total">Total TTC : 32 550,00 €</p>
  <svg width="60" height="20" role="img" aria-label="Logo"><rect width="60" height="20" fill="#1d4ed8"/></svg>
  ${transparency ? `<img src="data:image/gif;base64,${GIF}" alt="Tampon" width="20">` : ''}
  <img src="qr:https://example.com/pay/42" width="80" alt="Payer en ligne">
</body></html>`;

const report = (pdfa) =>
  h(
    Document,
    { title: 'Rapport', lang: 'fr', creationDate: date, pdfa },
    h(
      Page,
      { footer: h(PageNumber) },
      h(Heading, { level: 1 }, 'Rapport mensuel'),
      h(Text, null, 'Voir ', h(Link, { href: 'https://example.com' }, 'le site'), '.'),
      h(
        Table,
        { columns: [{ width: 'auto' }, { width: 80 }] },
        h(Row, { header: true }, h(Cell, null, h(Text, null, 'Mois')), h(Cell, null, h(Text, null, 'CA'))),
        ...['Sept', 'Oct', 'Nov'].map((m, i) => h(Row, { key: m }, h(Cell, null, h(Text, null, m)), h(Cell, null, h(Text, null, String(10 + i))))),
      ),
    ),
  );

const cases = [];
for (const level of ['2b', '2u', '2a', '3b', '3u', '3a', '4']) {
  cases.push([`invoice-${level}`, { html: invoice(true), fonts, pdfa: level, watermark: 'COPIE' }]);
  cases.push([`report-${level}`, report(level)]);
  cases.push([`markdown-${level}`, { markdown: '# Note\n\n- un\n- deux\n\n| a | b |\n|---|---|\n| 1 | 2 |', lang: 'fr', pdfa: level }]);
}
for (const level of ['1b', '1a']) cases.push([`invoice-${level}`, { html: invoice(false), fonts, pdfa: level }]);
// PDF/A and PDF/UA at once: validated against both
const ua = [['invoice-3a-ua', { html: invoice(true), fonts, pdfa: '3a', pdfua: true }], ['report-ua', { html: invoice(true), fonts, pdfua: true }]];

const dir = mkdtempSync(join(tmpdir(), 'turbopdf-pdfa-'));
const write = async ([name, input]) => {
  const file = join(dir, `${name}.pdf`);
  writeFileSync(file, (await turbopdf.create(input)).toBuffer());
  return file;
};
const files = await Promise.all(cases.map(write));
const uaFiles = await Promise.all(ua.map(write));

/** Run veraPDF (machine-readable report) and return, per file, the failed rules. */
function validate(list, flavour) {
  let xml;
  try {
    xml = execFileSync(verapdf, ['--format', 'mrr', ...(flavour ? ['--flavour', flavour] : []), ...list], { encoding: 'utf8', maxBuffer: 64 << 20 });
  } catch (e) {
    // veraPDF exits non-zero when a file is not compliant: the report is still on stdout
    xml = e.stdout;
    if (!xml) throw e;
  }
  return [...xml.matchAll(/<job>([\s\S]*?)<\/job>/g)].map(([, job]) => {
    const name = job.match(/<name>([^<]*)<\/name>/)?.[1] ?? '?';
    const report = job.match(/<validationReport[^>]*>/)?.[0] ?? '';
    const profile = report.match(/profileName="([^"]*)"/)?.[1] ?? '?';
    const ok = /isCompliant="true"/.test(report);
    const failed = [...job.matchAll(/<rule [^>]*clause="([^"]*)"[^>]*status="failed"[^>]*>\s*<description>([^<]*)<\/description>/g)].map(([, clause, d]) => `${clause}: ${d.trim()}`);
    if (!report) failed.push(job.match(/<taskException[^>]*>[\s\S]*?<\/taskException>/)?.[0] ?? 'no validation report');
    return { name: name.split(/[\\/]/).pop(), profile, ok: ok && failed.length === 0, failed };
  });
}

const results = [...validate([...files, uaFiles[0]]), ...validate(uaFiles, 'ua1')];
let bad = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.name} (${r.profile})`);
  for (const f of r.failed) console.log(`     ${f}`);
  if (!r.ok) bad++;
}
const expected = files.length + 1 + uaFiles.length;
if (results.length !== expected) throw new Error(`veraPDF reported ${results.length} files, expected ${expected}`);
if (bad) throw new Error(`${bad} file(s) are not conformant`);
console.log(`ok: ${results.length} validations passed (veraPDF)`);
