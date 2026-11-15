// Same tree → same PDF bytes, native (napi) vs WebAssembly. Run from engine/ after `npm run build:native`
// (at the repo root) and `cargo build --release -p papier-wasm --target wasm32-unknown-unknown`.
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { load } from './papier.mjs';

const native = createRequire(import.meta.url)(`../../native/papier.${process.platform}-${process.arch}.node`);
const wasm = await load();
const md5 = (b) => createHash('md5').update(b).digest('hex');
let failed = 0;
for (const f of readdirSync(new URL('../fixtures', import.meta.url)).filter((f) => f.endsWith('.json'))) {
  const tree = readFileSync(new URL(`../fixtures/${f}`, import.meta.url), 'utf8');
  if (tree.includes('"file"')) {
    console.log(`${f}: skipped (file sources)`);
    continue;
  }
  const a = (await native.render(tree)).pdf;
  let t = performance.now();
  const b = wasm.render(tree).pdf;
  const ms = performance.now() - t;
  const same = md5(a) === md5(b);
  if (!same) failed++;
  console.log(`${f}: native ${md5(a)} | wasm ${md5(b)} ${same ? 'identical' : 'DIFFERENT'} (wasm ${ms.toFixed(1)} ms)`);
}
process.exit(failed ? 1 : 0);
