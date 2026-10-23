import { api, type Operation } from "./api";
import { product } from "./product";
import type { RunAction } from "./state";
export function OperationStatus({
  operation,
  change,
  run,
  pending,
  recover,
}: {
  operation: Operation;
  change: (operation: Operation) => void;
  run: RunAction;
  pending: string;
  recover: () => Promise<void>;
}) {
  if (operation.state === "idle") return null;
  if (
    operation.state === "checking_connection" ||
    operation.state === "preparing_certificate"
  )
    return (
      <div className="operation notice" role="status">
        {operation.state === "checking_connection"
          ? `Connection check · ${operation.remaining_seconds} seconds remaining`
          : operation.message}{" "}
        <span>Open Device setup to continue.</span>
      </div>
    );
  if (operation.state === "saving")
    return (
      <div className="operation" role="status">
        <div>
          <strong>Saving capture</strong>
          <p>{operation.stage.replaceAll("_", " ")}</p>
        </div>
        <progress
          value={operation.total === null ? undefined : operation.completed}
          max={operation.total ?? undefined}
          aria-label="Archive progress"
        />
        <span>
          {operation.completed.toLocaleString()}
          {operation.total !== null
            ? ` / ${operation.total.toLocaleString()}`
            : ""}
        </span>
        <button
          disabled={!operation.cancel_allowed || !!pending}
          onClick={() =>
            void run("Cancelling save", async () =>
              change(await api.cancel(operation.id)),
            )
          }
        >
          Cancel save
        </button>
      </div>
    );
  if (operation.state === "restoring")
    return (
      <div className="operation notice" role="status">
        <strong>Restoring device routing</strong>
        <span>{operation.message}</span>
      </div>
    );
  if (operation.state === "failed")
    return (
      <div className="operation error" role="alert">
        <div>
          <strong>
            {operation.recovery_required
              ? "Device recovery required"
              : "Action could not finish"}
          </strong>
          <p>{operation.message}</p>
          <p>Raw evidence and any previous archive remain available.</p>
        </div>
        {operation.recovery_required ? (
          <button
            disabled={!!pending}
            onClick={() =>
              void run(
                "Retrying safe recovery",
                operation.kind === "save"
                  ? recover
                  : async () => change(await api.retryRecovery(operation.id)),
              )
            }
          >
            Retry recovery
          </button>
        ) : (
          operation.kind === "save" && (
            <button
              disabled={!!pending}
              onClick={() =>
                void run("Retrying save", async () =>
                  change(await api.save(operation.directory)),
                )
              }
            >
              Retry save
            </button>
          )
        )}
      </div>
    );
  if (operation.state === "cancelled")
    return (
      <div className="operation notice" role="status">
        {operation.message} Raw evidence and any previous archive are preserved.
      </div>
    );
  if (operation.result.kind === "save")
    return (
      <div className="operation success" role="status">
        <div>
          <strong>Archive saved</strong>
          <p className="mono">{operation.result.archive}</p>
        </div>
        <button
          onClick={() =>
            void run("Revealing archive", () => product.reveal("archive"))
          }
        >
          Reveal archive
        </button>
      </div>
    );
  return null;
}
