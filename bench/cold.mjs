// Cold start and warm render of the built package (dist/), native and WebAssembly, as a serverless
// function sees them: a new process, `import 'turbopdf'`, the first PDF, then the next ones.
//
//   npm run build && node bench/cold.mjs           # Markdown table
//   node bench/cold.mjs --json                     # machine-readable
//
// Each engine runs in RUNS fresh processes; the table gives medians. Numbers depend on the machine:
// compare runs made on the same one.
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const RUNS = 7;
const WARM = 20;

if (process.argv[2] === '--child') {
  const t0 = performance.now();
  const { default: turbopdf } = await import(pathToFileURL(join(root, 'dist', 'index.js')).href);
  const t1 = performance.now();
  const html = `<h1>Invoice 42</h1><table>${Array.from({ length: 30 }, (_, i) => `<tr><td>Item ${i}</td><td>${i * 7} €</td></tr>`).join('')}</table>`;
  await turbopdf.create(html);
  const t2 = performance.now();
  const warm = [];
  for (let i = 0; i < WARM; i++) {
    const s = performance.now();
    await turbopdf.create(html);
    warm.push(performance.now() - s);
  }
  warm.sort((a, b) => a - b);
  const info = typeof turbopdf.engineInfo === 'function' ? (await turbopdf.engineInfo()).kind : process.env.TURBOPDF_ENGINE;
  process.stdout.write(JSON.stringify({ engine: info, import: t1 - t0, first: t2 - t1, warm: warm[warm.length >> 1] }));
  await turbopdf.close();
  process.exit(0);
}

const median = (xs) => [...xs].sort((a, b) => a - b)[xs.length >> 1];
const results = {};
for (const engine of ['native', 'wasm']) {
  const runs = [];
  for (let i = 0; i < RUNS; i++) {
    const s = performance.now();
    const out = execFileSync(process.execPath, [fileURLToPath(import.meta.url), '--child'], { env: { ...process.env, TURBOPDF_ENGINE: engine } });
    runs.push({ ...JSON.parse(out.toString()), process: performance.now() - s });
  }
  results[engine] = {
    engine: runs[0].engine,
    import: median(runs.map((r) => r.import)),
    first: median(runs.map((r) => r.first)),
    coldTotal: median(runs.map((r) => r.import + r.first)),
    warm: median(runs.map((r) => r.warm)),
  };
}

if (process.argv.includes('--json')) console.log(JSON.stringify(results, null, 2));
else {
  const ms = (n) => `${n.toFixed(1)} ms`;
  console.log(`Node ${process.version}, ${process.platform}-${process.arch}, median of ${RUNS} processes\n`);
  console.log('| engine | import | first PDF | cold total | warm render |');
  console.log('|---|---|---|---|---|');
  for (const [k, r] of Object.entries(results)) console.log(`| ${k} (${r.engine}) | ${ms(r.import)} | ${ms(r.first)} | ${ms(r.coldTotal)} | ${ms(r.warm)} |`);
}
