// Remote images, downloaded by the SDK before rendering, only from the origins listed in `allowRemote`.
// The engine itself never uses the network: it receives the bytes like any other image.
//
// Guarded against server-side request forgery: a URL is fetched only when its origin (scheme, host, port)
// and path prefix match an allowed entry, every redirect is checked again, credentials in URLs are refused,
// and the response must be an image under the size limit, delivered before the time limit.
import type { Document } from '../tree';
import { toBase64 } from '../bytes';
import { TurboPdfError } from '../errors';
import type { Warning } from '../types';

/** Largest image downloaded, in bytes. */
export const MAX_REMOTE_BYTES = 20 * 1024 * 1024;
/** Time limit for one image, redirects included, in ms (the render's `timeout` still applies). */
export const REMOTE_TIMEOUT = 10_000;
const MAX_REDIRECTS = 3;
const PARALLEL = 6;

interface Allowed {
  origin: string;
  path: string;
}

/** `allowRemote` entries → origins and path prefixes. Throws `invalid_source` on a bad entry. */
export function allowList(entries: unknown): Allowed[] {
  if (entries === undefined) return [];
  if (!Array.isArray(entries)) throw bad('`allowRemote` must be an array of URLs, e.g. [\'https://cdn.example.com\'].');
  return entries.map((e) => {
    let u: URL;
    try {
      u = new URL(String(e));
    } catch {
      throw bad(`\`allowRemote\`: "${String(e)}" is not a URL. Write the origin, e.g. 'https://cdn.example.com'.`);
    }
    if (u.protocol !== 'https:' && u.protocol !== 'http:') throw bad(`\`allowRemote\`: "${String(e)}" must be an http(s) URL.`);
    if (u.username || u.password) throw bad(`\`allowRemote\`: "${String(e)}" must not contain credentials.`);
    if (u.search || u.hash) throw bad(`\`allowRemote\`: "${String(e)}" must not have a query or a fragment.`);
    // a path prefix ends with `/`, so `/img` does not allow `/img-private`
    const path = u.pathname.endsWith('/') ? u.pathname : `${u.pathname}/`;
    return { origin: u.origin, path };
  });
}

const bad = (message: string) => new TurboPdfError('invalid_source', message);

/** Whether `url` may be fetched. The URL parser has already resolved `..` and `%2e%2e` segments. */
export function allowed(url: URL, list: Allowed[]): boolean {
  if (url.username || url.password) return false;
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return false;
  return list.some((a) => a.origin === url.origin && (a.path === '/' || url.pathname.startsWith(a.path)));
}

/** The image sources of the tree that point at the network: `{ file: 'https://…' }`. */
function remoteSources(doc: Document): { file: string }[] {
  const out: { file: string }[] = [];
  const walk = (v: unknown, key?: string) => {
    if (Array.isArray(v)) for (const x of v) walk(x);
    else if (v && typeof v === 'object') {
      const o = v as Record<string, unknown>;
      if (key === 'src' && typeof o.file === 'string' && /^https?:\/\//i.test(o.file)) out.push(o as { file: string });
      for (const [k, x] of Object.entries(o)) walk(x, k);
    }
  };
  walk(doc);
  return out;
}

async function download(start: string, list: Allowed[], signal: AbortSignal): Promise<Uint8Array> {
  let url = new URL(start);
  for (let hop = 0; ; hop++) {
    if (!allowed(url, list)) throw new Error(hop ? `it redirects to ${url.href}, which \`allowRemote\` does not allow` : 'not allowed');
    const res = await fetch(url, { redirect: 'manual', credentials: 'omit', signal, headers: { accept: 'image/*' } });
    if (res.status >= 300 && res.status < 400) {
      const to = res.headers.get('location');
      await res.body?.cancel();
      if (!to) throw new Error(`HTTP ${res.status} without a Location header`);
      if (hop >= MAX_REDIRECTS) throw new Error(`more than ${MAX_REDIRECTS} redirects`);
      url = new URL(to, url);
      continue;
    }
    if (!res.ok) {
      await res.body?.cancel();
      throw new Error(`HTTP ${res.status}`);
    }
    const type = (res.headers.get('content-type') ?? '').split(';')[0]!.trim().toLowerCase();
    if (!type.startsWith('image/')) {
      await res.body?.cancel();
      throw new Error(`the server answered ${type || 'without a content type'}, not an image`);
    }
    const declared = Number(res.headers.get('content-length'));
    if (declared > MAX_REMOTE_BYTES) {
      await res.body?.cancel();
      throw new Error(`it is larger than ${MAX_REMOTE_BYTES / 1024 / 1024} MB`);
    }
    return readCapped(res);
  }
}

/** The body, stopped as soon as it passes the size limit (a wrong or missing Content-Length is not trusted). */
async function readCapped(res: Response): Promise<Uint8Array> {
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_REMOTE_BYTES) {
      await reader.cancel();
      throw new Error(`it is larger than ${MAX_REMOTE_BYTES / 1024 / 1024} MB`);
    }
    chunks.push(value);
  }
  const out = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.byteLength;
  }
  return out;
}

/**
 * Download the allowed remote images of `doc` and point their sources at the bytes (`{ buffer: n }` in
 * `buffers`, or base64 without it). An image that fails keeps its URL and gets an `image_failed` warning;
 * returns those URLs, whose engine warning ("is remote") would repeat it.
 */
export async function fetchRemoteImages(
  doc: Document,
  allowRemote: unknown,
  warnings: Warning[],
  buffers: Uint8Array[] | undefined,
  signal: AbortSignal | undefined,
): Promise<Set<string>> {
  const failed = new Set<string>();
  const list = allowList(allowRemote);
  if (!list.length) return failed;
  const wanted = new Map<string, { file: string }[]>();
  for (const s of remoteSources(doc)) {
    let url: URL;
    try {
      url = new URL(s.file);
    } catch {
      continue;
    }
    if (!allowed(url, list)) continue; // the engine reports it, with how to allow it
    wanted.set(s.file, [...(wanted.get(s.file) ?? []), s]);
  }
  const urls = [...wanted.keys()];
  const worker = async () => {
    for (let url = urls.shift(); url !== undefined; url = urls.shift()) {
      const limit = AbortSignal.timeout(REMOTE_TIMEOUT);
      try {
        const both = signal && typeof AbortSignal.any === 'function' ? AbortSignal.any([signal, limit]) : limit;
        const bytes = await download(url, list, both);
        const src = buffers ? { buffer: buffers.push(bytes) - 1 } : { data: toBase64(bytes) };
        for (const s of wanted.get(url)!) {
          delete (s as Partial<typeof s>).file;
          Object.assign(s, src);
        }
      } catch (e) {
        if (signal?.aborted) throw new TurboPdfError('aborted', 'Rendering was aborted.');
        const reason = limit.aborted ? `no answer within ${REMOTE_TIMEOUT / 1000} s` : e instanceof Error ? e.message : String(e);
        failed.add(url);
        warnings.push({ type: 'image_failed', url, message: `Image ${url} could not be downloaded: ${reason}.` });
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(PARALLEL, urls.length) }, worker));
  return failed;
}
