import {
  createSession,
  type Operation,
  type Reply,
  type Request,
  type Response,
} from "./body-session";

export type DocumentOwner = ReturnType<typeof createOwner>;
function createOwner() {
  let disposed = false;
  let fault: string | null = null;
  let sequence = 0;
  const pending = new Map<
    number,
    { resolve: (reply: Reply) => void; reject: (cause: Error) => void }
  >();
  const worker =
    typeof Worker === "undefined"
      ? null
      : new Worker(new URL("./body-worker.ts", import.meta.url), {
          type: "module",
        });
  const execute = createSession();
  function fail(message: string) {
    fault = message;
    for (const request of pending.values()) request.reject(new Error(message));
    pending.clear();
  }
  if (worker) {
    worker.onmessage = (event: MessageEvent<Response>) => {
      const response = event.data;
      const request = pending.get(response.id);
      if (!request) return;
      pending.delete(response.id);
      if (response.kind === "error")
        request.reject(new Error(response.message));
      else request.resolve(response.reply);
    };
    worker.onerror = () =>
      fail("Body document worker failed. Reopen this event to retry.");
  }
  return {
    async read(operation: Operation): Promise<Reply> {
      if (disposed) throw new Error("Body document was closed");
      if (fault) throw new Error(fault);
      if (!worker) return execute(operation);
      const id = ++sequence;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        const request: Request = { id, operation };
        try {
          worker.postMessage(request);
        } catch (cause) {
          pending.delete(id);
          reject(cause);
        }
      });
    },
    dispose() {
      disposed = true;
      worker?.terminate();
      fail("Body document was closed");
    },
  };
}
export const bodyDocuments = { create: createOwner };
