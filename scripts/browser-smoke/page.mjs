// Bundled for the browser by run.mjs: the public `turbopdf/edge` API, as an app would use it.
import { create, init } from '../../dist/edge.js';
// browser apps build documents with the components too: the bundle must not pull in any Node API
import { Text } from '../../dist/components.js';

if (typeof Text !== 'function') throw new Error('turbopdf/components did not load');

let fonts;

/** Everything that may use the network: the engine (the only download TurboPDF needs) and the app's font. */
window.prepare = async (fontUrl) => {
  await init('/turbopdf.wasm');
  fonts = [{ family: 'Script', data: new Uint8Array(await (await fetch(fontUrl)).arrayBuffer()) }];
};

/** Render and offer the file, as an app would: run.mjs checks that no request leaves the page meanwhile. */
window.render = async (html) => {
  const pdf = await create({ html, fonts, title: 'Browser smoke', lang: 'en' });
  const bytes = pdf.toUint8Array();
  const blob = pdf.toBlob();
  if (blob.type !== 'application/pdf' || blob.size !== bytes.length) throw new Error('toBlob() does not match the PDF');
  URL.revokeObjectURL(URL.createObjectURL(blob));
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return { base64: btoa(binary), pages: pdf.pages, warnings: pdf.warnings };
};
