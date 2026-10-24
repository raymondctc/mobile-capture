import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "./components/ui/collapsible";
import { Badge } from "./components/ui/badge";
import { Checkbox } from "./components/ui/checkbox";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Textarea } from "./components/ui/textarea";
import { Label } from "./components/ui/label";
import { NativeSelect } from "./components/ui/native-select";
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from "./components/ui/combobox";
import { RefreshCw, Settings2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  api,
  captureSafe,
  operationBusy,
  type Device,
  type Operation,
  type Preferences,
  type Snapshot,
} from "./api";
import {
  product,
  contextSchema,
  formatBytes,
  type Application,
  type StartRequest,
} from "./product";
import type { CaptureDraft, RunAction } from "./state";
import { Policies } from "./Policies";

export function Capture({
  snapshot,
  operation,
  devices,
  draft,
  change,
  preferences,
  savePreferences,
  refresh,
  run,
  pending,
  onSnapshot,
  onOperation,
  setup,
  actionHost,
}: {
  snapshot: Snapshot | undefined;
  operation: Operation;
  devices: Device[];
  draft: CaptureDraft;
  change: (draft: CaptureDraft) => void;
  preferences: Preferences | undefined;
  savePreferences: (preferences: Preferences) => Promise<void>;
  refresh: () => Promise<void>;
  run: RunAction;
  pending: string;
  onSnapshot: (snapshot: Snapshot) => void;
  onOperation: (operation: Operation) => void;
  setup: () => void;
  actionHost: HTMLDivElement | null;
}) {
  const [apps, setApps] = useState<Application[]>([]);
  const [appSearch, setAppSearch] = useState("");
  const [appState, setAppState] = useState("");
  const appGeneration = useRef(0);
  const [logConsent, setLogConsent] = useState(false);
  const [marker, setMarker] = useState("");
  const [editedPolicy, setEditedPolicy] = useState<Preferences>();
  const active = snapshot?.phase === "recording";
  const session =
    snapshot && snapshot.phase !== "idle" ? snapshot.session : undefined;
  const blocked =
    !!pending || operationBusy(operation) || !captureSafe(snapshot);
  const hasLogs = draft.selection !== "network";
  const device = devices.find((entry) => entry.serial === draft.serial);
  const selectedApp = apps.find(
    (app) => app.application_id === draft.application_id,
  );
  const appOptions = [...new Set(apps.map((app) => app.application_id))];
  for (const id of [draft.application_id, appSearch.trim()]) {
    if (
      /^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$/.test(id) &&
      !appOptions.includes(id)
    )
      appOptions.push(id);
  }
  const appValid =
    /^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$/.test(draft.application_id) &&
    !selectedApp?.shared_uid;
  const contextValidation = contextSchema.safeParse(draft.context);
  const canStart =
    contextValidation.success &&
    !blocked &&
    device?.state === "ready" &&
    !!preferences &&
    (!hasLogs || (draft.log_mode === "app" ? appValid : logConsent));
  useEffect(() => {
    appGeneration.current += 1;
    setApps([]);
    setAppState("");
    setAppSearch("");
    setLogConsent(false);
  }, [draft.serial]);
  useEffect(() => {
    if (!active) {
      setLogConsent(false);
    }
  }, [active]);
  async function refreshApps() {
    const request = ++appGeneration.current;
    const found = await product.applications(draft.serial);
    if (request === appGeneration.current) {
      setApps(found);
      setAppState(`${found.length} installed apps. Refreshed for this device.`);
    }
  }
  async function start() {
    if (!preferences || !canStart) return;
    const selection: StartRequest["selection"] =
      draft.selection === "logs"
        ? { mode: "logs" }
        : { mode: draft.selection, consent: { whole_device: true } };
    const request: StartRequest = {
      serial: draft.serial,
      whole_device_consent: logConsent,
      selection,
      log_scope:
        draft.log_mode === "app"
          ? { mode: "app", application_id: draft.application_id }
          : { mode: "whole_device" },
      limits: preferences.limits,
      network_policy: preferences.network_policy,
      context: draft.context,
    };
    onSnapshot(await api.start(request));
    setLogConsent(false);
  }
  const needsScope =
    !active &&
    (!device ||
      device.state !== "ready" ||
      !preferences ||
      !contextValidation.success ||
      (hasLogs && (draft.log_mode === "app" ? !appValid : !logConsent)));
  const [editingScope, setEditingScope] = useState(!draft.serial);
  const scopeSession = active ? session : undefined;
  const scopeMode = scopeSession?.selection.mode ?? draft.selection;
  const scopeLog = scopeSession?.log_scope.mode ?? draft.log_mode;
  const scopeApp =
    scopeSession?.log_scope.mode === "app"
      ? scopeSession.log_scope.application_id
      : draft.application_id;
  return (
    <section
      className="shrink-0 rounded-t-lg border border-border bg-card"
      aria-label="Capture scope"
    >
      <Collapsible open={editingScope} onOpenChange={setEditingScope}>
        <div className="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-2 px-4 py-2">
          <div className="min-w-0 text-sm">
            <span className="mr-2 text-xs text-muted-foreground">Device</span>
            <span className="font-medium">
              {scopeSession?.device.name ?? device?.name ?? "Choose device"}
            </span>
          </div>
          <div className="text-sm">
            <span className="mr-2 text-xs text-muted-foreground">Sources</span>
            {scopeMode === "network"
              ? "Network"
              : scopeMode === "logs"
                ? "Logs"
                : "Logs + network"}
          </div>
          {scopeMode !== "network" && (
            <div className="min-w-0 break-all text-sm">
              <span className="mr-2 text-xs text-muted-foreground">Logs</span>
              {scopeLog === "whole_device"
                ? "Whole device"
                : scopeApp || "Choose app"}
            </div>
          )}
          <Badge variant="secondary" className="text-xs">
            {active
              ? "Recording scope"
              : canStart
                ? "Ready"
                : needsScope
                  ? "Complete capture scope"
                  : "Capture unavailable"}
          </Badge>
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="sm" className="ml-auto">
              {editingScope ? "Hide capture scope" : "Edit capture scope"}
            </Button>
          </CollapsibleTrigger>
        </div>
        <CollapsibleContent className="max-h-[42vh] overflow-y-auto overscroll-contain border-t border-border px-4 py-3">
          <div className="grid gap-5 md:grid-cols-2 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)_minmax(0,1fr)]">
            <section className="space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h2>Android device</h2>
                <Button
                  size="icon"
                  aria-label="Refresh devices"
                  title="Refresh devices"
                  disabled={!!pending || active || operationBusy(operation)}
                  onClick={() => void run("Refreshing devices", refresh)}
                >
                  <RefreshCw aria-hidden="true" />
                </Button>
              </div>
              <Label htmlFor="capture-device">Capture on</Label>
              <NativeSelect
                id="capture-device"
                value={draft.serial}
                disabled={active || operationBusy(operation) || !!pending}
                onChange={(event) =>
                  change({
                    ...draft,
                    serial: event.target.value,
                    application_id: "",
                  })
                }
              >
                <option value="">Select a device</option>
                {draft.serial && !device && (
                  <option value={draft.serial}>
                    {draft.serial} · unavailable
                  </option>
                )}
                {devices.map((item) => (
                  <option key={item.serial} value={item.serial}>
                    {item.name} · {item.state}
                  </option>
                ))}
              </NativeSelect>
              <p className="font-mono select-text break-all text-xs text-muted-foreground">
                {draft.serial || "Connect USB and enable USB debugging."}
              </p>
              {draft.serial && device?.state !== "ready" && (
                <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
                  The selected device is {device?.state ?? "unavailable"}.
                  Unlock it, approve USB debugging, reconnect, and refresh.
                  Choose a different device explicitly if needed.
                </p>
              )}
              <Button onClick={setup}>
                <Settings2 aria-hidden="true" />
                Open device setup
              </Button>
            </section>
            <section className="space-y-3">
              <h2>Sources and scope</h2>
              <fieldset
                className="space-y-3 rounded-lg border border-border p-3"
                disabled={blocked}
              >
                <legend className="sr-only">Capture sources</legend>
                <div className="grid grid-cols-1 gap-3 min-[400px]:grid-cols-2">
                  <Label className="min-w-0">
                    Sources
                    <NativeSelect
                      value={draft.selection}
                      onChange={(event) => {
                        const value = event.target.value;
                        if (
                          value === "logs" ||
                          value === "network" ||
                          value === "logs_and_network"
                        ) {
                          change({ ...draft, selection: value });
                          setLogConsent(false);
                        }
                      }}
                    >
                      <option value="network">Network only</option>
                      <option value="logs">Logs only</option>
                      <option value="logs_and_network">Logs and network</option>
                    </NativeSelect>
                  </Label>
                  {hasLogs && (
                    <Label className="min-w-0">
                      Log scope
                      <NativeSelect
                        value={draft.log_mode}
                        onChange={(event) => {
                          const value = event.target.value;
                          if (value === "app" || value === "whole_device") {
                            change({ ...draft, log_mode: value });
                            setLogConsent(false);
                          }
                        }}
                      >
                        <option value="app">Selected app</option>
                        <option value="whole_device">Whole device</option>
                      </NativeSelect>
                    </Label>
                  )}
                </div>
                {hasLogs && (
                  <>
                    {draft.log_mode === "app" ? (
                      <>
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <Label htmlFor="app-search">Installed apps</Label>
                          <Button
                            size="sm"
                            disabled={device?.state !== "ready"}
                            onClick={() =>
                              void run("Refreshing installed apps", refreshApps)
                            }
                          >
                            <RefreshCw aria-hidden="true" />
                            Refresh apps
                          </Button>
                        </div>
                        <Combobox
                          items={appOptions}
                          value={draft.application_id || null}
                          disabled={blocked}
                          onInputValueChange={setAppSearch}
                          onValueChange={(id) =>
                            change({ ...draft, application_id: id ?? "" })
                          }
                          itemToStringLabel={(id) => {
                            const app = apps.find(
                              (entry) => entry.application_id === id,
                            );
                            return app ? `${app.label} · ${id}` : id;
                          }}
                          filter={(id, query) => {
                            const app = apps.find(
                              (entry) => entry.application_id === id,
                            );
                            return `${app?.label ?? ""} ${id}`
                              .toLocaleLowerCase()
                              .includes(query.toLocaleLowerCase());
                          }}
                        >
                          <ComboboxInput
                            id="app-search"
                            aria-describedby="app-status"
                            placeholder="Search by name or app ID"
                            disabled={blocked}
                            showClear
                          />
                          <ComboboxContent>
                            <ComboboxEmpty>
                              No matching apps. Enter an application ID to use
                              it.
                            </ComboboxEmpty>
                            <ComboboxList>
                              {(id: string) => {
                                const app = apps.find(
                                  (entry) => entry.application_id === id,
                                );
                                return (
                                  <ComboboxItem
                                    key={id}
                                    value={id}
                                    disabled={app?.shared_uid}
                                    className="min-w-0 flex-col items-start gap-0.5 py-2"
                                  >
                                    <span className="break-words">
                                      {app?.label ?? "Use application ID"}
                                    </span>
                                    <span className="max-w-full break-all font-mono text-xs text-muted-foreground">
                                      {id}
                                    </span>
                                    {app?.shared_uid && (
                                      <span className="text-xs text-muted-foreground">
                                        Shared UID · unavailable
                                      </span>
                                    )}
                                  </ComboboxItem>
                                );
                              }}
                            </ComboboxList>
                          </ComboboxContent>
                        </Combobox>
                        <p
                          className="text-xs leading-relaxed text-muted-foreground"
                          role="status"
                          id="app-status"
                        >
                          {appState ||
                            "Refresh to read installed apps. Shared UID apps cannot be isolated."}
                        </p>
                        {selectedApp?.shared_uid && (
                          <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
                            This app shares its UID. Select another app or
                            explicitly choose whole-device logs.
                          </p>
                        )}
                        <p className="text-xs leading-relaxed text-muted-foreground">
                          The app identity is checked before collecting logs. A
                          missing or changed identity stops app log collection.
                        </p>
                      </>
                    ) : (
                      <Label className="flex items-start gap-2 rounded-md border border-border bg-muted/35 p-3 text-sm leading-relaxed">
                        <Checkbox
                          checked={
                            active
                              ? session?.log_scope.mode === "whole_device"
                              : logConsent
                          }
                          onCheckedChange={(checked) =>
                            setLogConsent(checked === true)
                          }
                        />
                        <span>
                          I consent to logs from every app and system process
                          for this recording.
                        </span>
                      </Label>
                    )}
                  </>
                )}
              </fieldset>
            </section>
            <section className="space-y-3">
              <details className="space-y-3 [&>summary]:cursor-pointer [&>summary]:text-sm [&>summary]:font-medium">
                <summary>Task context (optional)</summary>
                <fieldset
                  className="space-y-3 rounded-lg border border-border p-3"
                  disabled={blocked}
                >
                  <Label>
                    Title
                    <Input
                      maxLength={512}
                      value={draft.context.title}
                      onChange={(event) =>
                        change({
                          ...draft,
                          context: {
                            ...draft.context,
                            title: event.target.value,
                          },
                        })
                      }
                      placeholder="What are you investigating?"
                    />
                  </Label>
                  <Label>
                    Notes
                    <Textarea
                      maxLength={32768}
                      value={draft.context.notes}
                      onChange={(event) =>
                        change({
                          ...draft,
                          context: {
                            ...draft.context,
                            notes: event.target.value,
                          },
                        })
                      }
                      rows={3}
                    />
                  </Label>
                  <Label>
                    Reproduction steps, one per line
                    <Textarea
                      maxLength={409600}
                      value={draft.context.steps.join("\n")}
                      onChange={(event) =>
                        change({
                          ...draft,
                          context: {
                            ...draft.context,
                            steps: event.target.value.split("\n"),
                          },
                        })
                      }
                      rows={3}
                    />
                  </Label>
                </fieldset>
              </details>
              {!contextValidation.success && (
                <p
                  className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
                  role="alert"
                >
                  Task context is too long. Title allows 512 UTF-8 bytes, notes
                  32,768 bytes, and up to 100 steps of 4,096 bytes each.
                </p>
              )}
              <details className="space-y-3 [&>summary]:cursor-pointer [&>summary]:text-sm [&>summary]:font-medium">
                <summary>Capture limits</summary>
                {preferences && (
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      void run("Saving capture limits", async () => {
                        await savePreferences({
                          ...preferences,
                          limits: editedPolicy?.limits ?? preferences.limits,
                          network_policy:
                            editedPolicy?.network_policy ??
                            preferences.network_policy,
                        });
                        setEditedPolicy(undefined);
                      });
                    }}
                  >
                    <Policies
                      value={editedPolicy ?? preferences}
                      change={setEditedPolicy}
                      disabled={blocked}
                    />
                    <Button type="submit" disabled={blocked || !editedPolicy}>
                      Save limits
                    </Button>
                  </form>
                )}
              </details>
            </section>
          </div>
        </CollapsibleContent>
      </Collapsible>
      <section className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 text-xs [&:has(>*)]:border-t [&:has(>*)]:border-border [&:has(>*)]:py-2">
        {session && (
          <>
            <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
              <span>
                {session.selection.mode === "network"
                  ? "Network only"
                  : `${formatBytes(session.bytes)} raw log data`}
              </span>
              {session.receipt && session.selection.mode !== "network" && (
                <span>
                  {session.receipt.records.toLocaleString()} raw records
                </span>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              {session.receipt
                ? `Logs ended · ${session.receipt.reason.replaceAll("_", " ")}`
                : active
                  ? "Recording continues when this window closes. Reopen Mobile Capture Android to return."
                  : "Capture stopped."}
            </p>
            {session.network.state === "recording" && (
              <p className="text-xs leading-relaxed text-muted-foreground">
                Network · {session.network.detail.decoded_http_events} decoded
                events · {session.network.detail.tls_errors} TLS errors
                {session.network.detail.admission_closed
                  ? " · admission limit reached"
                  : ""}
              </p>
            )}
            {session.network.state === "recovery_required" && (
              <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
                {session.network.detail.message}
              </p>
            )}
            {(session.network.state === "stopped" ||
              session.network.state === "interrupted") && (
              <p
                className={
                  session.network.detail.routing_restored
                    ? "text-xs text-muted-foreground"
                    : "rounded-md border border-warning/30 bg-warning/10 p-2 text-xs text-warning"
                }
              >
                Network routing{" "}
                {session.network.detail.routing_restored
                  ? "restored"
                  : "needs recovery"}
                . {session.network.detail.reasons.join(" ")}
              </p>
            )}
            {session.log_attribution && (
              <Collapsible className="min-w-0 max-w-full">
                <CollapsibleTrigger asChild>
                  <Button variant="ghost" size="sm">
                    Log coverage
                  </Button>
                </CollapsibleTrigger>
                <CollapsibleContent className="space-y-2 pt-2 text-xs leading-relaxed text-muted-foreground [overflow-wrap:anywhere] select-text">
                  <p>
                    {session.log_attribution.strategy.replaceAll("_", " ")} ·{" "}
                    {session.log_attribution.application_id ?? "All apps"} ·
                    buffers {session.log_attribution.buffers.join(", ")} ·
                    levels {session.log_attribution.levels}
                  </p>
                  {session.log_attribution.exclusions.map((item) => (
                    <p key={item}>{item}</p>
                  ))}
                </CollapsibleContent>
              </Collapsible>
            )}
            {session.warnings.map((warning) => (
              <p
                className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
                key={warning}
              >
                {warning}
              </p>
            ))}
          </>
        )}
        {active && (
          <details className="relative ml-auto">
            <summary className="cursor-pointer text-sm">Add marker</summary>
            <form
              className="absolute top-full right-0 z-20 mt-2 flex w-[min(360px,80vw)] flex-wrap items-end gap-2 rounded-lg border border-border bg-card p-3 shadow-sm [&_label]:min-w-48 [&_label]:flex-1"
              onSubmit={(event) => {
                event.preventDefault();
                if (marker.trim())
                  void run("Adding marker", async () => {
                    onSnapshot(await api.marker(marker));
                    setMarker("");
                  });
              }}
            >
              <Label>
                Timeline marker
                <Input
                  value={marker}
                  maxLength={2000}
                  onChange={(event) => setMarker(event.target.value)}
                  placeholder="What happened now?"
                />
              </Label>
              <Button type="submit" disabled={!!pending || !marker.trim()}>
                Add marker
              </Button>
            </form>
          </details>
        )}
      </section>
      {actionHost &&
        createPortal(
          <section
            className="contents sm:flex sm:min-w-0 sm:flex-col sm:items-end sm:gap-1.5"
            aria-label="Recording controls"
          >
            <div className="col-start-2 row-start-1 flex flex-wrap items-center justify-end gap-2">
              {active ? (
                <>
                  <Button
                    variant="default"
                    disabled={!!pending || operationBusy(operation)}
                    onClick={() =>
                      void run("Stopping and saving", async () =>
                        onOperation(await api.save(null)),
                      )
                    }
                  >
                    Stop &amp; Save
                  </Button>
                  <Button
                    disabled={!!pending || operationBusy(operation)}
                    onClick={() =>
                      void run("Stopping capture", async () =>
                        onSnapshot(await api.stop()),
                      )
                    }
                  >
                    Stop without archive
                  </Button>
                </>
              ) : (
                <Button
                  variant="default"
                  disabled={!canStart}
                  onClick={() => void run("Starting capture", start)}
                >
                  Start recording
                </Button>
              )}
              {(snapshot?.phase === "recovery_required" ||
                snapshot?.phase === "unverified_shutdown") && (
                <Button
                  className="border-destructive/30 bg-destructive/5 text-destructive"
                  disabled={!!pending || operationBusy(operation)}
                  onClick={() =>
                    void run("Recovering device routing", async () =>
                      onSnapshot(await api.recover()),
                    )
                  }
                >
                  Retry safe recovery
                </Button>
              )}
            </div>
          </section>,
          actionHost,
        )}
    </section>
  );
}
