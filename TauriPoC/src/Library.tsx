import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
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
  close,
  snapshot,
  operation,
  onOperation,
  run,
  pending,
  revision,
}: {
  inspected: SessionDescriptor | undefined;
  open: (directory: string) => Promise<void>;
  close: () => void;
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
    <section className="shrink-0 border-b border-border px-4 py-2">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-2 [&_h2]:max-w-full [&_h2]:break-all [&_h2]:font-mono [&_h2]:text-sm [&_h2]:font-medium">
        <div className="min-w-0 flex-1">
          <h2>{inspected ? inspected.id : "Session library"}</h2>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {inspected
              ? `${inspected.device?.name ?? "Saved device"} · ${timeLabel(inspected.started_at)}${inspected.legacy ? " · legacy session" : ""}`
              : "Saved sessions remain readable without capture tools."}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            onClick={() => setExpanded(!expanded)}
            aria-expanded={expanded}
          >
            Saved sessions
          </Button>
          <Button
            disabled={!!pending}
            onClick={() =>
              void run("Opening session folder", async () => {
                const directory = await product.chooseFolder();
                if (directory) await open(directory);
              })
            }
          >
            Open folder
          </Button>
          {inspected && (
            <details className="relative">
              <summary className="cursor-pointer rounded-md border border-input px-2.5 py-1.5 text-sm">
                Session actions
              </summary>
              <div className="absolute top-full right-0 z-20 mt-1 grid min-w-44 gap-2 rounded-lg border border-border bg-white p-3 shadow-sm">
                <Button
                  disabled={!!pending}
                  onClick={() =>
                    void run("Revealing session", () =>
                      product.reveal("session"),
                    )
                  }
                >
                  Reveal
                </Button>
                <Button disabled={!!pending} onClick={close}>
                  Close session
                </Button>
                <Button
                  disabled={!canSave}
                  onClick={() =>
                    void run("Saving archive", async () =>
                      onOperation(await api.save(inspected.directory)),
                    )
                  }
                >
                  Save archive
                </Button>
                <Button
                  disabled={!canSave}
                  onClick={() =>
                    void run("Copying archive", async () => {
                      await api.copyArchive();
                    })
                  }
                >
                  Copy archive to…
                </Button>
                <Button
                  disabled={!!pending}
                  onClick={() =>
                    void run("Revealing archive", () =>
                      product.reveal("archive"),
                    )
                  }
                >
                  Reveal archive
                </Button>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  Archives are published after validation and routing
                  restoration.
                </p>
              </div>
            </details>
          )}
        </div>
      </div>
      {expanded && (
        <div
          className="mt-3 max-h-56 space-y-2 overflow-y-auto overscroll-contain border-t border-border pt-3"
          role="region"
          aria-label="Saved session catalog"
          tabIndex={0}
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <Input
              type="search"
              aria-label="Find saved sessions"
              placeholder="Find a session or device"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            <Button
              disabled={!!pending}
              onClick={() =>
                void run("Refreshing saved sessions", async () => {
                  setCatalog(await product.catalog());
                  setCatalogError("");
                })
              }
            >
              Refresh
            </Button>
          </div>
          {catalogError && (
            <p
              className="rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs leading-relaxed text-[#665026] select-text"
              role="alert"
            >
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
              <Button
                variant="ghost"
                className={`flex h-auto w-full flex-col items-start gap-1 rounded-md border border-border p-3 text-left whitespace-normal select-text [&_small]:break-all [&_small]:text-xs [&_span]:text-xs [&_span]:text-muted-foreground ${session.directory === inspected?.directory ? "bg-[#edf5f1]" : ""}`}
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
              </Button>
            ))}
          {catalog && !catalog.sessions.length && (
            <p className="text-xs leading-relaxed text-muted-foreground">
              No saved sessions were found in the capture folder. You can open
              another session folder.
            </p>
          )}
          {catalog?.truncated && (
            <p className="rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs leading-relaxed text-[#665026] select-text">
              The catalog scan reached its limit after {catalog.scanned}{" "}
              folders. Open a session folder directly to inspect another
              capture.
            </p>
          )}
          {catalog?.warnings.map((warning) => (
            <p
              className="rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs leading-relaxed text-[#665026] select-text"
              key={warning}
            >
              {warning}
            </p>
          ))}
        </div>
      )}
      {inspected && (
        <>
          {inspected.warnings.map((warning) => (
            <p
              className="rounded-md border border-[#e4dac0] bg-[#fbf8ef] px-3 py-2 text-xs leading-relaxed text-[#665026] select-text"
              key={warning}
            >
              {warning}
            </p>
          ))}
        </>
      )}
    </section>
  );
}
