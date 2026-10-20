import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { api, bodyPreviews, type BodyPreview, type Device, type Event, type NetworkView, type Snapshot } from "./api";
import "./style.css";

type Action = "refresh" | "start" | "marker" | "stop" | "recover" | "export";
const phaseNames = {
  idle: "Ready to capture",
  recording: "Recording",
  stopped: "Stopped",
  failed: "Capture ended · partial evidence",
  unverified_shutdown: "Shutdown unverified · export disabled",
  recovery_required: "Device routing needs recovery",
} satisfies Record<Snapshot["phase"], string>;
function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [devices, setDevices] = useState<Device[]>([]);
  const [serial, setSerial] = useState("");
  const [consent, setConsent] = useState(false);
  const [networkConsent, setNetworkConsent] = useState(false);
  const [pending, setPending] = useState<Action>();
  const [error, setError] = useState("");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [marker, setMarker] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const busy = useRef(false);
  const generation = useRef(0);
  const active = snapshot?.phase === "recording";
  const current =
    snapshot && snapshot.phase !== "idle" ? snapshot.session : undefined;
  const fixture = snapshot?.session.fixture;
  const network = current?.network;
  const networkSelected = network?.state !== undefined && network.state !== "not_selected";
  const events = current?.events ?? [];
  const selected = events.find((event) => event.id === selectedId);
  const displayed = events.filter(
    (event) =>
      (filter === "all" || event.source === filter) &&
      `${event.summary} ${event.raw_ref}`
        .toLocaleLowerCase()
        .includes(search.toLocaleLowerCase()),
  );
  const canStart =
    snapshot &&
    (snapshot.phase === "idle" || snapshot.phase === "stopped" || snapshot.phase === "failed") &&
    network?.state !== "recovery_required" &&
    devices.some(
      (device) => device.serial === serial && device.state === "ready",
    ) &&
    consent;
  const canExport =
    (snapshot?.phase === "stopped" || snapshot?.phase === "failed") &&
    (network?.state === "not_selected" ||
      ((network?.state === "stopped" || network?.state === "interrupted") &&
        network.detail.routing_restored));

  async function refreshDevices() {
    const found = await api.discover();
    setDevices(found);
    setSerial((previous) =>
      found.some(
        (device) => device.serial === previous && device.state === "ready",
      )
        ? previous
        : (found.find((device) => device.state === "ready")?.serial ?? ""),
    );
  }
  async function perform(action: Action, operation: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    generation.current += 1;
    setPending(action);
    setError("");
    try {
      await operation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      busy.current = false;
      setPending(undefined);
    }
  }
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const request = generation.current;
      if (!busy.current) {
        try {
          const result = await api.snapshot();
          if (!disposed && request === generation.current) setSnapshot(result);
        } catch (cause) {
          if (!disposed) setError(String(cause));
        }
      }
      if (!disposed) timer = setTimeout(poll, 650);
    }
    void poll();
    void perform("refresh", refreshDevices);
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);

  return (
    <div className="app">
      <header>
        <div>
          <div className="eyebrow">ANDROID DIAGNOSTICS / PROOF OF CONCEPT</div>
          <h1>
            Mobile Capture <span>PoC</span>
          </h1>
        </div>
        <div className={`phase ${active ? "live" : ""}`} role="status">
          <i />
          {pending === "stop" || pending === "recover"
            ? "Checking device recovery…"
            : snapshot
              ? phaseNames[snapshot.phase]
              : "Connecting to native capture owner…"}
        </div>
      </header>
      {fixture && (
        <div className="fixture" role="status">
          FIXTURE MODE{" "}
          <span>
            Compiled fake ADB. Network mode uses the installed proxy engine if selected. This launcher does not capture phone traffic.
          </span>
        </div>
      )}
      {error && (
        <div className="error" role="alert">
          <strong>Action could not complete.</strong> {error}
        </div>
      )}
      <div className="layout">
        <aside>
          <section>
            <div className="section-heading">
              <h2>Device</h2>
              <button
                className="text"
                disabled={!!pending || active}
                onClick={() => void perform("refresh", refreshDevices)}
              >
                {pending === "refresh" ? "Searching…" : "Refresh"}
              </button>
            </div>
            <label htmlFor="device">Android device</label>
            <select
              id="device"
              value={serial}
              disabled={active || !!pending}
              onChange={(event) => {
                setSerial(event.target.value);
                setConsent(false);
                setNetworkConsent(false);
              }}
            >
              <option value="">
                {devices.length ? "Select a ready device" : "No devices found"}
              </option>
              {devices.map((device) => (
                <option
                  key={device.serial}
                  value={device.serial}
                  disabled={device.state !== "ready"}
                >
                  {device.name} · {device.state}
                </option>
              ))}
            </select>
            <p className="mono muted">
              {current?.device.serial ??
                (serial || "Connect USB and enable USB debugging.")}
            </p>
            {devices.some((device) => device.state === "unauthorized") && (
              <p className="hint">
                Unlock your phone and accept its USB debugging prompt, then
                refresh.
              </p>
            )}
            {!devices.length && (
              <p className="hint">
                Install Android platform-tools, connect an Android device or
                start an emulator, then refresh.
              </p>
            )}
          </section>
          <section>
            <h2>Capture scope</h2>
            <div className="scope">
              <span>Device logs</span>
              <strong>Whole device</strong>
            </div>
            <label className="consent">
              <input
                type="checkbox"
                checked={consent}
                disabled={active || !!pending}
                onChange={(event) => setConsent(event.target.checked)}
              />
              <span>
                I consent to collecting logs from every app and system process
                on this device.
              </span>
            </label>
            <div className="unavailable">
              <span>App scope</span>
              <small>Unavailable in this PoC</small>
            </div>
            <label className="consent network-consent">
              <input
                type="checkbox"
                checked={networkConsent}
                disabled={active || !!pending}
                onChange={(event) => setNetworkConsent(event.target.checked)}
              />
              <span>
                Also capture whole-device network traffic. I consent to temporary device proxy routing and manual trust of the session certificate.
              </span>
            </label>
            {networkConsent && (
              <p className="hint">
                Requires installed mitmdump 12.2.3. Trust the session certificate on the Android device manually; HTTPS content without trust is not observed. App scope and iPhone capture are unavailable.
              </p>
            )}
            <div className="actions">
              <button
                className="primary"
                disabled={!canStart || !!pending}
                onClick={() =>
                  void perform("start", async () => {
                    setSnapshot(
                      await api.start({ serial, wholeDeviceConsent: consent, networkConsent }),
                    );
                    setSelectedId("");
                  })
                }
              >
                {pending === "start" ? "Starting…" : "Start capture"}
              </button>
              <button
                className="stop"
                disabled={!active || !!pending}
                onClick={() =>
                  void perform("stop", async () =>
                    setSnapshot(await api.stop()),
                  )
                }
              >
                {pending === "stop" ? "Stopping…" : "Stop"}
              </button>
              {snapshot?.phase === "recovery_required" && (
                <button
                  className="stop"
                  disabled={!!pending}
                  onClick={() => void perform("recover", async () => setSnapshot(await api.recover()))}
                >
                  {pending === "recover" ? "Retrying…" : "Retry recovery"}
                </button>
              )}
            </div>
            <p className="hint">
              Logs stop at 32 MiB, 100,000 raw records, or 120 seconds. Network admission is separately bounded. Stop waits for device routing restoration and source shutdown.
            </p>
          </section>
          {network && network.state !== "not_selected" && <NetworkStatus network={network} />}
          <section>
            <h2>Evidence</h2>
            <dl>
              <div>
                <dt>Log events</dt>
                <dd>{current?.logs.toLocaleString() ?? "0"}</dd>
              </div>
              <div>
                <dt>Markers</dt>
                <dd>{current?.markers ?? 0}</dd>
              </div>
              {networkSelected && (
                <div>
                  <dt>Network events</dt>
                  <dd>{events.filter((event) => event.source === "network").length.toLocaleString()} in recent timeline</dd>
                </div>
              )}
              <div>
                <dt>Raw bytes</dt>
                <dd>{current?.bytes.toLocaleString() ?? "0"}</dd>
              </div>
            </dl>
            <button
              className="export"
              disabled={!canExport || !!pending}
              onClick={() =>
                void perform("export", async () => {
                  await api.export();
                  setSnapshot(await api.snapshot());
                })
              }
            >
              {pending === "export" ? "Building ZIP…" : "Export evidence ZIP"}
            </button>
            {current?.archive ? (
              <p className="archive" role="status">
                Saved locally <code>{current.archive}</code>
              </p>
            ) : (
              <p className="hint">
                Export becomes available after verified shutdown and device routing restoration. Partial outcomes remain in the manifest.
              </p>
            )}
          </section>
        </aside>
        <main>
          <div className="timeline-heading">
            <div>
              <h2>Timeline</h2>
              <p>
                Latest 500 events. Search covers this recent window. Export
                includes all admitted evidence.
              </p>
            </div>
            <span className="count">{displayed.length} shown</span>
          </div>
          <div className="toolbar">
            <input
              aria-label="Search recent events"
              placeholder="Search recent events…"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            <select
              aria-label="Event source"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
            >
              <option value="all">All sources</option>
              <option value="logs">Device logs</option>
              <option value="marker">Markers</option>
              <option value="network">Network</option>
            </select>
          </div>
          <form
            className="marker-form"
            onSubmit={(event) => {
              event.preventDefault();
              void perform("marker", async () => {
                setSnapshot(await api.marker({ text: marker }));
                setMarker("");
              });
            }}
          >
            <input
              aria-label="Marker text"
              placeholder="Mark a reproduction step or observation"
              value={marker}
              maxLength={4096}
              disabled={!active || !!pending}
              onChange={(event) => setMarker(event.target.value)}
            />
            <button disabled={!active || !marker.trim() || !!pending}>
              Add marker
            </button>
          </form>
          <div className="events" aria-label="Capture timeline">
            {displayed.length ? (
              displayed.map((event) => (
                <button
                  key={event.id}
                  className={`event ${selectedId === event.id ? "selected" : ""}`}
                  onClick={() => setSelectedId(event.id)}
                  aria-pressed={selectedId === event.id}
                >
                  <time>{event.timestamp.slice(11, 23)}</time>
                  <span className={`source ${event.source}`}>
                    {event.source === "marker"
                      ? "MARK"
                      : event.source === "network"
                        ? "NET"
                      : event.detail.level
                        ? String(event.detail.level)
                        : "RAW"}
                  </span>
                  <span className="summary">{event.summary}</span>
                </button>
              ))
            ) : (
              <div className="empty">
                <h3>
                  {current
                    ? search || filter !== "all"
                      ? "No matching recent events"
                      : active
                        ? "Waiting for events"
                        : "No events were admitted"
                    : "Ready when you are"}
                </h3>
                <p>
                  {current
                    ? "Raw evidence and any coverage gaps appear in the export manifest."
                    : "Select an authorized device, consent to whole-device logs, and start capture."}
                </p>
              </div>
            )}
          </div>
          <Details event={selected} />
          {current && (
            <details className="notes">
              <summary>Scope, clock, and coverage notes</summary>
              {current.warnings.map((warning, index) => (
                <p key={`${index}-${warning}`}>{warning}</p>
              ))}
            </details>
          )}
        </main>
      </div>
      <footer>
        <span>{networkSelected ? "Network mode · temporary device proxy routing; certificate trust is manual" : "Log-only PoC · no routing or trust changes"}</span>
        <span>Device clock uncertainty unknown · schema 1 evidence</span>
      </footer>
    </div>
  );
}
function Details({ event }: { event: Event | undefined }) {
  return (
    <section className="details">
      <h2>Event details</h2>
      {event ? (
        <>
          <p className="detail-summary">{event.summary}</p>
          <dl>
            <div>
              <dt>Raw reference</dt>
              <dd>{event.raw_ref}</dd>
            </div>
            <div>
              <dt>Stable event ID</dt>
              <dd>{event.id}</dd>
            </div>
            <div>
              <dt>Timestamp</dt>
              <dd>{event.timestamp}</dd>
            </div>
            <div>
              <dt>Clock uncertainty</dt>
              <dd>
                {event.clock_uncertainty_ms === null
                  ? "Unknown"
                  : `${event.clock_uncertainty_ms} ms · host marker`}
              </dd>
            </div>
          </dl>
          {event.source === "network" && (
            <>
              {bodyPreviews(event.detail).map(({ label, body }) => <BodyEvidence key={label} label={label} body={body} />)}
              <p className="hint" role="note">The fields below describe this event. The export manifest records any unavailable bodies and coverage gaps.</p>
            </>
          )}
          <pre>{JSON.stringify(event.detail, null, 2)}</pre>
        </>
      ) : (
        <p className="hint">
          Select an event to inspect its raw reference, clock basis, and parsed
          fields.
        </p>
      )}
    </section>
  );
}
function utf8Preview(base64: string): string | undefined {
  try {
    const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return /[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(text) ? undefined : text;
  } catch {
    return undefined;
  }
}
function BodyEvidence({ label, body }: { label: string; body: BodyPreview }) {
  if ("unavailable" in body) {
    return <p className="hint"><strong>{label} body unavailable.</strong> {body.reason}</p>;
  }
  const decoded = utf8Preview(body.text);
  const preview = decoded ?? body.text;
  const format = decoded === undefined ? "base64" : "UTF-8";
  return (
    <div className="body-preview">
      <p className="hint"><strong>{label} body preview ({format}).</strong> {body.preview_bytes.toLocaleString()} of {body.retained_bytes.toLocaleString()} retained raw bytes shown. The admitted complete bytes are in the export.</p>
      <pre>{preview}</pre>
    </div>
  );
}
function NetworkStatus({ network }: { network: NetworkView }) {
  if (network.state === "not_selected") return null;
  if (network.state === "preparing") {
    return <section><h2>Network coverage</h2><p className="hint">Preparing the installed proxy engine and session certificate before device routing changes.</p></section>;
  }
  if (network.state === "recovery_required") {
    return (
      <section className="network-status" role="alert">
        <h2>Device recovery required</h2>
        <p className="hint">{network.detail.message}</p>
        <p className="hint">Keep the device connected and retry recovery. Export and a new capture are disabled until routing is restored and sources are verified.</p>
        {network.detail.guardian_interrupted && <p className="hint">The capture process was interrupted. Evidence will be marked partial.</p>}
      </section>
    );
  }
  const coverage = network.state === "recording" ? network.detail : network.detail.coverage;
  const completion = network.state === "recording" ? undefined : network.detail;
  return (
    <section className="network-status">
      <h2>Network coverage</h2>
      <dl>
        <div><dt>Decoded HTTP events</dt><dd>{coverage.decoded_http_events.toLocaleString()}</dd></div>
        <div><dt>TLS errors</dt><dd>{coverage.tls_errors.toLocaleString()}</dd></div>
        <div><dt>Omitted flows</dt><dd>{coverage.omitted_flows.toLocaleString()}</dd></div>
        <div><dt>Unavailable bodies</dt><dd>{coverage.unavailable_bodies.toLocaleString()}</dd></div>
      </dl>
      {coverage.admission_closed && <p className="hint">Evidence admission reached a limit. The proxy may still forward traffic while the session is active.</p>}
      {coverage.public_certificate && <p className="hint">Trust this session certificate manually on the Android device: <code>{coverage.public_certificate}</code></p>}
      {coverage.synthetic_check_url && <p className="hint">After trusting it, visit <code>{coverage.synthetic_check_url}</code> on that device to check HTTPS capture.</p>}
      {coverage.decoded_http_events === 0 && <p className="hint">No decoded HTTP has been observed. TLS errors alone do not establish captured content.</p>}
      {completion && (
        <>
          <p className="hint">{completion.routing_restored ? "Device routing restored." : "Device routing restoration unverified."} {completion.partial ? "Network evidence is partial." : "No network gaps reported by the collector."}</p>
          {completion.forced && <p className="hint">A capture process required forced shutdown.</p>}
          {completion.interrupted && <p className="hint">Capture was interrupted. Check the manifest for missing or incomplete sources.</p>}
          {completion.reasons.map((reason) => <p className="hint" key={reason}>{reason}</p>)}
        </>
      )}
    </section>
  );
}
const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
