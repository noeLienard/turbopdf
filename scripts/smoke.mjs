// After `npm install <tarball>` in an empty folder: render a PDF through the installed package, with the
// engine that was picked for this machine. Usage: node smoke.mjs [native|wasm]
import { createRequire } from 'node:module';
import { join } from 'node:path';

const require = createRequire(join(process.cwd(), 'noop.js'));
const turbopdf = require('turbopdf');
const pdf = await turbopdf.create({ html: '<h1>Hello</h1><table><thead><tr><th>A</th></tr></thead>' + '<tr><td>row</td></tr>'.repeat(120) + '</table>', footer: '{page}/{pages}' });
const bytes = pdf.toBuffer();
if (bytes.subarray(0, 5).toString() !== '%PDF-' || pdf.pages < 2) throw new Error('not a PDF');
console.log(`${process.platform}-${process.arch}, engine ${process.env.TURBOPDF_ENGINE ?? 'auto'}: ${pdf.pages} pages, ${bytes.length} bytes`);
