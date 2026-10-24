import assert from "node:assert/strict";
import { useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { Capture } from "../src/Capture";
import { Details } from "../src/Details";
import { Timeline } from "../src/Timeline";
import { OperationStatus } from "../src/Operation";
import { Dialog } from "../src/ui";
import { Settings } from "../src/Settings";
import { HandoffNotices } from "../src/Handoff";
import { Library } from "../src/Library";
import { isRoutineNotice } from "../src/Notices";
import { App } from "../src/main";
import { api } from "../src/api";
import { product } from "../src/product";
import { initialDraft } from "../src/state";

const limits = { bytes: 32000000, records: 100000, duration_ms: 120000 };
const policy = {
  max_flows: 10000,
  max_body_bytes: 33554432,
  max_header_bytes: 131072,
  max_total_bytes: 1073741824,
  duration_ms: 3600000,
};
const preferences = {
  output_root: "/evidence",
  limits,
  network_policy: policy,
  adb_override: null,
  mitmdump_override: null,
};
const device = { serial: "phone-a", name: "Phone A", state: "ready" };
const idle = { phase: "idle", session: { fixture: false, limits } };
const descriptor = {
  version: 1,
  id: "B",
  directory: "/evidence/B",
  device,
  started_at: "2026-10-23T00:00:00Z",
  legacy: false,
  metadata: null,
  warnings: [],
};
const coverage = {
  decoded_http_events: 1,
  tls_errors: 0,
  omitted_flows: 0,
  unavailable_bodies: 0,
  admission_closed: false,
  public_certificate: null,
  synthetic_check_url: null,
};
const session = {
  id: "A",
  directory: "/evidence/A",
  device,
  started_at: descriptor.started_at,
  limits,
  fixture: false,
  bytes: 1024,
  logs: 1,
  markers: 0,
  receipt: null,
  archive: null,
  warnings: [],
  descriptor_version: 1,
  selection: { mode: "network", consent: { whole_device: true } },
  log_scope: { mode: "app", application_id: "com.example.app" },
  network_policy: policy,
  context: { title: "", notes: "", steps: [] },
  log_attribution: null,
  network: { state: "recording", detail: coverage },
};
const event = (id: string) => ({
  id,
  timestamp: descriptor.started_at,
  source: "network",
  kind: "http_request",
  summary: `GET https://example.invalid/${id}`,
  raw_ref: "flows:1",
  clock_uncertainty_ms: null,
  original_timestamp: null,
  received_at: null,
});
const detail = (id: string) => ({
  event: event(id),
  detail_bytes: 10,
  bodies: [],
});
const progress = {
  indexed: 401,
  raw_bytes: 100,
  indexed_bytes: 100,
  backlog_bytes: 0,
  complete: true,
  latest_sequence: 401,
};
const page = {
  generation: "g",
  rows: [
    { sequence: 1, event: event("one") },
    { sequence: 2, event: event("two") },
  ],
  next_cursor: "older",
  total: 401,
  matching: 401,
  progress,
};
const emptyChunk = {
  offset: 0,
  total_bytes: 0,
  data_base64: "",
  next_offset: null,
};
const run = async (_label: string, action: () => Promise<unknown>) => {
  await action();
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: Error) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const user = userEvent.setup();
async function checkProgress() {
  const props = {
    change: () => {},
    run,
    pending: "",
    recover: async () => {},
  };
  const saving = {
    state: "saving",
    id: "save-A",
    stage: "indexing",
    completed: 50,
    total: 100,
    cancel_allowed: true,
  };
  const mounted = render(<OperationStatus {...props} operation={saving} />);
  const progressbar = screen.getByRole("progressbar", {
    name: "Archive progress",
  });
  assert.equal(progressbar.getAttribute("aria-valuenow"), "50");
  assert.equal(progressbar.getAttribute("data-state"), "loading");
  mounted.rerender(
    <OperationStatus {...props} operation={{ ...saving, total: null }} />,
  );
  assert.equal(progressbar.getAttribute("aria-valuenow"), null);
  assert.equal(progressbar.getAttribute("data-state"), "indeterminate");
  cleanup();
  console.log(
    "Progress DOM checks passed. A known 50 percent archive exposes its value and loading state; an unknown total stays indeterminate.",
  );
}
async function checkCaptureScope() {
  const starts: unknown[] = [];
  api.start = async (request) => {
    starts.push(request);
    return idle;
  };
  const unavailable = {
    serial: "phone-offline",
    name: "Offline phone",
    state: "offline",
  };
  function Harness({
    devices = [device, unavailable],
    draftValue = initialDraft,
    loading = false,
  }) {
    const [draft, change] = useState(draftValue);
    return (
      <Capture
        snapshot={idle}
        operation={{ state: "idle" }}
        devices={devices}
        draft={draft}
        change={change}
        preferences={loading ? undefined : preferences}
        savePreferences={async () => {}}
        refresh={async () => {}}
        run={run}
        pending=""
        onSnapshot={() => {}}
        onOperation={() => {}}
      />
    );
  }
  const mounted = render(<Harness />);
  const captureDevice = screen.getByLabelText("Device");
  await user.selectOptions(captureDevice, unavailable.serial);
  assert.equal(screen.queryByRole("button", { name: /capture scope/ }), null);
  assert.equal(screen.getByLabelText("Device").value, unavailable.serial);
  assert.ok(screen.getByText(/The selected device is offline/));
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    true,
  );
  mounted.rerender(<Harness devices={[]} />);
  assert.equal(screen.getByLabelText("Device").value, unavailable.serial);
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    true,
  );
  assert.equal(starts.length, 0);
  cleanup();
  const savedDraft = { ...initialDraft, serial: device.serial };
  const restored = render(
    <Harness devices={[]} draftValue={savedDraft} loading />,
  );
  assert.equal(screen.getByLabelText("Device").value, device.serial);
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    true,
  );
  restored.rerender(<Harness draftValue={savedDraft} />);
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    false,
  );
  assert.equal(starts.length, 0);
  cleanup();
  console.log(
    "Direct scope DOM checks passed. Device and Sources stay available, unavailable selection persists through discovery, and Start requires readiness without autostart.",
  );
}
async function checkCapture() {
  const starts: unknown[] = [];
  const markers: string[] = [];
  api.start = async (request) => {
    starts.push(request);
    return idle;
  };
  api.marker = async (text) => {
    markers.push(text);
    return { phase: "recording", session };
  };
  function Harness({
    snapshot = idle,
    pending = "",
    draftValue = initialDraft,
    operation = { state: "idle" },
  }) {
    const [draft, change] = useState({ ...draftValue, serial: "phone-a" });
    return (
      <Capture
        snapshot={snapshot}
        operation={operation}
        devices={[device]}
        draft={draft}
        change={change}
        preferences={preferences}
        savePreferences={async () => {}}
        refresh={async () => {}}
        run={run}
        pending={pending}
        onSnapshot={() => {}}
        onOperation={() => {}}
      />
    );
  }
  let mounted = render(<Harness />);
  assert.equal(starts.length, 0);
  const start = screen.getByRole("button", { name: "Start recording" });
  assert.equal(start.disabled, false);
  assert.equal(start.getAttribute("aria-describedby"), null);
  assert.equal(screen.queryByText(/Start records whole-device HTTP/), null);
  assert.equal(screen.queryByRole("checkbox"), null);
  await user.click(start);
  assert.deepEqual(starts[0], {
    serial: "phone-a",
    whole_device_consent: false,
    selection: { mode: "network", consent: { whole_device: true } },
    log_scope: { mode: "app", application_id: "" },
    limits,
    network_policy: policy,
    context: initialDraft.context,
  });
  assert.equal(
    screen
      .getByRole("button", { name: "Advanced" })
      .getAttribute("aria-expanded"),
    "false",
  );
  assert.equal(screen.queryByLabelText("Title"), null);
  assert.equal(screen.queryByRole("combobox", { name: "Log app" }), null);
  await user.selectOptions(
    screen.getByLabelText("Sources"),
    "logs_and_network",
  );
  const logApp = screen.getByRole("combobox", { name: "Log app" });
  await user.click(logApp);
  await user.click(await screen.findByRole("option", { name: "All apps" }));
  assert.equal(start.disabled, true);
  const consent = screen.getByRole("checkbox", {
    name: /I consent to logs from every app/,
  });
  consent.focus();
  await user.keyboard(" ");
  assert.equal(consent.getAttribute("aria-checked"), "true");
  assert.equal(start.disabled, false);
  await user.click(start);
  assert.equal(starts[1].whole_device_consent, true);
  assert.equal(consent.getAttribute("aria-checked"), "false");
  assert.equal(
    start.disabled,
    true,
    "All apps consent is required again for the next recording",
  );
  mounted.unmount();
  for (const props of [
    { pending: "Saving" },
    { snapshot: { phase: "recovery_required", session } },
    {
      draftValue: {
        ...initialDraft,
        context: { title: "界".repeat(171), notes: "", steps: [] },
      },
    },
    {
      operation: {
        state: "failed",
        id: "failed",
        kind: "save",
        message: "Recovery",
        recovery_required: true,
        directory: null,
      },
    },
  ]) {
    mounted = render(<Harness {...props} />);
    assert.equal(
      screen.getByRole("button", { name: "Start recording" }).disabled,
      true,
    );
    mounted.unmount();
  }
  mounted = render(
    <Harness
      snapshot={{ phase: "recording", session }}
      draftValue={{
        ...initialDraft,
        selection: "logs",
        serial: "edited-device",
      }}
    />,
  );
  assert.equal(screen.getByLabelText("Sources").value, "network");
  assert.equal(screen.getByLabelText("Device").value, session.device.serial);
  assert.equal(screen.getByLabelText("Sources").disabled, true);
  assert.equal(screen.getByLabelText("Device").disabled, true);
  assert.equal(screen.queryByText(/Start records whole-device HTTP/), null);
  await user.click(screen.getByText("Add marker", { selector: "summary" }));
  await user.type(
    screen.getByLabelText("Timeline marker"),
    "Reproduced{Enter}",
  );
  await waitFor(() => assert.deepEqual(markers, ["Reproduced"]));
  mounted.rerender(
    <Harness
      snapshot={{
        phase: "recording",
        session: {
          ...session,
          selection: {
            mode: "logs_and_network",
            consent: { whole_device: true },
          },
          warnings: [
            "Device clock uncertainty is unknown. Time proximity does not establish causality.",
            "Unknown coverage error. Inspect the collector.",
            "FIXTURE MODE. Compiled fake ADB evidence, not a phone.",
          ],
          log_attribution: {
            strategy: "android_uid",
            application_id: "invalid.example.demostore",
            user_id: 0,
            uid: 10001,
            install_identity: "install-A",
            buffers: ["main", "system", "crash"],
            levels: "V",
            exclusions: ["Shared UID apps are excluded."],
          },
        },
      }}
    />,
  );
  assert.ok(screen.getByText("Unknown coverage error. Inspect the collector."));
  assert.ok(
    screen.getByText("FIXTURE MODE. Compiled fake ADB evidence, not a phone."),
  );
  assert.equal(
    screen.queryByText(
      "Device clock uncertainty is unknown. Time proximity does not establish causality.",
    ),
    null,
  );
  const coverageTrigger = screen.getByRole("button", {
    name: "Capture information",
  });
  assert.equal(coverageTrigger.getAttribute("aria-expanded"), "false");
  assert.equal(screen.queryByText(/android uid ·/), null);
  await user.click(coverageTrigger);
  assert.ok(screen.getByRole("dialog", { name: "Capture information" }));
  assert.equal(coverageTrigger.getAttribute("aria-expanded"), "true");
  assert.match(
    screen.getByText(/android uid ·/).textContent!,
    /android uid · invalid.example.demostore · buffers main, system, crash · levels V/,
  );
  assert.ok(screen.getByText("Shared UID apps are excluded."));
  assert.ok(
    screen.getByText(
      "Device clock uncertainty is unknown. Time proximity does not establish causality.",
    ),
  );
  await user.click(coverageTrigger);
  assert.equal(screen.queryByText(/android uid ·/), null);
  assert.equal(screen.queryByText("Shared UID apps are excluded."), null);
  cleanup();
  console.log(
    "Capture DOM checks passed. Explicit network Start, separate keyboard log consent, policy gates, active scope, marker form submit and closed Capture information visibility.",
  );
}
async function checkOwnedScope() {
  const owned = {
    ...session,
    device: { ...device, serial: "owned-phone", name: "Owned phone" },
    selection: { mode: "logs_and_network", consent: { whole_device: true } },
    log_scope: { mode: "app", application_id: "com.owned.app" },
    context: {
      title: "Owned investigation",
      notes: "Owned notes",
      steps: ["Owned step"],
    },
    limits: { ...limits, records: 99 },
  };
  const stopped = { ...owned, network: { state: "not_selected" } };
  const draft = {
    ...initialDraft,
    serial: "next-phone",
    selection: "logs",
    application_id: "com.next.app",
    context: { title: "Next investigation", notes: "", steps: [] },
  };
  const props = {
    devices: [{ ...device, serial: draft.serial }],
    draft,
    change: () => {},
    preferences,
    savePreferences: async () => {},
    refresh: async () => {},
    run,
    onSnapshot: () => {},
    onOperation: () => {},
    pending: "",
    operation: { state: "idle" },
  };
  for (const capture of [
    { snapshot: { phase: "recording", session: owned } },
    { snapshot: { phase: "recovery_required", session: owned } },
    { snapshot: { phase: "unverified_shutdown", session: owned } },
    {
      snapshot: { phase: "stopped", session: stopped },
      operation: {
        state: "restoring",
        id: "restore",
        kind: "save",
        message: "Restoring routing",
      },
    },
  ]) {
    render(<Capture {...props} {...capture} />);
    assert.equal(screen.getByLabelText("Device").value, owned.device.serial);
    assert.equal(screen.getByLabelText("Device").disabled, true);
    assert.equal(screen.getByLabelText("Sources").value, owned.selection.mode);
    assert.equal(screen.getByLabelText("Sources").disabled, true);
    assert.equal(
      screen.getByRole("combobox", { name: "Log app" }).value,
      "com.owned.app",
    );
    assert.equal(
      screen.getByRole("combobox", { name: "Log app" }).disabled,
      true,
    );
    await user.click(screen.getByRole("button", { name: "Advanced" }));
    assert.ok(
      screen.getByRole("dialog", { name: "Advanced capture settings" }),
    );
    assert.equal(screen.getByLabelText("Title").value, owned.context.title);
    assert.equal(screen.getByLabelText("Title").matches(":disabled"), true);
    assert.equal(screen.getByLabelText("Maximum records").value, "99");
    assert.equal(
      screen.getByLabelText("Maximum records").matches(":disabled"),
      true,
    );
    cleanup();
  }
  render(
    <Capture
      {...props}
      snapshot={{ phase: "stopped", session: stopped }}
      pending="Starting capture"
    />,
  );
  assert.equal(screen.getByLabelText("Device").value, draft.serial);
  assert.equal(screen.getByLabelText("Sources").value, draft.selection);
  assert.equal(
    screen.getByRole("combobox", { name: "Log app" }).value,
    draft.application_id,
  );
  assert.equal(screen.getByLabelText("Device").disabled, true);
  await user.click(screen.getByRole("button", { name: "Advanced" }));
  assert.equal(screen.getByLabelText("Title").value, draft.context.title);
  assert.equal(
    screen.getByLabelText("Maximum records").value,
    String(limits.records),
  );
  assert.equal(screen.getByLabelText("Title").matches(":disabled"), true);
  cleanup();
  console.log(
    "Owned scope DOM checks passed. Recording, recovery, shutdown verification and restoration show locked actual scope and limits. A pending new Start retains the submitted draft over an old safe session.",
  );
}

async function checkNotices() {
  const routine = [
    'Android log scope: App { application_id: "com.example.app" }. All available emitted levels from main, system, and crash buffers.',
    "Android log scope: WholeDevice. All available emitted levels from main, system, and crash buffers.",
    "Device clock uncertainty is unknown. Time proximity does not establish causality.",
    "Device logs were not selected. Network capture covers whole-device HTTP(S). HTTPS requires manual CA trust; certificate pinning can prevent observation.",
    "Device logs are limited to com.example.app. Network capture covers whole-device HTTP(S). HTTPS requires manual CA trust; certificate pinning can prevent observation.",
    "Device logs cover all apps and system processes. Network capture covers whole-device HTTP(S). HTTPS requires manual CA trust; certificate pinning can prevent observation.",
    "Timestamp proximity does not prove causation.",
    "Whole-device proxy evidence. Manual CA trust, certificate pinning, and omissions limit coverage.",
    "Device clock uncertainty unknown",
  ];
  const actionable = [
    "Unknown coverage error. Check the collector.",
    "Device clock calibration failed.",
    "Guardian interrupted. The proxy listener may have stopped before recovery. Original evidence is partial.",
    "FIXTURE MODE. Compiled fake ADB evidence, not a phone.",
    routine[0] + " Collector failed.",
    routine[3] + " Recovery required.",
  ];
  for (const message of routine)
    assert.equal(isRoutineNotice(message), true, message);
  for (const message of actionable)
    assert.equal(isRoutineNotice(message), false, message);
  product.catalog = async () => ({
    sessions: [],
    warnings: [routine[1], actionable[1]],
    scanned: 0,
    truncated: false,
  });
  const props = {
    open: async () => {},
    close: () => {},
    operation: { state: "idle" },
    onOperation: () => {},
    run,
    pending: "",
    revision: 0,
  };
  const mounted = render(
    <Library
      {...props}
      inspected={{ ...descriptor, warnings: [routine[0], actionable[0]] }}
      snapshot={idle}
    />,
  );
  assert.ok(screen.getByText(actionable[0]));
  assert.equal(screen.queryByText(routine[0]), null);
  await user.click(screen.getByRole("button", { name: "Session information" }));
  assert.ok(screen.getByText(routine[0]));
  await user.click(screen.getByRole("button", { name: "Session information" }));
  await user.click(screen.getByRole("button", { name: "Saved sessions" }));
  assert.ok(await screen.findByText(actionable[1]));
  assert.equal(screen.queryByText(routine[1]), null);
  await user.click(screen.getByRole("button", { name: "Catalog information" }));
  assert.ok(screen.getByText(routine[1]));
  mounted.rerender(
    <Library
      {...props}
      inspected={{ ...descriptor, warnings: [routine[0], actionable[0]] }}
      snapshot={idle}
      visible={false}
    />,
  );
  assert.equal(
    screen.queryByText(routine[1]),
    null,
    "Hidden retained Library cannot leak an information portal",
  );
  mounted.rerender(
    <Library
      {...props}
      inspected={{ ...descriptor, warnings: [routine[0], actionable[0]] }}
      snapshot={idle}
      visible
    />,
  );
  assert.equal(
    screen.queryByText(routine[1]),
    null,
    "Returning to Library does not reopen its old information portal",
  );
  mounted.rerender(
    <Library
      {...props}
      inspected={{
        ...descriptor,
        directory: session.directory,
        warnings: [routine[0], actionable[0]],
      }}
      snapshot={{ phase: "recording", session }}
    />,
  );
  assert.equal(
    screen.queryByRole("button", { name: "Session information" }),
    null,
    "Current session routine information has one owner",
  );
  assert.ok(screen.getByText(actionable[0]));
  cleanup();
  console.log(
    "Notice DOM checks passed. Complete producer forms move to information in session and catalog views, unknown clock/coverage failures and fixture provenance remain visible, and current-session explanations are not duplicated.",
  );
}

async function checkAppCombobox() {
  const starts: unknown[] = [];
  const apps = [
    {
      application_id: "com.example.shop",
      label: "Shop",
      user_id: 0,
      uid: 10001,
      shared_uid: false,
    },
    {
      application_id: "com.example.shared",
      label: "Shared app",
      user_id: 0,
      uid: 10002,
      shared_uid: true,
    },
  ];
  product.applications = async () => apps;
  api.start = async (request) => {
    starts.push(request);
    return idle;
  };
  function Harness() {
    const [draft, change] = useState({
      ...initialDraft,
      serial: "phone-a",
      selection: "logs",
      log_mode: "app",
    });
    return (
      <Capture
        snapshot={idle}
        operation={{ state: "idle" }}
        devices={[device]}
        draft={draft}
        change={change}
        preferences={preferences}
        savePreferences={async () => {}}
        refresh={async () => {}}
        run={run}
        pending=""
        onSnapshot={() => {}}
        onOperation={() => {}}
      />
    );
  }
  render(<Harness />);
  const start = screen.getByRole("button", { name: "Start recording" });
  assert.equal(start.disabled, true);
  assert.equal(screen.queryByLabelText("Or enter an application ID"), null);
  await user.click(screen.getByRole("button", { name: "Refresh apps" }));
  const input = screen.getByRole("combobox", { name: "Log app" });
  await user.type(input, "Shop");
  await user.keyboard("{ArrowDown}{Enter}");
  await waitFor(() => assert.match(input.value, /com\.example\.shop/));
  assert.equal(start.disabled, false);
  await user.click(start);
  assert.deepEqual(starts[0].log_scope, {
    mode: "app",
    application_id: "com.example.shop",
  });
  await user.click(screen.getByRole("button", { name: "Clear selection" }));
  assert.equal(start.disabled, true);
  await user.type(input, "Shared");
  const shared = await screen.findByRole("option", { name: /Shared app/ });
  assert.equal(shared.getAttribute("aria-disabled"), "true");
  await user.keyboard("{Escape}");
  await user.clear(input);
  await user.type(input, "com.example.manual");
  await user.click(
    await screen.findByRole("option", { name: /Use application ID/ }),
  );
  assert.equal(start.disabled, false);
  await user.click(start);
  assert.deepEqual(starts[1].log_scope, {
    mode: "app",
    application_id: "com.example.manual",
  });
  cleanup();
  console.log(
    "App combobox DOM checks passed. Keyboard selection searches labels and IDs, shared UIDs stay disabled, clear resets admission and manual IDs use the same control.",
  );
}
async function checkTimeline() {
  localStorage.clear();
  const queries: unknown[] = [];
  const requests = new Map<string, ReturnType<typeof deferred>>();
  product.query = async (query) => {
    queries.push(query);
    return query.cursor ? { ...page, rows: [], next_cursor: null } : page;
  };
  product.detail = (id) => {
    const pending = deferred();
    requests.set(id, pending);
    return pending.promise;
  };
  product.detailChunk = async () => emptyChunk;
  const mounted = render(<Timeline inspected={descriptor} />);
  await screen.findByRole("button", {
    name: /GET https:\/\/example.invalid\/one/,
  });
  await user.click(screen.getByRole("button", { name: "Older page" }));
  await waitFor(() => assert.equal(queries.at(-1).cursor, "older"));
  await user.click(screen.getByRole("checkbox", { name: "Follow latest" }));
  await waitFor(() => assert.equal(queries.at(-1).cursor, null));
  const first = await screen.findByRole("button", {
    name: /GET https:\/\/example.invalid\/one/,
  });
  await user.click(first);
  const inspector = screen.getByRole("dialog", { name: "Event details" });
  assert.equal(inspector.getAttribute("aria-modal"), "false");
  assert.match(inspector.textContent!, /Reading event/);
  assert.equal(
    document.activeElement,
    within(inspector).getByRole("button", { name: "Close event details" }),
  );
  await user.keyboard("{Escape}");
  assert.equal(screen.queryByRole("dialog"), null);
  assert.equal(document.activeElement, first);
  await act(async () => requests.get("one")!.resolve(detail("one")));
  assert.equal(screen.queryByRole("dialog"), null);
  await user.click(first);
  const old = requests.get("one")!;
  await user.click(
    screen.getByRole("button", { name: /GET https:\/\/example.invalid\/two/ }),
  );
  await act(async () => requests.get("two")!.resolve(detail("two")));
  await screen.findByText("two", { selector: "dd" });
  await act(async () => old.resolve(detail("one")));
  assert.equal(screen.queryByText("one", { selector: "dd" }), null);
  await user.click(screen.getByRole("button", { name: "Older page" }));
  await screen.findByText("Selected outside this page");
  await user.click(screen.getByRole("button", { name: "Close event details" }));
  assert.equal(
    document.activeElement,
    screen.getByRole("heading", { name: "Timeline" }),
  );
  await user.click(screen.getByRole("checkbox", { name: "Follow latest" }));
  await user.click(
    await screen.findByRole("button", {
      name: /GET https:\/\/example.invalid\/one/,
    }),
  );
  const crossSession = requests.get("one")!;
  mounted.rerender(
    <Timeline
      inspected={{ ...descriptor, id: "C", directory: "/evidence/C" }}
    />,
  );
  assert.equal(screen.queryByRole("dialog"), null);
  await act(async () => crossSession.resolve(detail("one")));
  assert.equal(screen.queryByRole("dialog"), null);
  await user.click(
    await screen.findByRole("button", {
      name: /GET https:\/\/example.invalid\/one/,
    }),
  );
  await act(async () =>
    requests.get("one")!.reject(new Error("detail read failed")),
  );
  assert.match(screen.getByRole("alert").textContent!, /detail read failed/);
  cleanup();
  console.log(
    "Timeline DOM checks passed. Backend cursors, follow, modeless focus/Escape, off-page selection, stale and cross-session rejection, and local errors.",
  );
}
async function checkDetails() {
  const bodyCalls: unknown[] = [];
  const metadataCalls: unknown[] = [];
  product.detailChunk = async (...args) => {
    metadataCalls.push(args);
    return emptyChunk;
  };
  product.body = async (...args) => {
    bodyCalls.push(args);
    return {
      chunk: {
        offset: args[2],
        total_bytes: 65539,
        data_base64: args[3] ? "ZGVjb2RlZA==" : "AP9B",
        next_offset: args[2] === 0 ? 65536 : null,
      },
    };
  };
  const binary = {
    ...detail("binary"),
    bodies: [
      {
        side: "response",
        available: true,
        bytes: 65539,
        text: false,
        content_encoding: "gzip",
        decoded_available: true,
        reason: null,
        decoded_reason: null,
      },
    ],
  };
  const mounted = render(
    <Details key="binary" detail={binary} offPage={false} wrap />,
  );
  await user.click(screen.getByRole("button", { name: /Response/ }));
  await waitFor(() =>
    assert.match(
      screen.getByText(/00000000/, { selector: "pre" }).textContent!,
      /00 ff 41/,
    ),
  );
  await user.click(screen.getByRole("button", { name: "Next 64 KiB" }));
  await waitFor(() =>
    assert.deepEqual(bodyCalls.at(-1), ["binary", "response", 65536, false]),
  );
  await user.click(screen.getByRole("button", { name: "Decode gzip" }));
  await screen.findByText("decoded", { selector: "pre" });
  assert.deepEqual(bodyCalls.at(-1), ["binary", "response", 0, true]);
  const reveal = deferred();
  product.reveal = async () => reveal.promise;
  await user.click(screen.getByRole("button", { name: "Reveal raw source" }));
  assert.equal(
    screen.getByRole("button", { name: "Revealing raw source…" }).disabled,
    true,
  );
  await act(async () => reveal.reject(new Error("reveal failed")));
  assert.match(screen.getByRole("alert").textContent!, /reveal failed/);
  mounted.rerender(
    <Details
      key="replacement"
      detail={detail("replacement")}
      offPage={false}
      wrap
    />,
  );
  await waitFor(() =>
    assert.deepEqual(metadataCalls.at(-1), ["replacement", 0]),
  );
  assert.equal(screen.queryByRole("alert"), null);
  const abandoned = deferred();
  product.detailChunk = () => abandoned.promise;
  mounted.rerender(
    <Details
      key="abandoned"
      detail={detail("abandoned")}
      offPage={false}
      wrap
    />,
  );
  mounted.unmount();
  await act(async () =>
    abandoned.resolve({ ...emptyChunk, data_base64: "YWJj" }),
  );
  assert.equal(screen.queryByText("abc"), null);
  console.log(
    "Detail DOM checks passed. 64 KiB binary paging, decoded content, local raw errors, mount reset and disposed reads.",
  );
}
async function checkModal() {
  let closes = 0;
  const outside = document.createElement("button");
  outside.textContent = "Outside control";
  document.body.append(outside);
  outside.focus();
  const mounted = render(
    <Dialog
      title="Device setup"
      locked
      close={() => {
        closes++;
      }}
    >
      <button>First setup control</button>
      <button>Last setup control</button>
    </Dialog>,
  );
  const modal = screen.getByRole("dialog", { name: "Device setup" });
  assert.equal(modal.getAttribute("aria-modal"), "true");
  assert.equal(
    screen.getByRole("button", { name: "Close Device setup" }).disabled,
    true,
  );
  await user.keyboard("{Escape}");
  fireEvent.pointerDown(
    document.querySelector('[data-slot="dialog-overlay"]')!,
    { button: 0 },
  );
  await user.click(screen.getByRole("button", { name: "Close Device setup" }));
  assert.equal(closes, 0);
  screen.getByRole("button", { name: "Last setup control" }).focus();
  await user.tab();
  assert.equal(modal.contains(document.activeElement), true);
  mounted.rerender(
    <Dialog
      title="Device setup"
      close={() => {
        closes++;
      }}
    >
      <button>First setup control</button>
    </Dialog>,
  );
  await user.keyboard("{Escape}");
  assert.equal(closes, 1);
  await user.click(screen.getByRole("button", { name: "Close Device setup" }));
  assert.equal(closes, 2);
  fireEvent.pointerDown(
    document.querySelector('[data-slot="dialog-overlay"]')!,
    { button: 0 },
  );
  await waitFor(() => assert.equal(closes, 3));
  mounted.unmount();
  await waitFor(() => assert.equal(document.activeElement, outside));
  outside.remove();
  console.log(
    "Modal DOM checks passed. Locked Close/Escape/outside dismissal, keyboard containment and focus restoration.",
  );
}
async function checkSettingsHandoffs() {
  const saves: unknown[] = [];
  const props = {
    preferences,
    save: async (value) => {
      saves.push(value);
    },
    idle: false,
    editable: true,
    run,
    pending: "",
    setup: () => {},
    doctor: () => {},
  };
  const settings = render(<Settings {...props} />);
  assert.equal(
    screen.getByRole("button", { name: "Save settings" }).disabled,
    false,
  );
  assert.equal(
    screen.getByRole("button", { name: "Check for updates" }).disabled,
    true,
  );
  const override = screen.getByLabelText("ADB executable");
  await user.type(override, "relative-adb");
  assert.equal(
    screen.getByRole("button", { name: "Save settings" }).disabled,
    true,
  );
  await user.clear(override);
  await user.type(override, "/tools/adb");
  await user.click(screen.getByRole("button", { name: "Save settings" }));
  assert.equal(saves.length, 1);
  assert.equal(saves[0].adb_override, "/tools/adb");
  await screen.findByRole("status");
  settings.rerender(<Settings {...props} editable={false} />);
  assert.equal(
    screen.getByRole("button", { name: "Save settings" }).disabled,
    true,
  );
  assert.equal(
    screen.getByLabelText("Session folder").matches(":disabled"),
    true,
  );
  cleanup();
  const notices = [
    {
      id: 1,
      request: {
        kind: "device",
        serial: "phone-requested",
        applicationId: "com.example.requested",
      },
      state: "received",
      message: "Launch link received. No recording has started.",
    },
    {
      id: 2,
      request: { kind: "session", id: "saved-id" },
      state: "received",
      message: "Open saved evidence.",
    },
    {
      id: 3,
      request: { kind: "rejected", message: "Unsupported scheme" },
      state: "received",
      message: "Unsupported scheme",
    },
  ];
  const applied: unknown[] = [];
  const dismissed: unknown[] = [];
  const handoffs = render(
    <HandoffNotices
      notices={notices}
      available={false}
      apply={async (notice) => {
        applied.push(notice);
      }}
      dismiss={(id) => {
        dismissed.push(id);
      }}
    />,
  );
  assert.equal(screen.getAllByRole("button", { name: "Apply link" }).length, 2);
  assert.ok(
    screen
      .getAllByRole("button", { name: "Apply link" })
      .every((button) => button.disabled),
  );
  assert.ok(screen.getByText("phone-requested · com.example.requested"));
  assert.ok(screen.getByText("Session saved-id"));
  assert.ok(screen.getByText("Unsupported scheme"));
  handoffs.rerender(
    <HandoffNotices
      notices={notices}
      available
      apply={async (notice) => {
        applied.push(notice);
      }}
      dismiss={(id) => {
        dismissed.push(id);
      }}
    />,
  );
  await user.click(screen.getAllByRole("button", { name: "Apply link" })[0]);
  await user.click(screen.getAllByRole("button", { name: "Dismiss" })[1]);
  assert.deepEqual(applied, [notices[0]]);
  assert.deepEqual(dismissed, [2]);
  cleanup();
  console.log(
    "Settings and handoff DOM checks passed. Preference repair stays available, paths validate before explicit Save, updates stay idle-only, and retained launch links require an available Apply action.",
  );
}
async function checkViewPreservation() {
  localStorage.clear();
  localStorage.setItem(
    "android.capture.setup.v1",
    JSON.stringify({ deferred: true, certificate: null, check: null }),
  );
  localStorage.setItem(
    "android.capture.draft.v1",
    JSON.stringify({ ...initialDraft, serial: "phone-a" }),
  );
  let work = { state: "idle" };
  api.snapshot = async () => idle;
  api.operation = async () => work;
  api.discover = async () => [device];
  api.preferences = async () => preferences;
  api.pendingHandoff = async () => null;
  api.identity = async () => {
    throw new Error("No setup identity in DOM fixture");
  };
  product.catalog = async () => ({
    sessions: [descriptor],
    warnings: [],
    scanned: 1,
    truncated: false,
  });
  product.open = async () => descriptor;
  product.chooseFolder = async () => descriptor.directory;
  product.query = async () => page;
  product.detail = async () => detail("one");
  product.detailChunk = async () => emptyChunk;
  render(<App />);
  await waitFor(() =>
    assert.equal(
      screen.getByRole("button", { name: "Start recording" }).disabled,
      false,
    ),
  );
  await user.click(screen.getByRole("button", { name: "Open folder" }));
  await user.selectOptions(screen.getByLabelText("Source filter"), "network");
  await user.click(
    await screen.findByRole("button", {
      name: /GET https:\/\/example.invalid\/one/,
    }),
  );
  const inspector = await screen.findByRole("dialog", {
    name: "Event details",
  });
  const evidence = screen.getByRole("region", { name: "Capture evidence" });
  const filter = screen.getByLabelText("Source filter");
  await user.click(screen.getByRole("button", { name: "Settings" }));
  assert.equal(
    screen
      .getByRole("button", { name: "Settings" })
      .getAttribute("aria-pressed"),
    "true",
  );
  assert.equal(evidence.hidden, true);
  assert.equal(evidence.hasAttribute("inert"), true);
  assert.equal(getComputedStyle(evidence).display, "none");
  assert.equal(screen.queryByRole("dialog", { name: "Event details" }), null);
  assert.equal(evidence.contains(document.activeElement), false);
  const folder = screen.getByLabelText("Session folder");
  await user.clear(folder);
  await user.type(folder, "/unsaved-evidence");
  await user.click(screen.getByRole("button", { name: "Host Doctor" }));
  assert.ok(screen.getByRole("dialog", { name: "Host Doctor" }));
  await user.click(
    screen.getByRole("button", { name: "Storage and tool settings" }),
  );
  assert.equal(screen.queryByRole("dialog"), null);
  assert.equal(screen.getByLabelText("Session folder"), folder);
  assert.equal(folder.value, "/unsaved-evidence");
  await user.click(screen.getByRole("button", { name: "Device setup" }));
  await user.click(screen.getByRole("button", { name: "Close Device setup" }));
  assert.ok(screen.getByRole("heading", { name: "Settings" }));
  assert.equal(folder.value, "/unsaved-evidence");
  await user.click(screen.getByRole("button", { name: "Capture" }));
  assert.equal(screen.getByLabelText("Source filter"), filter);
  assert.equal(filter.value, "network");
  assert.equal(
    screen.getByRole("dialog", { name: "Event details" }),
    inspector,
  );
  assert.equal(screen.queryByRole("textbox", { name: "Session folder" }), null);
  await user.click(screen.getByRole("button", { name: "Settings" }));
  assert.equal(screen.getByLabelText("Session folder"), folder);
  assert.equal(folder.value, "/unsaved-evidence");
  await user.click(screen.getByRole("button", { name: "Device setup" }));
  work = {
    state: "restoring",
    id: "restore-test",
    kind: "connection_check",
    message: "Restoring fixture routing",
  };
  await waitFor(
    () =>
      assert.equal(
        screen.getByRole("button", { name: "Close Device setup" }).disabled,
        true,
      ),
    { timeout: 2500 },
  );
  await user.keyboard("{Escape}");
  assert.ok(screen.getByRole("dialog", { name: "Device setup" }));
  cleanup();
  console.log(
    "View lifetime DOM checks passed. Timeline filters and modeless inspector survive hidden/inert view switches; unsaved Settings survive Capture, Doctor and Setup; restoring routing locks Setup dismissal.",
  );
}

async function checkWholeApp() {
  for (const read of ["detail", "metadata", "body", "raw"]) {
    localStorage.clear();
    localStorage.setItem(
      "android.capture.setup.v1",
      JSON.stringify({ deferred: true, certificate: null, check: null }),
    );
    localStorage.setItem(
      "android.capture.draft.v1",
      JSON.stringify({ ...initialDraft, serial: "phone-a" }),
    );
    let currentSnapshot = { phase: "recording", session };
    const held = deferred();
    const stopCalls: unknown[] = [];
    const saveCalls: unknown[] = [];
    api.snapshot = async () => currentSnapshot;
    api.operation = async () => ({ state: "idle" });
    api.discover = async () => [device];
    api.preferences = async () => preferences;
    api.pendingHandoff = async () => null;
    api.stop = async (...args) => {
      stopCalls.push(args);
      currentSnapshot = {
        phase: "stopped",
        session: {
          ...session,
          network: {
            state: "stopped",
            detail: {
              coverage,
              routing_restored: true,
              partial: false,
              forced: false,
              interrupted: false,
              reasons: [],
            },
          },
        },
      };
      return currentSnapshot;
    };
    api.save = async (directory) => {
      saveCalls.push(directory);
      return {
        state: "saving",
        id: "save-A",
        stage: "indexing",
        completed: 0,
        total: null,
        cancel_allowed: true,
      };
    };
    product.catalog = async () => ({
      sessions: [descriptor],
      warnings: [],
      scanned: 1,
      truncated: false,
    });
    product.open = async (directory) => ({
      ...descriptor,
      directory,
      id: directory.endsWith("A") ? "A" : "B",
    });
    product.chooseFolder = async () => descriptor.directory;
    product.query = async () => page;
    const withBody = {
      ...detail("one"),
      bodies: [
        {
          side: "response",
          available: true,
          bytes: 3,
          text: true,
          content_encoding: null,
          decoded_available: false,
          reason: null,
          decoded_reason: null,
        },
      ],
    };
    product.detail = async () => (read === "detail" ? held.promise : withBody);
    product.detailChunk = async () =>
      read === "metadata" ? held.promise : emptyChunk;
    product.body = async () =>
      read === "body" ? held.promise : { chunk: emptyChunk };
    product.reveal = async () => (read === "raw" ? held.promise : undefined);
    render(<App />);
    const stop = await screen.findByRole(
      "button",
      { name: "More recording actions" },
      { timeout: 2500 },
    );
    await waitFor(() => assert.equal(stop.disabled, false));
    await user.click(screen.getByRole("button", { name: "Open folder" }));
    await user.click(
      await screen.findByRole("button", {
        name: /GET https:\/\/example.invalid\/one/,
      }),
    );
    if (read === "body")
      await user.click(await screen.findByRole("button", { name: /Response/ }));
    if (read === "raw")
      await user.click(
        await screen.findByRole("button", { name: "Reveal raw source" }),
      );
    assert.equal(stop.disabled, false);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    assert.ok(screen.getByRole("heading", { name: "Settings" }));
    if (read === "detail") {
      await user.click(screen.getByRole("button", { name: "Stop & Save" }));
      await waitFor(() => assert.deepEqual(saveCalls, [null]));
    } else {
      await user.click(stop);
      await user.click(
        await screen.findByRole("menuitem", { name: "Stop without archive" }),
      );
      await waitFor(() => assert.deepEqual(stopCalls, [[]]));
    }
    cleanup();
    await act(async () =>
      held.resolve(
        read === "detail"
          ? withBody
          : read === "body"
            ? { chunk: emptyChunk }
            : read === "raw"
              ? undefined
              : emptyChunk,
      ),
    );
  }
  const reveals: unknown[] = [];
  product.reveal = async (...args) => {
    reveals.push(args);
  };
  render(
    <OperationStatus
      operation={{
        state: "finished",
        id: "save-A",
        result: {
          kind: "save",
          archive: "/evidence/A/capture-package.zip",
          directory: "/evidence/A",
        },
      }}
      change={() => {}}
      run={run}
      pending=""
      recover={async () => {}}
    />,
  );
  await user.click(screen.getByRole("button", { name: "Reveal archive" }));
  assert.deepEqual(reveals, [["archive", "save-A"]]);
  cleanup();
  console.log(
    "Whole-App DOM checks passed. Delayed detail, metadata, body and raw reads leave active Stop/Stop & Save available while a different saved folder is inspected. Save and archive reveal retain active operation ownership.",
  );
}
async function checks() {
  try {
    const checks = {
      progress: checkProgress,
      scope: checkCaptureScope,
      capture: checkCapture,
      apps: checkAppCombobox,
      ownership: checkOwnedScope,
      notices: checkNotices,
      views: checkViewPreservation,
      timeline: checkTimeline,
      details: checkDetails,
      modal: checkModal,
      settings: checkSettingsHandoffs,
      app: checkWholeApp,
    };
    const requested = process.env.ANDROID_UI_CHECK;
    assert.ok(!requested || requested in checks, "Unknown Android UI check");
    for (const [name, check] of Object.entries(checks)) {
      if (!requested || requested === name) await check();
    }
  } finally {
    cleanup();
  }
}
export const complete = checks();
