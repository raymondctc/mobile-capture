import { Progress } from "./components/ui/progress";
import { Button } from "./components/ui/button";
import { api, type Operation } from "./api";
import { useState } from "react";
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
  const [cancelling, setCancelling] = useState(false);
  const [cancelError, setCancelError] = useState("");
  async function cancelSave(id: string) {
    setCancelling(true);
    setCancelError("");
    try {
      change(await api.cancel(id));
    } catch (cause) {
      setCancelError(String(cause));
    } finally {
      setCancelling(false);
    }
  }
  if (operation.state === "idle") return null;
  if (
    operation.state === "checking_connection" ||
    operation.state === "preparing_certificate"
  )
    return (
      <div
        className="mx-4 flex flex-wrap items-center gap-3 rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs text-[#665026]"
        role="status"
      >
        {operation.state === "checking_connection"
          ? `Connection check · ${operation.remaining_seconds} seconds remaining`
          : operation.message}{" "}
        <span>Open Device setup to continue.</span>
      </div>
    );
  if (operation.state === "saving")
    return (
      <div
        className="mx-4 flex flex-wrap items-center gap-3 border-b border-border bg-white px-3 py-2 text-xs [&_p]:text-muted-foreground"
        role="status"
      >
        <div>
          <strong>Saving capture</strong>
          <p>{operation.stage.replaceAll("_", " ")}</p>
        </div>
        <Progress
          className="h-1.5 min-w-24 max-w-64 flex-1"
          value={
            operation.total === null
              ? undefined
              : (operation.completed / Math.max(operation.total, 1)) * 100
          }
          aria-label="Archive progress"
        />
        <span>
          {operation.completed.toLocaleString()}
          {operation.total !== null
            ? ` / ${operation.total.toLocaleString()}`
            : ""}
        </span>
        <Button
          disabled={!operation.cancel_allowed || cancelling}
          onClick={() => void cancelSave(operation.id)}
        >
          Cancel save
        </Button>
        {cancelError && <span role="alert">{cancelError}</span>}
      </div>
    );
  if (operation.state === "restoring")
    return (
      <div
        className="mx-4 flex flex-wrap items-center gap-3 rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs text-[#665026]"
        role="status"
      >
        <strong>Restoring device routing</strong>
        <span>{operation.message}</span>
      </div>
    );
  if (operation.state === "failed")
    return (
      <div
        className="mx-4 flex flex-wrap items-center justify-between gap-3 rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-xs text-destructive"
        role="alert"
      >
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
          <Button
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
          </Button>
        ) : (
          operation.kind === "save" && (
            <Button
              disabled={!!pending}
              onClick={() =>
                void run("Retrying save", async () =>
                  change(await api.save(operation.directory)),
                )
              }
            >
              Retry save
            </Button>
          )
        )}
      </div>
    );
  if (operation.state === "cancelled")
    return (
      <div
        className="mx-4 flex flex-wrap items-center gap-3 rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs text-[#665026]"
        role="status"
      >
        {operation.message} Raw evidence and any previous archive are preserved.
      </div>
    );
  if (operation.result.kind === "save")
    return (
      <div
        className="mx-4 flex flex-wrap items-center justify-between gap-3 rounded-md border border-[#c8e3d4] bg-[#edf7f1] px-3 py-2 text-xs text-[#23553e]"
        role="status"
      >
        <div>
          <strong>Archive saved</strong>
          <p className="font-mono select-text break-all">
            {operation.result.archive}
          </p>
        </div>
        <Button
          onClick={() =>
            void run("Revealing archive", () =>
              product.reveal("archive", operation.id),
            )
          }
        >
          Reveal archive
        </Button>
      </div>
    );
  return null;
}
