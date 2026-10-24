import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import {
  handoffSchema,
  preferencesEditable,
  snapshotSchema,
  type Handoff,
  type Operation,
  type Preferences,
} from "../src/api";
import {
  consumeHandoff,
  handoffAvailable,
  HandoffNotices,
  resolveHandoff,
  type HandoffNotice,
} from "../src/Handoff";
import { Settings } from "../src/Settings";
import { Capture } from "../src/Capture";
import { initialDraft } from "../src/state";
import type { SessionDescriptor } from "../src/product";

export async function checkHandoffBehavior() {
  const limits = { bytes: 32000000, records: 100000, duration_ms: 120000 };
  const preferences: Preferences = {
    output_root: "/capture",
    limits,
    network_policy: {
      max_flows: 10000,
      max_body_bytes: 33554432,
      max_header_bytes: 131072,
      max_total_bytes: 1073741824,
      duration_ms: 3600000,
    },
    adb_override: "/unavailable/adb",
    mitmdump_override: null,
  };
  const idle = snapshotSchema.parse({
    phase: "idle",
    session: { fixture: false, limits },
  });
  const noOperation: Operation = { state: "idle" };
  const saving: Operation = {
    state: "saving",
    id: "save",
    stage: "indexing",
    completed: 1,
    total: null,
    cancel_allowed: true,
  };
  const recovery: Operation = {
    state: "failed",
    kind: "certificate",
    id: "recover",
    message: "Restore needed",
    recovery_required: true,
    directory: null,
  };
  const device = {
    serial: "phone-requested",
    name: "Requested phone",
    state: "ready",
  } satisfies SessionDescriptor["device"];
  const saved: SessionDescriptor = {
    version: 1,
    id: "saved-id",
    directory: "/capture/catalog-entry",
    device,
    started_at: "2026-10-23T12:00:00Z",
    legacy: false,
    metadata: null,
    warnings: [],
  };
  const catalog = {
    sessions: [saved],
    scanned: 1,
    truncated: false,
    warnings: [],
  };
  const app = {
    application_id: "com.example.requested",
    label: "Requested app",
    user_id: 0,
    uid: 10123,
    shared_uid: false,
  };
  const deviceRequest = handoffSchema.parse({
    device: device.serial,
    application_id: app.application_id,
    session_id: null,
    message: null,
  });
  assert.deepEqual(deviceRequest, {
    kind: "device",
    serial: device.serial,
    applicationId: app.application_id,
  });
  assert.deepEqual(handoffSchema.parse({ device: device.serial }), {
    kind: "device",
    serial: device.serial,
    applicationId: null,
  });
  assert.deepEqual(handoffSchema.parse({ session_id: saved.id }), {
    kind: "session",
    id: saved.id,
  });
  assert.deepEqual(
    handoffSchema.parse({ message: "Unsupported Android launch link" }),
    { kind: "rejected", message: "Unsupported Android launch link" },
  );
  assert.equal(
    handoffSchema.parse({ device: device.serial, session_id: saved.id }).kind,
    "rejected",
  );
  assert.equal(
    handoffSchema.parse({ application_id: app.application_id }).kind,
    "rejected",
  );
  assert.equal(handoffSchema.safeParse({ device: 123 }).success, false);

  const available = {
    snapshot: idle,
    operation: noOperation,
    inspected: undefined,
    pending: false,
  };
  assert.equal(handoffAvailable(available), true);
  assert.equal(handoffAvailable({ ...available, snapshot: undefined }), false);
  assert.equal(handoffAvailable({ ...available, operation: saving }), false);
  assert.equal(handoffAvailable({ ...available, operation: recovery }), false);
  assert.equal(handoffAvailable({ ...available, inspected: saved }), false);
  assert.equal(handoffAvailable({ ...available, pending: true }), false);
  const session = {
    id: "control-session",
    device,
    directory: "/capture/control-session",
    started_at: saved.started_at,
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
    log_scope: { mode: "app", application_id: app.application_id },
    network_policy: preferences.network_policy,
    context: { title: "", notes: "", steps: [] },
    log_attribution: null,
    network: { state: "not_selected" },
  };
  for (const phase of ["stopped", "failed"])
    assert.equal(
      handoffAvailable({
        ...available,
        snapshot: snapshotSchema.parse({ phase, session }),
      }),
      true,
    );
  for (const phase of ["recording", "recovery_required", "unverified_shutdown"])
    assert.equal(
      handoffAvailable({
        ...available,
        snapshot: snapshotSchema.parse({ phase, session }),
      }),
      false,
    );

  const queriedDevices: string[] = [];
  const sources = {
    discover: async () => [device],
    applications: async (serial: string) => {
      queriedDevices.push(serial);
      return [app];
    },
    catalog: async () => catalog,
  };
  assert.deepEqual(await resolveHandoff(deviceRequest, sources), {
    kind: "device",
    device,
    applicationId: app.application_id,
  });
  assert.deepEqual(queriedDevices, [device.serial]);
  for (const state of [
    "offline",
    "unauthorized",
    "unavailable",
  ] satisfies Array<NonNullable<SessionDescriptor["device"]>["state"]>) {
    assert.equal(
      (
        await resolveHandoff(deviceRequest, {
          ...sources,
          discover: async () => [
            { ...device, state },
            { ...device, serial: "other-phone" },
          ],
        })
      ).kind,
      "blocked",
    );
  }
  assert.equal(
    (
      await resolveHandoff(deviceRequest, {
        ...sources,
        discover: async () => [{ ...device, serial: "other-phone" }],
      })
    ).kind,
    "blocked",
  );
  assert.equal(
    (
      await resolveHandoff(deviceRequest, {
        ...sources,
        applications: async () => [],
      })
    ).kind,
    "blocked",
  );
  assert.equal(
    (
      await resolveHandoff(deviceRequest, {
        ...sources,
        applications: async () => [{ ...app, shared_uid: true }],
      })
    ).kind,
    "blocked",
  );
  assert.deepEqual(
    await resolveHandoff({ kind: "session", id: saved.id }, sources),
    { kind: "session", session: saved },
  );
  assert.equal(
    (await resolveHandoff({ kind: "session", id: saved.directory }, sources))
      .kind,
    "blocked",
  );
  assert.equal(
    (
      await resolveHandoff(
        { kind: "session", id: "absent-id" },
        { ...sources, catalog: async () => ({ ...catalog, truncated: true }) },
      )
    ).kind,
    "blocked",
  );
  assert.equal(
    (
      await resolveHandoff(
        { kind: "session", id: saved.id },
        {
          ...sources,
          catalog: async () => ({
            ...catalog,
            sessions: [saved, { ...saved, directory: "/capture/ambiguous" }],
          }),
        },
      )
    ).kind,
    "blocked",
  );

  const retained: HandoffNotice[] = [];
  const applied: Handoff[] = [];
  let actionPending = false;
  let release: (handoff: Handoff | null) => void = () =>
    assert.fail("Read has not started");
  const readResult = new Promise<Handoff | null>((resolve) => {
    release = resolve;
  });
  const retain = (request: Handoff): HandoffNotice => {
    const notice: HandoffNotice = {
      id: retained.length + 1,
      request,
      state: "received",
      message:
        request.kind === "rejected"
          ? request.message
          : "Launch link received. No recording has started.",
    };
    retained.push(notice);
    return notice;
  };
  const apply = async (notice: HandoffNotice) => {
    applied.push(notice.request);
  };
  const poll = consumeHandoff({
    read: () => readResult,
    retain,
    canApply: () => !actionPending,
    apply,
  });
  actionPending = true;
  release(deviceRequest);
  await poll;
  assert.deepEqual(
    retained.map((notice) => notice.request),
    [deviceRequest],
  );
  assert.deepEqual(applied, []);
  await consumeHandoff({
    read: async () => null,
    retain,
    canApply: () => true,
    apply,
  });
  assert.equal(retained.length, 1);
  await consumeHandoff({
    read: async () => ({ kind: "rejected", message: "Unsupported scheme" }),
    retain,
    canApply: () => true,
    apply,
  });
  assert.equal(retained.length, 2);
  assert.deepEqual(applied, []);
  await consumeHandoff({
    read: async () => ({ kind: "session", id: saved.id }),
    retain,
    canApply: () => true,
    apply,
  });
  assert.deepEqual(applied, [{ kind: "session", id: saved.id }]);
  assert.equal(retained.length, 3);
  await assert.rejects(
    consumeHandoff({
      read: async () => deviceRequest,
      retain,
      canApply: () => true,
      apply: async () => {
        throw new Error("Native inspection failed");
      },
    }),
    /Native inspection failed/,
  );
  assert.deepEqual(retained[3].request, deviceRequest);
  const noticeMarkup = renderToStaticMarkup(
    <HandoffNotices
      notices={retained.slice(0, 3)}
      available={false}
      apply={apply}
      dismiss={() => {}}
    />,
  );
  assert.match(noticeMarkup, /phone-requested.*com.example.requested/);
  assert.match(noticeMarkup, /Session saved-id/);
  assert.match(noticeMarkup, /Unsupported scheme/);
  assert.equal((noticeMarkup.match(/disabled="">Apply link/g) ?? []).length, 2);
  const retryMarkup = renderToStaticMarkup(
    <HandoffNotices
      notices={[retained[0]]}
      available={true}
      apply={apply}
      dismiss={() => {}}
    />,
  );
  assert.match(retryMarkup, /<button>Apply link<\/button>/);

  assert.equal(preferencesEditable(undefined, noOperation), true);
  assert.equal(preferencesEditable(undefined, saving), false);
  assert.equal(preferencesEditable(undefined, recovery), false);
  assert.equal(
    preferencesEditable(
      snapshotSchema.parse({ phase: "recording", session }),
      noOperation,
    ),
    false,
  );
  const settingsMarkup = renderToStaticMarkup(
    <Settings
      preferences={preferences}
      save={async () => {}}
      idle={false}
      editable={preferencesEditable(undefined, noOperation)}
      run={async (_label, action) => action()}
      pending=""
      setup={() => {}}
      doctor={() => {}}
    />,
  );
  assert.doesNotMatch(settingsMarkup, /<fieldset disabled/);
  assert.match(
    settingsMarkup,
    /<button class="primary">Save settings<\/button>/,
  );
  assert.match(settingsMarkup, /disabled="">Check for updates/);
  const lockedMarkup = renderToStaticMarkup(
    <Settings
      preferences={preferences}
      save={async () => {}}
      idle={false}
      editable={false}
      run={async (_label, action) => action()}
      pending=""
      setup={() => {}}
      doctor={() => {}}
    />,
  );
  assert.match(lockedMarkup, /<fieldset disabled="">/);
  assert.match(lockedMarkup, /disabled="">Save settings/);
  const captureMarkup = renderToStaticMarkup(
    <Capture
      snapshot={undefined}
      operation={noOperation}
      devices={[device]}
      draft={{
        ...initialDraft,
        serial: device.serial,
        selection: "logs",
        application_id: app.application_id,
      }}
      change={() => {}}
      preferences={preferences}
      savePreferences={async () => {}}
      refresh={async () => {}}
      run={async (_label, action) => action()}
      pending=""
      onSnapshot={() => {}}
      onOperation={() => {}}
      setup={() => {}}
      actionHost={null}
    />,
  );
  assert.match(captureMarkup, /<fieldset disabled="">/);
}
