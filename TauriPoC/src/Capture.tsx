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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "./components/ui/popover";
import { ChevronDown, RefreshCw, SlidersHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";
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
import { CaptureInfo, isRoutineNotice } from "./Notices";

const allApps = "__all_apps__";
const phaseNames: Record<Snapshot["phase"], string> = {
  idle: "Ready",
  recording: "Recording",
  stopped: "Stopped",
  failed: "Partial evidence",
  unverified_shutdown: "Shutdown needs verification",
  recovery_required: "Routing needs recovery",
};
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
  const appOptions = [
    allApps,
    ...new Set(apps.map((app) => app.application_id)),
  ];
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
  const scopeSession =
    session &&
    (!captureSafe(snapshot) ||
      operation.state === "restoring" ||
      (operation.state === "failed" && operation.recovery_required))
      ? session
      : undefined;
  const scopeSerial = scopeSession?.device.serial ?? draft.serial;
  const scopeDevice = scopeSession?.device ?? device;
  const scopeMode = scopeSession?.selection.mode ?? draft.selection;
  const scopeLog = scopeSession?.log_scope.mode ?? draft.log_mode;
  const scopeApp =
    scopeSession?.log_scope.mode === "app"
      ? scopeSession.log_scope.application_id
      : draft.application_id;
  const scopeContext = scopeSession?.context ?? draft.context;
  const scopePolicy =
    preferences && scopeSession
      ? {
          ...preferences,
          limits: scopeSession.limits,
          network_policy: scopeSession.network_policy,
        }
      : (editedPolicy ?? preferences);
  if (scopeApp && !appOptions.includes(scopeApp)) appOptions.push(scopeApp);
  return (
    <section
      className="shrink-0 border-b border-border bg-background"
      aria-label="Capture scope"
    >
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-3 px-3 py-2">
        <div className="flex min-w-0 flex-wrap items-end gap-2">
          <Badge
            variant="secondary"
            className="my-1 h-auto max-w-40 gap-1.5 whitespace-normal text-xs"
            role="status"
          >
            <span
              className={`size-1.5 shrink-0 rounded-full ${active ? "bg-emerald-400" : "bg-muted-foreground"}`}
            />
            {pending || (snapshot ? phaseNames[snapshot.phase] : "Connecting…")}
          </Badge>
          <div className="min-w-0 w-44">
            <Label
              htmlFor="capture-device"
              className="mb-1 text-xs text-muted-foreground"
            >
              Device
            </Label>
            <div className="flex min-w-0 gap-1">
              <NativeSelect
                id="capture-device"
                value={scopeSerial}
                disabled={blocked}
                onChange={(event) =>
                  change({
                    ...draft,
                    serial: event.target.value,
                    application_id: "",
                  })
                }
              >
                <option value="">Select a device</option>
                {scopeSerial &&
                  !devices.some((item) => item.serial === scopeSerial) && (
                    <option value={scopeSerial}>
                      {scopeDevice?.name ?? scopeSerial} ·{" "}
                      {scopeDevice?.state ?? "unavailable"}
                    </option>
                  )}
                {devices.map((item) => (
                  <option key={item.serial} value={item.serial}>
                    {item.name} · {item.state}
                  </option>
                ))}
              </NativeSelect>
              <Button
                size="icon"
                variant="ghost"
                aria-label="Refresh devices"
                disabled={blocked}
                onClick={() => void run("Refreshing devices", refresh)}
              >
                <RefreshCw aria-hidden="true" />
              </Button>
            </div>
          </div>
          <Label className="w-36 min-w-0 text-xs text-muted-foreground">
            Sources
            <NativeSelect
              value={scopeMode}
              disabled={blocked}
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
          {scopeMode !== "network" && (
            <div className="w-56 min-w-0">
              <Label
                htmlFor="app-search"
                className="mb-1 text-xs text-muted-foreground"
              >
                Log app
              </Label>
              <div className="flex min-w-0 gap-1">
                <div className="min-w-0 flex-1">
                  <Combobox
                    items={appOptions}
                    value={
                      scopeLog === "whole_device" ? allApps : scopeApp || null
                    }
                    disabled={blocked}
                    onInputValueChange={setAppSearch}
                    onValueChange={(id) => {
                      change({
                        ...draft,
                        log_mode: id === allApps ? "whole_device" : "app",
                        application_id:
                          id === allApps ? draft.application_id : (id ?? ""),
                      });
                      setLogConsent(false);
                    }}
                    itemToStringLabel={(id) => {
                      if (id === allApps) return "All apps";
                      const app = apps.find(
                        (entry) => entry.application_id === id,
                      );
                      return app ? `${app.label} · ${id}` : id;
                    }}
                    filter={(id, query) => {
                      const app = apps.find(
                        (entry) => entry.application_id === id,
                      );
                      return `${id === allApps ? "All apps" : (app?.label ?? "")} ${id}`
                        .toLocaleLowerCase()
                        .includes(query.toLocaleLowerCase());
                    }}
                  >
                    <ComboboxInput
                      id="app-search"
                      placeholder="All apps or app ID"
                      disabled={blocked}
                      showClear={scopeLog !== "whole_device"}
                    />
                    <ComboboxContent>
                      <ComboboxEmpty>
                        No matching apps. Enter an application ID to use it.
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
                                {id === allApps
                                  ? "All apps"
                                  : (app?.label ?? "Use application ID")}
                              </span>
                              {id !== allApps && (
                                <span className="max-w-full break-all font-mono text-xs text-muted-foreground">
                                  {id}
                                </span>
                              )}
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
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label="Refresh apps"
                  disabled={blocked || device?.state !== "ready"}
                  onClick={() =>
                    void run("Refreshing installed apps", refreshApps)
                  }
                >
                  <RefreshCw aria-hidden="true" />
                </Button>
              </div>
            </div>
          )}
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="ghost" size="sm">
                <SlidersHorizontal aria-hidden="true" />
                Advanced
              </Button>
            </PopoverTrigger>
            <PopoverContent
              aria-label="Advanced capture settings"
              align="start"
              className="max-h-[min(36rem,75dvh)] w-[min(38rem,calc(100vw-2rem))] overflow-y-auto p-4"
            >
              <h2 className="font-medium">Task context and capture limits</h2>
              <fieldset className="space-y-3" disabled={blocked}>
                <Label>
                  Title
                  <Input
                    maxLength={512}
                    value={scopeContext.title}
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
                    value={scopeContext.notes}
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
                    value={scopeContext.steps.join("\n")}
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
              {scopePolicy && preferences && (
                <form
                  className="space-y-3"
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (blocked) return;
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
                    value={scopePolicy}
                    change={setEditedPolicy}
                    disabled={blocked}
                  />
                  <Button type="submit" disabled={blocked || !editedPolicy}>
                    Save limits
                  </Button>
                </form>
              )}
            </PopoverContent>
          </Popover>
          <CaptureInfo>
            <h2 className="font-medium text-foreground">Capture information</h2>
            <p>App selection limits Android logs only.</p>
            {!session?.warnings.some(
              (warning) =>
                isRoutineNotice(warning) &&
                warning.includes("Network capture covers"),
            ) && (
              <p>
                Network capture covers whole-device HTTP(S). HTTPS requires
                manual CA trust; certificate pinning can prevent observation.
              </p>
            )}
            <p>
              Recording continues when this window closes. Reopen Mobile Capture
              Android to return.
            </p>
            {scopeMode !== "network" && (
              <p id="app-status">
                {appState ||
                  "Refresh to read installed apps. Shared UID apps cannot be isolated."}{" "}
                The app identity is checked before collecting logs. A missing or
                changed identity stops app log collection.
              </p>
            )}
            {session?.warnings.filter(isRoutineNotice).map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
            {session?.log_attribution && (
              <>
                <p>
                  {session.log_attribution.strategy.replaceAll("_", " ")} ·{" "}
                  {session.log_attribution.application_id ?? "All apps"} ·
                  buffers {session.log_attribution.buffers.join(", ")} · levels{" "}
                  {session.log_attribution.levels}
                </p>
                {session.log_attribution.exclusions.map((item) => (
                  <p key={item}>{item}</p>
                ))}
              </>
            )}
          </CaptureInfo>
        </div>
        <section
          className="flex shrink-0 flex-wrap items-center justify-end gap-1 pt-4"
          aria-label="Recording controls"
        >
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
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    size="icon"
                    aria-label="More recording actions"
                    disabled={!!pending || operationBusy(operation)}
                  >
                    <ChevronDown aria-hidden="true" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-48">
                  <DropdownMenuItem
                    disabled={!!pending || operationBusy(operation)}
                    onSelect={() =>
                      void run("Stopping capture", async () =>
                        onSnapshot(await api.stop()),
                      )
                    }
                  >
                    Stop without archive
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
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
        </section>
      </div>
      {scopeMode !== "network" && scopeLog === "whole_device" && (
        <Label className="mx-3 mb-2 flex items-center gap-2 text-xs leading-relaxed">
          <Checkbox
            disabled={blocked}
            checked={
              scopeSession
                ? scopeSession.log_scope.mode === "whole_device"
                : logConsent
            }
            onCheckedChange={(checked) => setLogConsent(checked === true)}
          />
          <span>
            I consent to logs from every app and system process for this
            recording.
          </span>
        </Label>
      )}
      {!scopeSession && draft.serial && device?.state !== "ready" && (
        <p className="mx-3 mb-2 text-xs text-warning">
          The selected device is {device?.state ?? "unavailable"}. Unlock it,
          approve USB debugging, reconnect, and refresh. Choose a different
          device explicitly if needed.
        </p>
      )}
      {!scopeSession &&
        hasLogs &&
        selectedApp?.shared_uid &&
        draft.log_mode === "app" && (
          <p className="mx-3 mb-2 text-xs text-warning">
            This app shares its UID. Select another app or explicitly choose All
            apps.
          </p>
        )}
      {!scopeSession && !contextValidation.success && (
        <p className="mx-3 mb-2 text-xs text-warning" role="alert">
          Task context is too long. Title allows 512 UTF-8 bytes, notes 32,768
          bytes, and up to 100 steps of 4,096 bytes each.
        </p>
      )}
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
            {session.receipt && (
              <p className="text-xs text-muted-foreground">
                Logs ended · {session.receipt.reason.replaceAll("_", " ")}
              </p>
            )}
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
            {session.warnings
              .filter((warning) => !isRoutineNotice(warning))
              .map((warning) => (
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
    </section>
  );
}
