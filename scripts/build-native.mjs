// Build the Rust engine's Node module and copy it to native/papier.<platform>-<arch>.node
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const debug = process.argv.includes('--debug');
execFileSync('cargo', ['build', '-p', 'papier-node', ...(debug ? [] : ['--release'])], { cwd: join(root, 'engine'), stdio: 'inherit' });

const lib = { darwin: 'libpapier_node.dylib', linux: 'libpapier_node.so', win32: 'papier_node.dll' }[process.platform];
if (!lib) throw new Error(`Unsupported platform ${process.platform}`);
const out = join(root, 'native', `papier.${process.platform}-${process.arch}.node`);
mkdirSync(dirname(out), { recursive: true });
// new inode: overwriting a loaded/signed binary in place gets the next process SIGKILLed on macOS
rmSync(out, { force: true });
copyFileSync(join(root, 'engine', 'target', debug ? 'debug' : 'release', lib), out);
console.log(`→ ${out}`);
