// Worker thread running the WebAssembly engine for Node, Bun and Deno, so renders never block the event
// loop. Built to dist/wasm-worker.cjs; `wasm.ts` starts it with the compiled module in `workerData`.
import { parentPort, workerData } from 'node:worker_threads';
import { nodeFiles } from './node-files';
import { core, type Core } from './wasm-core';

const engine = core((workerData as { turbopdf: WebAssembly.Module }).turbopdf, nodeFiles);

parentPort!.on('message', ({ id, op, args }: { id: number; op: keyof Core; args: unknown[] }) => {
  try {
    const result = (engine[op] as (...a: unknown[]) => unknown)(...args);
    // PDF / PNG bytes move to the caller without a copy
    const transfer: ArrayBuffer[] = [];
    const own = (u8: Uint8Array) => {
      if (u8.byteOffset === 0 && u8.byteLength === u8.buffer.byteLength) transfer.push(u8.buffer as ArrayBuffer);
    };
    if (op === 'render') own((result as { pdf: Uint8Array }).pdf);
    if (op === 'png') (result as Uint8Array[]).forEach(own);
    if (op === 'convertHtml') (result as { buffers: Uint8Array[] }).buffers.forEach(own);
    parentPort!.postMessage({ id, result }, transfer);
  } catch (e) {
    parentPort!.postMessage({ id, error: (e as Error).message });
  }
});
