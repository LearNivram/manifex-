// Replacement for `monaco-worker-manager/worker` (used by monaco-yaml's worker).
// monaco-editor >= 0.55 expects `initialize` to be called synchronously when the
// worker starts; the original helper deferred it to the first message, which made
// Monaco drop the initial handshake ("Missing requestHandler or method: …").
// @ts-expect-error – deep import without type declarations
import { initialize as initializeWorker } from "monaco-editor/esm/vs/editor/editor.worker.js";

export function initialize<T extends object>(fn: (ctx: unknown, createData: unknown) => T): void {
  initializeWorker((ctx: unknown, createData: unknown) => Object.create(fn(ctx, createData)));
}
