// Node, Bun and Deno: the native engine for this machine, else WebAssembly in a worker; files from the
// working directory unless `baseDir` says otherwise.
import { loadNative } from './rust/native';
import { setHost } from './rust/provider';

setHost({ engine: loadNative, baseDir: () => process.cwd() });
