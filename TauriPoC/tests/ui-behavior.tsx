import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import {
  captureSafe,
  operationBusy,
  operationSchema,
  snapshotSchema,
  type CheckResult,
  type Preferences,
} from "../src/api";
import {
  contextSchema,
  dataChunkSchema,
  decodeChunk,
  networkPolicySchema,
} from "../src/product";
import { checkFreshness, initialDraft, timelineFence } from "../src/state";
import { hexChunk } from "../src/Details";
import { Capture } from "../src/Capture";
import { checkHandoffBehavior } from "./handoff-behavior";

const now = Date.parse("2026-10-23T12:00:00Z");
const check: CheckResult = {
  kind: "connection_check",
  serial: "phone-a",
  directory: "/capture/check",
  url: "https://capture.invalid/check/nonce",
  certificate_fingerprint: "abc",
  https_observed: true,
  phone_origin_confirmed: true,
  routing_restored: true,
  verified: true,
  checked_at: "2026-10-23T11:59:00Z",
  expires_at: "2026-10-23T12:05:00Z",
  message: "Verified",
};
const identity = {
  certificate_fingerprint: "abc",
  certificate_path: "/capture/public.cer",
  checked_at: "2026-10-23T12:00:00Z",
};
assert.equal(checkFreshness(check, "phone-a", identity, now), "");
for (const invalid of [
  { ...check, verified: false },
  { ...check, https_observed: false },
  { ...check, phone_origin_confirmed: false },
  { ...check, routing_restored: false },
  { ...check, expires_at: "invalid" },
  { ...check, expires_at: "2026-10-23T11:59:00Z" },
])
  assert.notEqual(checkFreshness(invalid, "phone-a", identity, now), "");
assert.match(
  checkFreshness(check, "phone-b", identity, now),
  /different device/,
);
assert.match(
  checkFreshness(
    check,
    "phone-a",
    { ...identity, certificate_fingerprint: "rotated" },
    now,
  ),
  /changed/,
);
assert.notEqual(checkFreshness(check, "phone-a", undefined, now), "");
assert.deepEqual(timelineFence("pause", 90, { after: 12, through: null }), {
  after: 12,
  through: 90,
});
assert.deepEqual(timelineFence("clear", 90, { after: 12, through: 70 }), {
  after: 90,
  through: null,
});
assert.deepEqual(timelineFence("full", 90, { after: 12, through: 70 }), {
  after: null,
  through: null,
});
assert.equal(
  contextSchema.safeParse({ title: "a".repeat(512), notes: "", steps: [] })
    .success,
  true,
);
assert.equal(
  contextSchema.safeParse({ title: "界".repeat(171), notes: "", steps: [] })
    .success,
  false,
);
assert.equal(
  contextSchema.safeParse({ title: "", notes: "", steps: Array(101).fill("") })
    .success,
  false,
);
assert.equal(
  contextSchema.safeParse({ title: "", notes: "", steps: ["a".repeat(4097)] })
    .success,
  false,
);
const policy: Preferences["network_policy"] = {
  max_flows: 10000,
  max_body_bytes: 33554432,
  max_header_bytes: 131072,
  max_total_bytes: 1073741824,
  duration_ms: 3600000,
};
assert.equal(networkPolicySchema.safeParse(policy).success, true);
assert.equal(
  networkPolicySchema.safeParse({ ...policy, max_header_bytes: 131073 })
    .success,
  false,
);
assert.equal(
  networkPolicySchema.safeParse({ ...policy, duration_ms: 86400001 }).success,
  false,
);
assert.equal(
  operationBusy({
    state: "failed",
    id: "save",
    kind: "save",
    message: "Restore needed",
    recovery_required: true,
    directory: null,
  }),
  true,
);
assert.equal(
  operationBusy({
    state: "cancelled",
    id: "save",
    kind: "save",
    message: "Cancelled",
  }),
  false,
);
assert.equal(
  operationSchema.safeParse({
    state: "saving",
    id: "a",
    stage: "indexing",
    completed: 5,
    total: null,
    cancel_allowed: true,
  }).success,
  true,
);
assert.equal(
  operationSchema.safeParse({
    state: "saving",
    id: "a",
    stage: "indexing",
    completed: 5,
    cancelAllowed: true,
  }).success,
  false,
);
const limits = { bytes: 32000000, records: 100000, duration_ms: 120000 };
const idle = snapshotSchema.parse({
  phase: "idle",
  session: { fixture: false, limits },
});
assert.equal(captureSafe(idle), true);
assert.equal(captureSafe(undefined), false);
const session = {
  id: "capture",
  device: { serial: "phone-a", name: "Phone A", state: "ready" },
  directory: "/capture/session",
  started_at: "2026-10-23T12:00:00Z",
  limits,
  fixture: false,
  bytes: 0,
  logs: 0,
  markers: 0,
  receipt: null,
  archive: null,
  warnings: [],
  descriptor_version: 1,
  selection: { mode: "logs" },
  log_scope: { mode: "app", application_id: "com.example.app" },
  network_policy: policy,
  context: { title: "", notes: "", steps: [] },
  log_attribution: null,
  network: { state: "not_selected" },
  events: ["large legacy snapshot ignored"],
};
const stopped = snapshotSchema.parse({ phase: "stopped", session });
assert.ok(stopped.phase === "stopped");
assert.equal(captureSafe(stopped), true);
assert.equal("events" in stopped.session, false);
assert.equal(
  captureSafe(snapshotSchema.parse({ phase: "recording", session })),
  false,
);
assert.equal(
  captureSafe(snapshotSchema.parse({ phase: "recovery_required", session })),
  false,
);
const completion = {
  coverage: {
    decoded_http_events: 1,
    tls_errors: 0,
    omitted_flows: 0,
    unavailable_bodies: 0,
    admission_closed: false,
    public_certificate: null,
    synthetic_check_url: null,
  },
  routing_restored: false,
  partial: true,
  forced: false,
  interrupted: false,
  reasons: [],
};
assert.equal(
  captureSafe(
    snapshotSchema.parse({
      phase: "stopped",
      session: {
        ...session,
        network: { state: "stopped", detail: completion },
      },
    }),
  ),
  false,
);
assert.equal(
  dataChunkSchema.safeParse({
    offset: 0,
    total_bytes: 2,
    data_base64: "!!!",
    next_offset: null,
  }).success,
  false,
);
assert.deepEqual(Array.from(decodeChunk("AP9B")), [0, 255, 65]);
assert.match(
  hexChunk(new Uint8Array([0, 255, 65]), 65536),
  /00010000  00 ff 41/,
);
const markup = renderToStaticMarkup(
  <Capture
    snapshot={idle}
    operation={{ state: "idle" }}
    devices={[stopped.session.device]}
    draft={{ ...initialDraft, serial: "phone-a" }}
    change={() => {}}
    preferences={{
      output_root: "/capture",
      limits,
      network_policy: policy,
      adb_override: null,
      mitmdump_override: null,
    }}
    savePreferences={async () => {}}
    refresh={async () => {}}
    run={async (_label, action) => {
      await action();
    }}
    pending=""
    onSnapshot={() => {}}
    onOperation={() => {}}
    setup={() => {}}
  />,
);
assert.match(
  markup,
  /<option value="network" selected="">Network only<\/option>/,
);
assert.match(markup, /whole-device network capture/);
assert.match(markup, /disabled="">Start recording/);
assert.doesNotMatch(markup, /iPhone|Simulator|Homebrew/);
const activeMarkup = renderToStaticMarkup(
  <Capture
    snapshot={snapshotSchema.parse({ phase: "recording", session: { ...session, bytes: 1024, selection: { mode: "logs_and_network", consent: { whole_device: true } }, log_scope: { mode: "whole_device" }, network: { state: "recording", detail: completion.coverage } } })}
    operation={{ state: "idle" }}
    devices={[stopped.session.device]}
    draft={{ ...initialDraft, serial: "phone-a", selection: "logs_and_network", log_mode: "whole_device" }}
    change={() => {}}
    preferences={{ output_root: "/capture", limits, network_policy: policy, adb_override: null, mitmdump_override: null }}
    savePreferences={async () => {}}
    refresh={async () => {}}
    run={async (_label, action) => { await action(); }}
    pending=""
    onSnapshot={() => {}}
    onOperation={() => {}}
    setup={() => {}}
  />,
);
assert.equal((activeMarkup.match(/checked=""/g) ?? []).length, 2);
assert.match(activeMarkup, /raw log data/);
assert.doesNotMatch(activeMarkup, />0 logs</);
void checkHandoffBehavior().then(
  () => console.log("Android UI behavior and launch-link checks passed."),
  (cause: unknown) => {
    console.error(cause);
    process.exitCode = 1;
  },
);
