// Run the whole test suite on the WebAssembly engine (TURBOPDF_ENGINE=wasm), on any OS.
import { spawnSync } from 'node:child_process';

const r = spawnSync('npx', ['vitest', 'run'], { stdio: 'inherit', shell: process.platform === 'win32', env: { ...process.env, TURBOPDF_ENGINE: 'wasm' } });
process.exit(r.status ?? 1);
