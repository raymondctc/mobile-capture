import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { api, type Device, type Event, type Snapshot } from "./api";
import "./style.css";

type Action = "refresh" | "start" | "marker" | "stop" | "export";
const phaseNames = { idle: "Ready to capture", recording: "Recording", stopped: "Stopped · collector verified", failed: "Collector ended · partial capture", unverified_shutdown: "Shutdown unverified · export disabled" } satisfies Record<Snapshot["phase"], string>;
function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [devices, setDevices] = useState<Device[]>([]);
  const [serial, setSerial] = useState("");
  const [consent, setConsent] = useState(false);
  const [pending, setPending] = useState<Action>();
  const [error, setError] = useState("");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [marker, setMarker] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const busy = useRef(false);
  const generation = useRef(0);
  const active = snapshot?.phase === "recording";
  const current = snapshot && snapshot.phase !== "idle" ? snapshot.session : undefined;
  const fixture = snapshot?.session.fixture;
  const events = current?.events ?? [];
  const selected = events.find((event) => event.id === selectedId);
  const displayed = events.filter((event) => (filter === "all" || event.source === filter) && `${event.summary} ${event.raw_ref}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  const canStart = snapshot && snapshot.phase !== "recording" && snapshot.phase !== "unverified_shutdown" && devices.some((device) => device.serial === serial && device.state === "ready") && consent;
  const canExport = snapshot?.phase === "stopped" || snapshot?.phase === "failed";

  async function refreshDevices() {
    const found = await api.discover();
    setDevices(found);
    setSerial((previous) => found.some((device) => device.serial === previous && device.state === "ready") ? previous : found.find((device) => device.state === "ready")?.serial ?? "");
  }
  async function perform(action: Action, operation: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    generation.current += 1;
    setPending(action);
    setError("");
    try { await operation(); } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { busy.current = false; setPending(undefined); }
  }
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const request = generation.current;
      if (!busy.current) {
        try { const result = await api.snapshot(); if (!disposed && request === generation.current) setSnapshot(result); }
        catch (cause) { if (!disposed) setError(String(cause)); }
      }
      if (!disposed) timer = setTimeout(poll, 650);
    }
    void poll();
    void perform("refresh", refreshDevices);
    return () => { disposed = true; clearTimeout(timer); };
  }, []);

  return <div className="app">
    <header><div><div className="eyebrow">ANDROID DIAGNOSTICS / PROOF OF CONCEPT</div><h1>Mobile Capture <span>PoC</span></h1></div><div className={`phase ${active ? "live" : ""}`} role="status"><i/>{pending === "stop" ? "Stopping · waiting for collector" : snapshot ? phaseNames[snapshot.phase] : "Connecting to native capture owner…"}</div></header>
    {fixture && <div className="fixture" role="status">FIXTURE MODE <span>Compiled fake ADB. No phone is connected by this launcher.</span></div>}
    {error && <div className="error" role="alert"><strong>Action could not complete.</strong> {error}</div>}
    <div className="layout"><aside>
      <section><div className="section-heading"><h2>Device</h2><button className="text" disabled={!!pending || active} onClick={() => void perform("refresh", refreshDevices)}>{pending === "refresh" ? "Searching…" : "Refresh"}</button></div>
        <label htmlFor="device">Android device</label><select id="device" value={serial} disabled={active || !!pending} onChange={(event) => { setSerial(event.target.value); setConsent(false); }}><option value="">{devices.length ? "Select a ready device" : "No devices found"}</option>{devices.map((device) => <option key={device.serial} value={device.serial} disabled={device.state !== "ready"}>{device.name} · {device.state}</option>)}</select>
        <p className="mono muted">{current?.device.serial ?? (serial || "Connect USB and enable USB debugging.")}</p>
        {devices.some((device) => device.state === "unauthorized") && <p className="hint">Unlock your phone and accept its USB debugging prompt, then refresh.</p>}
        {!devices.length && <p className="hint">Install Android platform-tools, connect an Android device or start an emulator, then refresh.</p>}
      </section>
      <section><h2>Capture scope</h2><div className="scope"><span>Device logs</span><strong>Whole device</strong></div><label className="consent"><input type="checkbox" checked={consent} disabled={active || !!pending} onChange={(event) => setConsent(event.target.checked)}/><span>I consent to collecting logs from every app and system process on this device.</span></label>
        <div className="unavailable"><span>App scope</span><small>Unavailable in this PoC</small></div><div className="unavailable"><span>Network capture</span><small>Unavailable in this PoC</small></div>
        <div className="actions"><button className="primary" disabled={!canStart || !!pending} onClick={() => void perform("start", async () => { setSnapshot(await api.start({ serial, wholeDeviceConsent: consent })); setSelectedId(""); })}>{pending === "start" ? "Starting…" : "Start capture"}</button><button className="stop" disabled={!active || !!pending} onClick={() => void perform("stop", async () => setSnapshot(await api.stop()))}>{pending === "stop" ? "Stopping…" : "Stop"}</button></div>
        <p className="hint">Stops at 32 MiB, 100,000 raw records, or 120 seconds. Stop waits for the owned collector.</p>
      </section>
      <section><h2>Evidence</h2><dl><div><dt>Log events</dt><dd>{current?.logs.toLocaleString() ?? "0"}</dd></div><div><dt>Markers</dt><dd>{current?.markers ?? 0}</dd></div><div><dt>Raw bytes</dt><dd>{current?.bytes.toLocaleString() ?? "0"}</dd></div></dl><button className="export" disabled={!canExport || !!pending} onClick={() => void perform("export", async () => { await api.export(); setSnapshot(await api.snapshot()); })}>{pending === "export" ? "Building ZIP…" : "Export evidence ZIP"}</button>
        {current?.archive ? <p className="archive" role="status">Saved locally <code>{current.archive}</code></p> : <p className="hint">Export becomes available after verified shutdown. Partial outcomes remain in the manifest.</p>}
      </section>
    </aside><main>
      <div className="timeline-heading"><div><h2>Timeline</h2><p>Latest 500 events. Search covers this recent window. Export includes all admitted evidence.</p></div><span className="count">{displayed.length} shown</span></div>
      <div className="toolbar"><input aria-label="Search recent events" placeholder="Search recent events…" value={search} onChange={(event) => setSearch(event.target.value)}/><select aria-label="Event source" value={filter} onChange={(event) => setFilter(event.target.value)}><option value="all">All sources</option><option value="logs">Device logs</option><option value="marker">Markers</option></select></div>
      <form className="marker-form" onSubmit={(event) => { event.preventDefault(); void perform("marker", async () => { setSnapshot(await api.marker({ text: marker })); setMarker(""); }); }}><input aria-label="Marker text" placeholder="Mark a reproduction step or observation" value={marker} maxLength={4096} disabled={!active || !!pending} onChange={(event) => setMarker(event.target.value)}/><button disabled={!active || !marker.trim() || !!pending}>Add marker</button></form>
      <div className="events" aria-label="Capture timeline">{displayed.length ? displayed.map((event) => <button key={event.id} className={`event ${selectedId === event.id ? "selected" : ""}`} onClick={() => setSelectedId(event.id)} aria-pressed={selectedId === event.id}><time>{event.timestamp.slice(11, 23)}</time><span className={`source ${event.source}`}>{event.source === "marker" ? "MARK" : event.detail.level ? String(event.detail.level) : "RAW"}</span><span className="summary">{event.summary}</span></button>) : <div className="empty"><h3>{current ? search || filter !== "all" ? "No matching recent events" : active ? "Waiting for device logs" : "No log events were admitted" : "Ready when you are"}</h3><p>{current ? "Raw evidence and any coverage gaps appear in the export manifest." : "Select an authorized device, consent to whole-device logs, and start capture."}</p></div>}</div>
      <Details event={selected}/>
      {current && <details className="notes"><summary>Scope, clock, and coverage notes</summary>{current.warnings.map((warning, index) => <p key={`${index}-${warning}`}>{warning}</p>)}</details>}
    </main></div><footer><span>Isolated PoC · no routing or trust changes</span><span>Device clock uncertainty unknown · schema 1 evidence</span></footer>
  </div>;
}
function Details({ event }: { event: Event | undefined }) { return <section className="details"><h2>Event details</h2>{event ? <><p className="detail-summary">{event.summary}</p><dl><div><dt>Raw reference</dt><dd>{event.raw_ref}</dd></div><div><dt>Stable event ID</dt><dd>{event.id}</dd></div><div><dt>Timestamp</dt><dd>{event.timestamp}</dd></div><div><dt>Clock uncertainty</dt><dd>{event.clock_uncertainty_ms === null ? "Unknown" : `${event.clock_uncertainty_ms} ms · host marker`}</dd></div></dl><pre>{JSON.stringify(event.detail, null, 2)}</pre></> : <p className="hint">Select an event to inspect its raw reference, clock basis, and parsed fields.</p>}</section>; }
const root = document.getElementById("root");
if (root) createRoot(root).render(<React.StrictMode><App/></React.StrictMode>);
