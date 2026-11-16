// Build the engine for WebAssembly and copy it to wasm/turbopdf.wasm: the engine used where no native
// binary exists (any OS or CPU). Needs `rustup target add wasm32-unknown-unknown`.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
execFileSync('cargo', ['build', '--release', '-p', 'turbopdf-wasm', '--target', 'wasm32-unknown-unknown'], { cwd: join(root, 'engine'), stdio: 'inherit' });
const out = join(root, 'wasm', 'turbopdf.wasm');
mkdirSync(dirname(out), { recursive: true });
rmSync(out, { force: true });
copyFileSync(join(root, 'engine', 'target', 'wasm32-unknown-unknown', 'release', 'turbopdf_wasm.wasm'), out);
console.log(`→ ${out} (${(statSync(out).size / 1e6).toFixed(1)} MB)`);
