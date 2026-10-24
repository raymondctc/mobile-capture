import { Button } from "./components/ui/button";
import { useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  api,
  captureSafe,
  errorMessage,
  operationBusy,
  preferencesEditable,
  type Device,
  type Operation,
  type Preferences,
  type Snapshot,
} from "./api";
import { product, type SessionDescriptor } from "./product";
import { useDraft, useSetupHistory, type RunAction } from "./state";
import { Capture } from "./Capture";
import { Timeline } from "./Timeline";
import { Library } from "./Library";
import { Setup } from "./Setup";
import { Diagnostics } from "./Diagnostics";
import { Settings } from "./Settings";
import { OperationStatus } from "./Operation";
import {
  consumeHandoff,
  handoffAvailable,
  HandoffNotices,
  resolveHandoff,
  type HandoffNotice,
} from "./Handoff";
import { Dialog } from "./ui";
import "./index.css";
import { Radio, Settings2 } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./components/ui/tooltip";

type MainView = "capture" | "settings";
type Panel = "setup" | "doctor" | null;
export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [operation, setOperation] = useState<Operation>({ state: "idle" });
  const [preferences, setPreferences] = useState<Preferences>();
  const [devices, setDevices] = useState<Device[]>([]);
  const [draft, setDraft] = useDraft();
  const [history, setHistory] = useSetupHistory();
  const [panel, setPanel] = useState<Panel>(() =>
    history.deferred ? null : "setup",
  );
  const [inspected, setInspected] = useState<SessionDescriptor>();
  const [view, setView] = useState<MainView>("capture");
  const [revision, setRevision] = useState(0);
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [snapshotError, setSnapshotError] = useState("");
  const [operationError, setOperationError] = useState("");
  const connection = snapshotError || operationError;
  const [handoffs, setHandoffs] = useState<HandoffNotice[]>([]);
  const [handoffError, setHandoffError] = useState("");
  const busy = useRef(false);
  const generation = useRef(0);
  const openGeneration = useRef(0);
  const currentId = useRef("");
  const completedOperation = useRef("");
  const discoveryRequest = useRef(0);
  const inspectedSession = useRef<SessionDescriptor | undefined>(undefined);
  const nextHandoff = useRef(0);
  const latestOperation = useRef(operation);
  const operationVerified = useRef(false);
  latestOperation.current = operation;
  const run: RunAction = useCallback(async (label, action) => {
    if (busy.current) return;
    busy.current = true;
    generation.current += 1;
    setPending(label);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      generation.current += 1;
      busy.current = false;
      setPending("");
    }
  }, []);
  const open = useCallback(async (directory: string) => {
    const request = ++openGeneration.current;
    const result = await product.open(directory);
    if (request === openGeneration.current) {
      inspectedSession.current = result;
      setInspected(result);
    }
  }, []);
  function closeSession() {
    generation.current += 1;
    openGeneration.current += 1;
    inspectedSession.current = undefined;
    setInspected(undefined);
  }
  function changeDraft(next: typeof draft) {
    generation.current += 1;
    setDraft(next);
  }
  const refresh = useCallback(async () => {
    setDevices(await api.discover());
  }, []);
  async function savePreferences(next: Preferences) {
    setPreferences(await api.savePreferences(next));
    setRevision((value) => value + 1);
  }
  const applyHandoff = useCallback(
    async (notice: HandoffNotice) => {
      if (busy.current || inspectedSession.current) return;
      await run("Applying Android launch link", async () => {
        const request = generation.current;
        const update = (
          message: string,
          state: HandoffNotice["state"] = "received",
        ) =>
          setHandoffs((previous) =>
            previous.map((entry) =>
              entry.id === notice.id ? { ...entry, state, message } : entry,
            ),
          );
        const conflict =
          "Finish the current action and close any inspected session, then apply this link while capture control is idle. No recording has started.";
        try {
          const [capture, work] = await Promise.all([
            api.snapshot(),
            api.operation(),
          ]);
          setSnapshot(capture);
          setOperation(work);
          if (
            !handoffAvailable({
              snapshot: capture,
              operation: work,
              inspected: inspectedSession.current,
              pending: false,
            })
          ) {
            update(conflict);
            return;
          }
          const target = await resolveHandoff(notice.request, {
            discover: api.discover,
            applications: product.applications,
            catalog: product.catalog,
          });
          if (target.kind === "blocked") {
            update(target.message);
            return;
          }
          const [verified, latestWork] = await Promise.all([
            api.snapshot(),
            api.operation(),
          ]);
          setSnapshot(verified);
          setOperation(latestWork);
          if (
            request !== generation.current ||
            !handoffAvailable({
              snapshot: verified,
              operation: latestWork,
              inspected: inspectedSession.current,
              pending: false,
            })
          ) {
            update(conflict);
            return;
          }
          if (target.kind === "session") {
            await open(target.session.directory);
            update(
              "Opened the requested saved session. No recording has started.",
              "applied",
            );
          } else {
            setDevices((previous) => [
              target.device,
              ...previous.filter(
                (device) => device.serial !== target.device.serial,
              ),
            ]);
            setDraft((previous) => ({
              ...previous,
              serial: target.device.serial,
              application_id: target.applicationId ?? "",
              log_mode: "app",
            }));
            update(
              "Selected the requested device and app scope. Review sources and consent before starting a recording.",
              "applied",
            );
          }
        } catch (cause) {
          update(
            `The launch link could not be applied. ${errorMessage(cause)}`,
          );
        }
      });
    },
    [open, run, setDraft],
  );
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const request = generation.current;
      let capture: Snapshot | undefined;
      if (!busy.current) {
        try {
          capture = await api.snapshot();
          if (
            !disposed &&
            request === generation.current &&
            operationVerified.current
          ) {
            setSnapshot(capture);
            setSnapshotError("");
          }
        } catch (cause) {
          if (!disposed && request === generation.current)
            setSnapshotError(errorMessage(cause));
        }
      }
      if (!disposed && !busy.current && request === generation.current) {
        try {
          await consumeHandoff({
            read: api.pendingHandoff,
            retain: (handoff) => {
              const notice: HandoffNotice = {
                id: ++nextHandoff.current,
                request: handoff,
                state: "received",
                message:
                  handoff.kind === "rejected"
                    ? handoff.message
                    : "Launch link received. Apply it while capture control is idle and no session is open. No recording has started.",
              };
              if (!disposed) setHandoffs((previous) => [...previous, notice]);
              return notice;
            },
            canApply: () =>
              !disposed &&
              request === generation.current &&
              operationVerified.current &&
              handoffAvailable({
                snapshot: capture,
                operation: latestOperation.current,
                inspected: inspectedSession.current,
                pending: busy.current,
              }),
            apply: applyHandoff,
          });
          if (!disposed) setHandoffError("");
        } catch (cause) {
          if (!disposed) setHandoffError(errorMessage(cause));
        }
      }
      if (!disposed) timer = setTimeout(poll, 900);
    }
    void poll();
    void run("Loading devices and preferences", async () => {
      const results = await Promise.allSettled([
        api.discover(),
        api.preferences(),
      ]);
      const [deviceResult, preferenceResult] = results;
      if (disposed) return;
      if (deviceResult.status === "fulfilled") setDevices(deviceResult.value);
      if (preferenceResult.status === "fulfilled")
        setPreferences(preferenceResult.value);
      const failures = results.filter((result) => result.status === "rejected");
      if (failures.length)
        throw new Error(
          failures
            .map((result) =>
              result.status === "rejected" ? errorMessage(result.reason) : "",
            )
            .join("\n"),
        );
    });
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [run, applyHandoff]);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function pollOperation() {
      const request = generation.current;
      try {
        const work = await api.operation();
        if (!disposed && request === generation.current) {
          operationVerified.current = true;
          latestOperation.current = work;
          setOperation(work);
          setOperationError("");
        }
      } catch (cause) {
        if (!disposed && request === generation.current) {
          operationVerified.current = false;
          setOperationError(errorMessage(cause));
        }
      }
      if (!disposed) timer = setTimeout(pollOperation, 500);
    }
    void pollOperation();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function pollDevices() {
      const request = ++discoveryRequest.current;
      const actionGeneration = generation.current;
      if (!busy.current) {
        try {
          const result = await api.discover();
          if (
            !disposed &&
            request === discoveryRequest.current &&
            actionGeneration === generation.current
          )
            setDevices(result);
        } catch {
          if (!disposed && actionGeneration === generation.current)
            setDevices([]);
        }
      }
      if (!disposed) timer = setTimeout(pollDevices, 4000);
    }
    timer = setTimeout(pollDevices, 4000);
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);
  useEffect(() => {
    if (
      !snapshot ||
      snapshot.phase === "idle" ||
      snapshot.session.id === currentId.current
    )
      return;
    currentId.current = snapshot.session.id;
    void run("Opening current capture", () => open(snapshot.session.directory));
  }, [snapshot, open, run]);
  useEffect(() => {
    if (
      operation.state !== "finished" ||
      operation.id === completedOperation.current
    )
      return;
    completedOperation.current = operation.id;
    const result = operation.result;
    if (result.kind === "certificate")
      setHistory((previous) => ({ ...previous, certificate: result }));
    if (result.kind === "connection_check")
      setHistory((previous) => ({ ...previous, check: result }));
    if (result.kind === "save") setRevision((value) => value + 1);
  }, [operation, setHistory]);
  const setupLocked =
    operation.state === "preparing_certificate" ||
    operation.state === "checking_connection" ||
    operation.state === "restoring" ||
    (operation.state === "failed" &&
      operation.recovery_required &&
      operation.kind !== "save");
  const idle = captureSafe(snapshot) && !operationBusy(operation);
  async function recoverSave() {
    setSnapshot(await api.recover());
    setOperation(await api.operation());
  }
  const launchNotices = (
    <>
      {handoffError && (
        <p
          className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
          role="alert"
        >
          Android launch links could not be checked. {handoffError}
        </p>
      )}
      <HandoffNotices
        notices={handoffs}
        available={
          !connection &&
          handoffAvailable({
            snapshot,
            operation,
            inspected,
            pending: !!pending,
          })
        }
        apply={applyHandoff}
        dismiss={(id) =>
          setHandoffs((previous) =>
            previous.filter((notice) => notice.id !== id),
          )
        }
      />
    </>
  );
  return (
    <TooltipProvider>
      <div className="flex h-dvh min-h-0 min-w-0 overflow-hidden bg-background text-foreground">
        <nav
          aria-label="Workspace"
          className="flex w-12 shrink-0 flex-col items-center gap-2 border-r border-border py-3"
        >
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant={view === "capture" ? "secondary" : "ghost"}
                aria-label="Capture"
                aria-pressed={view === "capture"}
                onClick={() => setView("capture")}
              >
                <Radio aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">Capture</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                className="mt-auto"
                size="icon"
                variant={view === "settings" ? "secondary" : "ghost"}
                aria-label="Settings"
                aria-pressed={view === "settings"}
                onClick={() => setView("settings")}
              >
                <Settings2 aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">Settings</TooltipContent>
          </Tooltip>
        </nav>
        <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-2">
          <Capture
            snapshot={snapshot}
            operation={operation}
            devices={devices}
            draft={draft}
            change={changeDraft}
            preferences={preferences}
            savePreferences={savePreferences}
            refresh={refresh}
            run={run}
            pending={pending}
            onSnapshot={setSnapshot}
            onOperation={setOperation}
          />
          {snapshot?.session.fixture && (
            <div
              className="mx-4 flex flex-wrap gap-2 rounded-md border border-border bg-card px-3 py-2 text-xs text-muted-foreground"
              role="status"
            >
              <strong>Fixture mode</strong>
              <span>
                This launcher uses a simulated Android device. It does not prove
                physical phone capture or trust.
              </span>
            </div>
          )}
          {(error || connection) && (
            <div
              className="mx-4 flex shrink-0 flex-wrap items-start justify-between gap-3 rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive"
              role="alert"
            >
              <div>
                <strong>
                  {connection
                    ? "Native capture service is unavailable"
                    : "Action could not complete"}
                </strong>
                <p>{error || connection}</p>
              </div>
              <Button
                disabled={!!pending || operationBusy(operation)}
                onClick={() =>
                  void run("Reconnecting", async () => {
                    const [capture, work, prefs] = await Promise.all([
                      api.reconnect(),
                      api.operation(),
                      api.preferences(),
                    ]);
                    setSnapshot(capture);
                    setOperation(work);
                    setPreferences(prefs);
                    await refresh();
                    setSnapshotError("");
                    setOperationError("");
                  })
                }
              >
                Retry connection
              </Button>
            </div>
          )}
          <OperationStatus
            operation={operation}
            change={setOperation}
            run={run}
            pending={pending}
            recover={recoverSave}
          />
          {!panel && launchNotices}
          <main className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-3">
            <section
              aria-label="Capture evidence"
              hidden={view !== "capture"}
              inert={view !== "capture"}
              style={{ display: view === "capture" ? undefined : "none" }}
              className="flex h-full min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card"
            >
              <Library
                inspected={inspected}
                open={open}
                close={closeSession}
                snapshot={snapshot}
                operation={operation}
                onOperation={setOperation}
                run={run}
                pending={pending}
                revision={revision}
                visible={view === "capture"}
              />
              <Timeline inspected={inspected} />
            </section>
            <section
              aria-label="Settings view"
              hidden={view !== "settings"}
              inert={view !== "settings"}
              style={{ display: view === "settings" ? undefined : "none" }}
              className="mx-auto max-w-4xl space-y-5 py-4"
            >
              <h1 className="text-xl font-semibold tracking-tight">Settings</h1>
              <Settings
                preferences={preferences}
                save={savePreferences}
                idle={idle}
                editable={preferencesEditable(snapshot, operation)}
                run={run}
                pending={pending}
                setup={() => setPanel("setup")}
                doctor={() => setPanel("doctor")}
              />
            </section>
          </main>
          {panel && (
            <Dialog
              title={panel === "setup" ? "Device setup" : "Host Doctor"}
              close={() => setPanel(null)}
              locked={setupLocked}
            >
              {launchNotices}
              {(error || connection) && (
                <p
                  className="rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive select-text"
                  role="alert"
                >
                  {error || connection}
                </p>
              )}
              {setupLocked && (
                <OperationStatus
                  operation={operation}
                  change={setOperation}
                  run={run}
                  pending={pending}
                  recover={recoverSave}
                />
              )}
              {panel === "setup" && (
                <Setup
                  serial={draft.serial}
                  selectDevice={(serial) =>
                    changeDraft({ ...draft, serial, application_id: "" })
                  }
                  devices={devices}
                  refresh={refresh}
                  snapshot={snapshot}
                  operation={operation}
                  onOperation={setOperation}
                  history={history}
                  onHistory={setHistory}
                  run={run}
                  pending={pending}
                  close={() => setPanel(null)}
                  doctor={() => setPanel("doctor")}
                />
              )}
              {panel === "doctor" && (
                <Diagnostics
                  run={run}
                  pending={pending}
                  settings={() => {
                    setView("settings");
                    setPanel(null);
                  }}
                />
              )}
            </Dialog>
          )}
        </div>
      </div>
    </TooltipProvider>
  );
}
const root = document.getElementById("root");
if (!root)
  throw new Error("Mobile Capture cannot find its application container.");
createRoot(root).render(<App />);
