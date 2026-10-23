import { useEffect, useState } from "react";
import {
  api,
  captureSafe,
  errorMessage,
  operationBusy,
  type Operation,
  type Snapshot,
} from "./api";
import { product, type SessionDescriptor, type catalogSchema } from "./product";
import type { z } from "zod";
import type { RunAction } from "./state";
import { timeLabel } from "./ui";
export function Library({
  inspected,
  open,
  snapshot,
  operation,
  onOperation,
  run,
  pending,
  revision,
}: {
  inspected: SessionDescriptor | undefined;
  open: (directory: string) => Promise<void>;
  snapshot: Snapshot | undefined;
  operation: Operation;
  onOperation: (operation: Operation) => void;
  run: RunAction;
  pending: string;
  revision: number;
}) {
  const [catalog, setCatalog] = useState<z.infer<typeof catalogSchema>>();
  const [catalogError, setCatalogError] = useState("");
  const [expanded, setExpanded] = useState(false);
  const [search, setSearch] = useState("");
  useEffect(() => {
    let disposed = false;
    void product
      .catalog()
      .then((result) => {
        if (!disposed) {
          setCatalog(result);
          setCatalogError("");
        }
      })
      .catch((cause: unknown) => {
        if (!disposed) setCatalogError(errorMessage(cause));
      });
    return () => {
      disposed = true;
    };
  }, [revision]);
  const canSave =
    captureSafe(snapshot) &&
    !operationBusy(operation) &&
    !!inspected &&
    !pending;
  return (
    <section className="library">
      <div className="library-heading">
        <div>
          <h2>{inspected ? inspected.id : "Session library"}</h2>
          <p className="muted">
            {inspected
              ? `${inspected.device?.name ?? "Saved device"} · ${timeLabel(inspected.started_at)}${inspected.legacy ? " · legacy session" : ""}`
              : "Saved sessions remain readable without capture tools."}
          </p>
        </div>
        <div className="button-row">
          <button
            onClick={() => setExpanded(!expanded)}
            aria-expanded={expanded}
          >
            Saved sessions
          </button>
          <button
            disabled={!!pending}
            onClick={() =>
              void run("Opening session folder", async () => {
                const directory = await product.chooseFolder();
                if (directory) await open(directory);
              })
            }
          >
            Open folder
          </button>
          {inspected && (
            <button
              disabled={!!pending}
              onClick={() =>
                void run("Revealing session", () => product.reveal("session"))
              }
            >
              Reveal
            </button>
          )}
        </div>
      </div>
      {expanded && (
        <div className="session-catalog">
          <div className="section-heading">
            <input
              type="search"
              aria-label="Find saved sessions"
              placeholder="Find a session or device"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            <button
              disabled={!!pending}
              onClick={() =>
                void run("Refreshing saved sessions", async () => {
                  setCatalog(await product.catalog());
                  setCatalogError("");
                })
              }
            >
              Refresh
            </button>
          </div>
          {catalogError && (
            <p className="notice" role="alert">
              {catalogError}
            </p>
          )}
          {catalog?.sessions
            .filter((session) =>
              `${session.id} ${session.device?.name ?? ""} ${session.directory}`
                .toLocaleLowerCase()
                .includes(search.toLocaleLowerCase()),
            )
            .map((session) => (
              <button
                className={`session-row ${session.directory === inspected?.directory ? "selected" : ""}`}
                key={session.directory}
                disabled={!!pending}
                onClick={() =>
                  void run("Opening saved session", () =>
                    open(session.directory),
                  )
                }
              >
                <strong>{session.id}</strong>
                <span>
                  {session.device?.name ?? "Saved device"} ·{" "}
                  {timeLabel(session.started_at)}
                </span>
                <small>{session.directory}</small>
              </button>
            ))}
          {catalog && !catalog.sessions.length && (
            <p className="hint">
              No saved sessions were found in the capture folder. You can open
              another session folder.
            </p>
          )}
          {catalog?.truncated && (
            <p className="notice">
              The catalog scan reached its limit after {catalog.scanned}{" "}
              folders. Open a session folder directly to inspect another
              capture.
            </p>
          )}
          {catalog?.warnings.map((warning) => (
            <p className="notice" key={warning}>
              {warning}
            </p>
          ))}
        </div>
      )}
      {inspected && (
        <>
          <div className="session-actions">
            <button
              disabled={!canSave}
              onClick={() =>
                void run("Saving archive", async () =>
                  onOperation(await api.save(inspected.directory)),
                )
              }
            >
              Save archive
            </button>
            <button
              disabled={!canSave}
              onClick={() =>
                void run("Copying archive", async () => {
                  await api.copyArchive();
                })
              }
            >
              Copy archive to…
            </button>
            <button
              disabled={!!pending}
              onClick={() =>
                void run("Revealing archive", () => product.reveal("archive"))
              }
            >
              Reveal archive
            </button>
            <span className="hint">
              Archives are published after validation and routing restoration.
            </span>
          </div>
          {inspected.warnings.map((warning) => (
            <p className="notice" key={warning}>
              {warning}
            </p>
          ))}
        </>
      )}
    </section>
  );
}
