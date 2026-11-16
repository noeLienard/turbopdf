// Build the Rust engine's Node module and copy it to native/turbopdf.<platform>-<arch>.node
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const debug = process.argv.includes('--debug');
// TURBOPDF_ZIG_TARGET=x86_64-unknown-linux-gnu.2.17: build with cargo-zigbuild against that glibc, so the
// module loads on every Linux from that version on (Amazon Linux 2/2023, AWS Lambda, Vercel, Debian 10+).
// A plain build links the build machine's glibc (2.35 on Ubuntu 22.04) and fails to load on older ones.
const zig = process.env.TURBOPDF_ZIG_TARGET;
const triple = zig?.replace(/\.\d+\.\d+$/, '');
execFileSync('cargo', [zig ? 'zigbuild' : 'build', '-p', 'turbopdf-node', ...(debug ? [] : ['--release']), ...(zig ? ['--target', zig] : [])], {
  cwd: join(root, 'engine'),
  stdio: 'inherit',
});

const lib = { darwin: 'libturbopdf_node.dylib', linux: 'libturbopdf_node.so', win32: 'turbopdf_node.dll' }[process.platform];
if (!lib) throw new Error(`Unsupported platform ${process.platform}`);
const out = join(root, 'native', `turbopdf.${process.platform}-${process.arch}.node`);
mkdirSync(dirname(out), { recursive: true });
// new inode: overwriting a loaded/signed binary in place gets the next process SIGKILLed on macOS
rmSync(out, { force: true });
copyFileSync(join(root, 'engine', 'target', ...(triple ? [triple] : []), debug ? 'debug' : 'release', lib), out);
console.log(`→ ${out}`);
