import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, type Update, type UpdateAction } from "./api";
import { Button } from "./components/ui/button";

export function useUpdates() {
  const [status, setStatus] = useState<Update>({ state: "idle" });
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);
  const acting = useRef<UpdateAction | null>(null);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const generation = request.current;
      try {
        const next = await api.updateStatus();
        const phase = acting.current?.kind;
        const currentAction =
          !phase ||
          (phase === "download" && next.state === "downloading") ||
          (phase === "install" && next.state === "installing") ||
          (phase === "check" && next.state === "checking");
        if (!disposed && currentAction && generation === request.current) {
          setStatus(next);
          setError("");
        }
      } catch (cause) {
        if (!disposed && generation === request.current)
          setError(errorMessage(cause));
      }
      if (!disposed) timer = setTimeout(() => void poll(), 700);
    }
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);
  const act = useCallback(async (action: UpdateAction) => {
    if (acting.current) return;
    acting.current = action;
    request.current += 1;
    setPending(true);
    setError("");
    setStatus((previous) => {
      if (action.kind === "check") return { state: "checking" };
      if (
        !("release" in previous) ||
        previous.release.version !== action.version
      )
        return previous;
      return action.kind === "download"
        ? {
            state: "downloading",
            release: previous.release,
            received: 0,
            total: null,
          }
        : { state: "installing", release: previous.release };
    });
    try {
      setStatus(await api.updateAction(action));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      request.current += 1;
      acting.current = null;
      setPending(false);
    }
  }, []);
  return { status, pending, error, act };
}
export type UpdateController = ReturnType<typeof useUpdates>;

export function UpdateNotice({
  controller,
  openSettings,
}: {
  controller: UpdateController;
  openSettings: () => void;
}) {
  const [dismissed, setDismissed] = useState("");
  const { status } = controller;
  if (
    !("release" in status) ||
    (dismissed === status.release.version &&
      status.state !== "repair_required" &&
      status.state !== "installing")
  )
    return null;
  return (
    <aside
      className="flex items-center gap-3 border-b border-border px-4 py-2 text-xs"
      aria-label="App update"
    >
      <span className="flex-1">
        {status.state === "repair_required"
          ? "The app update needs repair. View update details before capturing."
          : status.state === "installing"
            ? "Installing update. Restarting shortly…"
            : status.state === "ready"
              ? `Version ${status.release.version} is ready to install.`
              : `Version ${status.release.version} is available.`}
      </span>
      <Button size="sm" onClick={openSettings}>
        View update
      </Button>
      {status.state !== "installing" && status.state !== "repair_required" && (
        <Button
          size="sm"
          variant="ghost"
          aria-label="Dismiss update notice"
          onClick={() => setDismissed(status.release.version)}
        >
          Dismiss
        </Button>
      )}
    </aside>
  );
}
export function UpdateSettings({
  controller,
}: {
  controller: UpdateController;
}) {
  const { status, pending, error, act } = controller;
  const disabled =
    pending ||
    status.state === "checking" ||
    status.state === "downloading" ||
    status.state === "installing";
  let content;
  switch (status.state) {
    case "unconfigured":
      content = (
        <p>In-app updates are unavailable in this development build.</p>
      );
      break;
    case "idle":
      content = <p>Check for a newer Android desktop release.</p>;
      break;
    case "checking":
      content = <p role="status">Checking for updates…</p>;
      break;
    case "current":
      content = <p role="status">You’re up to date.</p>;
      break;
    case "error":
      content = (
        <p role="alert">Could not check for updates. {status.message}</p>
      );
      break;
    case "available":
    case "ready":
      content = (
        <>
          <p>
            Version {status.release.version}{" "}
            {status.state === "ready"
              ? "is downloaded and verified."
              : "is available."}
          </p>
          {status.release.notes && (
            <pre className="whitespace-pre-wrap font-sans text-sm select-text">
              {status.release.notes}
            </pre>
          )}
          {status.error && <p role="alert">{status.error}</p>}
          {status.state === "ready" && (
            <p>
              Finish capture and save its archive before installing.
              Installation restarts the app.
            </p>
          )}
          <Button
            disabled={disabled}
            onClick={() =>
              void act({
                kind: status.state === "ready" ? "install" : "download",
                version: status.release.version,
              })
            }
          >
            {status.state === "ready"
              ? "Install and restart"
              : status.error
                ? "Retry download"
                : "Download update"}
          </Button>
        </>
      );
      break;
    case "downloading":
      content = (
        <div role="status">
          <p>Downloading version {status.release.version}…</p>
          <progress
            aria-label="Update download"
            value={status.total ? status.received : undefined}
            max={status.total ?? undefined}
          />
          <p>
            {Math.round(status.received / 1024)} KB
            {status.total
              ? ` of ${Math.round(status.total / 1024)} KB`
              : " received"}
          </p>
        </div>
      );
      break;
    case "repair_required":
      content = (
        <p role="alert">
          {status.message} Capture stays unavailable until the app is repaired.
          Use Quit to close the app.
        </p>
      );
      break;
    case "installing":
      content = (
        <p role="status">
          Installing version {status.release.version}. The app will restart.
        </p>
      );
      break;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
  return (
    <div className="space-y-3" aria-label="Software updates">
      {content}
      {error && <p role="alert">{error}</p>}
      {(status.state === "idle" ||
        status.state === "current" ||
        status.state === "checking" ||
        status.state === "error") && (
        <Button disabled={disabled} onClick={() => void act({ kind: "check" })}>
          {status.state === "error"
            ? "Retry update check"
            : "Check for updates"}
        </Button>
      )}
    </div>
  );
}
