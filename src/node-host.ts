// Node, Bun and Deno: the native engine for this machine, else WebAssembly in a worker; files from the
// working directory unless `baseDir` says otherwise.
import { loadNative } from './rust/native';
import { TurboPdfError } from './errors';
import { setHost } from './rust/provider';

async function react(): Promise<(element: unknown) => string> {
  try {
    const server = (await import('react-dom/server')) as unknown as { renderToStaticMarkup(el: unknown): string };
    return server.renderToStaticMarkup;
  } catch (cause) {
    throw new TurboPdfError('react_not_installed', 'A React element was passed but "react" and "react-dom" are not installed. Run: npm install react react-dom', { cause });
  }
}

setHost({ engine: loadNative, baseDir: () => process.cwd(), react });
