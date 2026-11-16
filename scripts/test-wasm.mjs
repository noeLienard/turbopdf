// Run the whole test suite on the WebAssembly engine (TURBOPDF_ENGINE=wasm), on any OS. Builds first, so
// renders go through the worker thread (dist/wasm-worker.cjs) as in the published package.
import { spawnSync } from 'node:child_process';

const run = (cmd, args, env = {}) => {
  const r = spawnSync(cmd, args, { stdio: 'inherit', shell: process.platform === 'win32', env: { ...process.env, ...env } });
  if (r.status !== 0) process.exit(r.status ?? 1);
};
run('npx', ['tsup', '--silent']);
run('npx', ['vitest', 'run'], { TURBOPDF_ENGINE: 'wasm' });
