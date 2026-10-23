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
}) {
  const [apps, setApps] = useState<Application[]>([]);
  const [appSearch, setAppSearch] = useState("");
  const [appState, setAppState] = useState("");
  const appGeneration = useRef(0);
  const [logConsent, setLogConsent] = useState(false);
  const [networkConsent, setNetworkConsent] = useState(false);
  const [marker, setMarker] = useState("");
  const [editedPolicy, setEditedPolicy] = useState<Preferences>();
  const active = snapshot?.phase === "recording";
  const session =
    snapshot && snapshot.phase !== "idle" ? snapshot.session : undefined;
  const blocked =
    !!pending || operationBusy(operation) || !captureSafe(snapshot);
  const hasLogs = draft.selection !== "network";
  const hasNetwork = draft.selection !== "logs";
  const device = devices.find((entry) => entry.serial === draft.serial);
  const selectedApp = apps.find(
    (app) => app.application_id === draft.application_id,
  );
  const appValid =
    /^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$/.test(draft.application_id) &&
    !selectedApp?.shared_uid;
  const contextValidation = contextSchema.safeParse(draft.context);
  const canStart =
    contextValidation.success &&
    !blocked &&
    device?.state === "ready" &&
    !!preferences &&
    (!hasLogs || (draft.log_mode === "app" ? appValid : logConsent)) &&
    (!hasNetwork || networkConsent);
  useEffect(() => {
    appGeneration.current += 1;
    setApps([]);
    setAppState("");
    setLogConsent(false);
    setNetworkConsent(false);
  }, [draft.serial]);
  useEffect(() => {
    if (!active) {
      setLogConsent(false);
      setNetworkConsent(false);
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
        : { mode: draft.selection, consent: { whole_device: networkConsent } };
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
    setNetworkConsent(false);
  }
  return (
    <aside className="capture-sidebar">
      <section>
        <div className="section-heading">
          <h2>Android device</h2>
          <button
            className="text"
            disabled={!!pending || active || operationBusy(operation)}
            onClick={() => void run("Refreshing devices", refresh)}
          >
            Refresh
          </button>
        </div>
        <label htmlFor="capture-device">Capture on</label>
        <select
          id="capture-device"
          value={draft.serial}
          disabled={active || operationBusy(operation) || !!pending}
          onChange={(event) =>
            change({ ...draft, serial: event.target.value, application_id: "" })
          }
        >
          <option value="">Select a device</option>
          {draft.serial && !device && (
            <option value={draft.serial}>{draft.serial} · unavailable</option>
          )}
          {devices.map((item) => (
            <option key={item.serial} value={item.serial}>
              {item.name} · {item.state}
            </option>
          ))}
        </select>
        <p className="mono muted">
          {draft.serial || "Connect USB and enable USB debugging."}
        </p>
        {draft.serial && device?.state !== "ready" && (
          <p className="notice">
            The selected device is {device?.state ?? "unavailable"}. Unlock it,
            approve USB debugging, reconnect, and refresh. Choose a different
            device explicitly if needed.
          </p>
        )}
        <button className="text" onClick={setup}>
          Open device setup
        </button>
      </section>
      <section>
        <h2>Sources and scope</h2>
        <fieldset disabled={blocked}>
          <legend className="sr-only">Capture sources</legend>
          <label>
            Sources
            <select
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
                  setNetworkConsent(false);
                }
              }}
            >
              <option value="network">Network only</option>
              <option value="logs">Logs only</option>
              <option value="logs_and_network">Logs and network</option>
            </select>
          </label>
          {hasLogs && (
            <>
              <label>
                Log scope
                <select
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
                </select>
              </label>
              {draft.log_mode === "app" ? (
                <>
                  <div className="section-heading">
                    <label htmlFor="app-search">Installed apps</label>
                    <button
                      className="text"
                      disabled={device?.state !== "ready"}
                      onClick={() =>
                        void run("Refreshing installed apps", refreshApps)
                      }
                    >
                      Refresh apps
                    </button>
                  </div>
                  <input
                    id="app-search"
                    type="search"
                    placeholder="Find installed apps"
                    value={appSearch}
                    onChange={(event) => setAppSearch(event.target.value)}
                  />
                  <select
                    aria-label="Installed app"
                    value={selectedApp ? draft.application_id : ""}
                    onChange={(event) =>
                      change({ ...draft, application_id: event.target.value })
                    }
                  >
                    <option value="">Select an installed app</option>
                    {apps
                      .filter((app) =>
                        `${app.label} ${app.application_id}`
                          .toLocaleLowerCase()
                          .includes(appSearch.toLocaleLowerCase()),
                      )
                      .map((app) => (
                        <option
                          key={`${app.application_id}:${app.user_id}`}
                          value={app.application_id}
                          disabled={app.shared_uid}
                        >
                          {app.label} · {app.application_id}
                          {app.shared_uid ? " · shared UID, unavailable" : ""}
                        </option>
                      ))}
                  </select>
                  <p className="hint" role="status">
                    {appState ||
                      "Refresh to read installed apps. Shared UID apps cannot be isolated."}
                  </p>
                  <label>
                    Or enter an application ID
                    <input
                      value={draft.application_id}
                      placeholder="com.example.app"
                      onChange={(event) =>
                        change({
                          ...draft,
                          application_id: event.target.value.trim(),
                        })
                      }
                    />
                  </label>
                  {selectedApp?.shared_uid && (
                    <p className="notice">
                      This app shares its UID. Select another app or explicitly
                      choose whole-device logs.
                    </p>
                  )}
                  <p className="hint">
                    The app identity is checked before collecting logs. A
                    missing or changed identity stops app log collection.
                  </p>
                </>
              ) : (
                <label className="consent">
                  <input
                    type="checkbox"
                    checked={active ? session?.log_scope.mode === "whole_device" : logConsent}
                    onChange={(event) => setLogConsent(event.target.checked)}
                  />
                  <span>
                    I consent to logs from every app and system process for this
                    recording.
                  </span>
                </label>
              )}
            </>
          )}
          {hasNetwork && (
            <label className="consent">
              <input
                type="checkbox"
                checked={active ? session?.selection.mode !== "logs" : networkConsent}
                onChange={(event) => setNetworkConsent(event.target.checked)}
              />
              <span>
                I consent to whole-device network capture and temporary proxy
                routing for this recording. HTTPS requires manual trust of the
                capture certificate.
              </span>
            </label>
          )}
        </fieldset>
      </section>
      <section>
        <h2>Task context</h2>
        <fieldset disabled={blocked}>
          <label>
            Title
            <input
              maxLength={512}
              value={draft.context.title}
              onChange={(event) =>
                change({
                  ...draft,
                  context: { ...draft.context, title: event.target.value },
                })
              }
              placeholder="What are you investigating?"
            />
          </label>
          <label>
            Notes
            <textarea
              maxLength={32768}
              value={draft.context.notes}
              onChange={(event) =>
                change({
                  ...draft,
                  context: { ...draft.context, notes: event.target.value },
                })
              }
              rows={3}
            />
          </label>
          <label>
            Reproduction steps, one per line
            <textarea
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
          </label>
        </fieldset>
        {!contextValidation.success && (
          <p className="notice" role="alert">
            Task context is too long. Title allows 512 UTF-8 bytes, notes 32,768
            bytes, and up to 100 steps of 4,096 bytes each.
          </p>
        )}
        <details>
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
              <button disabled={blocked || !editedPolicy}>Save limits</button>
            </form>
          )}
        </details>
      </section>
      <section className="capture-actions">
        {active ? (
          <>
            <button
              className="primary"
              disabled={!!pending || operationBusy(operation)}
              onClick={() =>
                void run("Stopping and saving", async () =>
                  onOperation(await api.save(null)),
                )
              }
            >
              Stop &amp; Save
            </button>
            <button
              disabled={!!pending || operationBusy(operation)}
              onClick={() =>
                void run("Stopping capture", async () =>
                  onSnapshot(await api.stop()),
                )
              }
            >
              Stop without archive
            </button>
          </>
        ) : (
          <button
            className="primary"
            disabled={!canStart}
            onClick={() => void run("Starting capture", start)}
          >
            Start recording
          </button>
        )}
        {(snapshot?.phase === "recovery_required" ||
          snapshot?.phase === "unverified_shutdown") && (
          <button
            className="danger"
            disabled={!!pending || operationBusy(operation)}
            onClick={() =>
              void run("Recovering device routing", async () =>
                onSnapshot(await api.recover()),
              )
            }
          >
            Retry safe recovery
          </button>
        )}
        {session && (
          <>
            <div className="metrics">
              <span>{session.selection.mode === "network" ? "Network only" : `${formatBytes(session.bytes)} raw log data`}</span>
              {session.receipt && session.selection.mode !== "network" && <span>{session.receipt.records.toLocaleString()} raw records</span>}
            </div>
            <p className="hint">
              {session.receipt
                ? `Logs ended · ${session.receipt.reason.replaceAll("_", " ")}`
                : active
                  ? "Recording continues when this window closes. Reopen Mobile Capture Android to return."
                  : "Capture stopped."}
            </p>
            {session.network.state === "recording" && (
              <p className="hint">
                Network · {session.network.detail.decoded_http_events} decoded
                events · {session.network.detail.tls_errors} TLS errors
                {session.network.detail.admission_closed
                  ? " · admission limit reached"
                  : ""}
              </p>
            )}
            {session.network.state === "recovery_required" && (
              <p className="notice">{session.network.detail.message}</p>
            )}
            {(session.network.state === "stopped" ||
              session.network.state === "interrupted") && (
              <p
                className={
                  session.network.detail.routing_restored ? "hint" : "notice"
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
              <details>
                <summary>Log coverage</summary>
                <p className="hint">
                  {session.log_attribution.strategy.replaceAll("_", " ")} ·{" "}
                  {session.log_attribution.application_id ?? "All apps"} ·
                  buffers {session.log_attribution.buffers.join(", ")} · levels{" "}
                  {session.log_attribution.levels}
                </p>
                {session.log_attribution.exclusions.map((item) => (
                  <p className="hint" key={item}>
                    {item}
                  </p>
                ))}
              </details>
            )}
            {session.warnings.map((warning) => (
              <p className="notice" key={warning}>
                {warning}
              </p>
            ))}
          </>
        )}
        {active && (
          <form
            className="marker-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (marker.trim())
                void run("Adding marker", async () => {
                  onSnapshot(await api.marker(marker));
                  setMarker("");
                });
            }}
          >
            <label>
              Timeline marker
              <input
                value={marker}
                maxLength={2000}
                onChange={(event) => setMarker(event.target.value)}
                placeholder="What happened now?"
              />
            </label>
            <button disabled={!!pending || !marker.trim()}>Add marker</button>
          </form>
        )}
      </section>
    </aside>
  );
}
