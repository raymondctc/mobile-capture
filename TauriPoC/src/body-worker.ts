import { createSession, type Request, type Response } from "./body-session";
const execute = createSession();
self.onmessage = (event: MessageEvent<Request>) => {
  const { id, operation } = event.data;
  let response: Response;
  try {
    response = { id, kind: "result", reply: execute(operation) };
  } catch (cause) {
    response = {
      id,
      kind: "error",
      message: cause instanceof Error ? cause.message : String(cause),
    };
  }
  self.postMessage(response);
};
