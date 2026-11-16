// Render a PDF with the installed package and require the native engine (not the WebAssembly fallback).
import turbopdf, { engineInfo } from 'turbopdf';

const info = engineInfo();
console.log(info);
if (info.kind !== 'native') throw new Error(`expected the native engine, got ${info.kind}: ${info.fallbackReason}`);
const pdf = await turbopdf.create('<h1>Smoke ✔</h1><p>Amazon Linux, Lambda, Debian</p>');
const head = new TextDecoder().decode(pdf.toBuffer().subarray(0, 5));
if (head !== '%PDF-' || pdf.warnings.length) throw new Error(`bad PDF: ${head} ${JSON.stringify(pdf.warnings)}`);
console.log(`ok: ${pdf.toBuffer().length} bytes`);
