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
  const actionHost = document.createElement("div");
  document.body.append(actionHost);
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
        setup={() => {}}
        actionHost={actionHost}
      />
    );
  }
  const mounted = render(<Harness />);
  const captureDevice = screen.getByLabelText("Capture on");
  await user.selectOptions(captureDevice, unavailable.serial);
  const scope = screen.getByRole("region", { name: "Capture scope" });
  const trigger = within(scope).getByRole("button", {
    expanded: true,
  });
  assert.equal(
    trigger.disabled,
    false,
    "Incomplete capture scope can be collapsed",
  );
  await user.click(trigger);
  assert.equal(trigger.getAttribute("aria-expanded"), "false");
  assert.equal(screen.queryByLabelText("Capture on"), null);
  assert.match(scope.textContent!, /Complete capture scope/);
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    true,
  );
  mounted.rerender(<Harness devices={[]} />);
  assert.equal(trigger.getAttribute("aria-expanded"), "false");
  assert.equal(screen.queryByLabelText("Capture on"), null);
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    true,
  );
  await user.click(trigger);
  assert.equal(screen.getByLabelText("Capture on").value, unavailable.serial);
  assert.equal(starts.length, 0);
  cleanup();
  const savedDraft = { ...initialDraft, serial: device.serial };
  const restored = render(
    <Harness devices={[]} draftValue={savedDraft} loading />,
  );
  const restoredTrigger = within(
    screen.getByRole("region", { name: "Capture scope" }),
  ).getByRole("button", { name: /capture scope/ });
  assert.equal(
    restoredTrigger.getAttribute("aria-expanded"),
    "false",
    "Loading discovery does not expand an existing capture draft",
  );
  restored.rerender(<Harness draftValue={savedDraft} />);
  assert.equal(restoredTrigger.getAttribute("aria-expanded"), "false");
  assert.equal(
    screen.getByRole("button", { name: "Start recording" }).disabled,
    false,
  );
  assert.equal(starts.length, 0);
  cleanup();
  actionHost.remove();
  console.log(
    "Capture scope DOM checks passed. Missing setup opens guidance, an unavailable device can be collapsed and reopened, refresh keeps deliberate collapse, and Start stays disabled without autostart.",
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
  const actionHost = document.createElement("div");
  document.body.append(actionHost);
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
        setup={() => {}}
        actionHost={actionHost}
      />
    );
  }
  let mounted = render(<Harness />);
  assert.equal(starts.length, 0);
  const start = screen.getByRole("button", { name: "Start recording" });
  assert.equal(start.disabled, false);
  const notice = document.getElementById(
    start.getAttribute("aria-describedby")!,
  );
  assert.match(
    notice!.textContent!,
    /whole-device HTTP\(S\).*proxy routing.*manual CA trust/,
  );
  assert.equal(actionHost.contains(notice), true);
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
  await user.click(screen.getByRole("button", { name: "Edit capture scope" }));
  assert.equal(
    screen.getByText("Task context (optional)").parentElement!.open,
    false,
  );
  await user.selectOptions(
    screen.getByLabelText("Sources"),
    "logs_and_network",
  );
  await user.selectOptions(screen.getByLabelText("Log scope"), "whole_device");
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
  assert.match(
    screen.getByRole("region", { name: "Capture scope" }).textContent!,
    /SourcesNetwork/,
  );
  assert.ok(screen.getByText(/Start records whole-device HTTP/));
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
          selection: { mode: "logs_and_network", consent: { whole_device: true } },
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
  const coverageTrigger = screen.getByRole("button", { name: "Log coverage" });
  assert.equal(coverageTrigger.getAttribute("aria-expanded"), "false");
  assert.equal(screen.queryByText(/android uid ·/), null);
  await user.click(coverageTrigger);
  assert.equal(coverageTrigger.getAttribute("aria-expanded"), "true");
  assert.match(
    screen.getByText(/android uid ·/).textContent!,
    /android uid · invalid.example.demostore · buffers main, system, crash · levels V/,
  );
  assert.ok(screen.getByText("Shared UID apps are excluded."));
  await user.click(coverageTrigger);
  assert.equal(screen.queryByText(/android uid ·/), null);
  assert.equal(screen.queryByText("Shared UID apps are excluded."), null);
  cleanup();
  actionHost.remove();
  console.log(
    "Capture DOM checks passed. Explicit network Start, separate keyboard log consent, policy gates, active scope, marker form submit and closed Log coverage visibility.",
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
      { name: "Stop without archive" },
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
    if (read === "detail") {
      await user.click(screen.getByRole("button", { name: "Stop & Save" }));
      await waitFor(() => assert.deepEqual(saveCalls, [null]));
    } else {
      await user.click(stop);
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
