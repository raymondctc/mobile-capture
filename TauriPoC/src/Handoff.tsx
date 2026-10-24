import { Button } from "./components/ui/button";
import {
  captureSafe,
  operationBusy,
  type api,
  type Device,
  type Handoff,
  type Operation,
  type Snapshot,
} from "./api";
import { product, type SessionDescriptor } from "./product";

export type HandoffNotice = {
  id: number;
  request: Handoff;
  state: "received" | "applied";
  message: string;
};
type Resolution =
  | {
      kind: "device";
      device: Device;
      applicationId: string | null;
    }
  | { kind: "session"; session: SessionDescriptor }
  | { kind: "blocked"; message: string };

export function handoffAvailable({
  snapshot,
  operation,
  inspected,
  pending,
}: {
  snapshot: Snapshot | undefined;
  operation: Operation;
  inspected: SessionDescriptor | undefined;
  pending: boolean;
}): boolean {
  return (
    captureSafe(snapshot) && !operationBusy(operation) && !inspected && !pending
  );
}

export async function consumeHandoff({
  read,
  retain,
  canApply,
  apply,
}: {
  read: () => Promise<Handoff | null>;
  retain: (request: Handoff) => HandoffNotice;
  canApply: () => boolean;
  apply: (notice: HandoffNotice) => Promise<void>;
}): Promise<void> {
  const request = await read();
  if (!request) return;
  const notice = retain(request);
  if (request.kind !== "rejected" && canApply()) await apply(notice);
}

export async function resolveHandoff(
  request: Handoff,
  sources: Pick<typeof api, "discover"> &
    Pick<typeof product, "applications" | "catalog">,
): Promise<Resolution> {
  if (request.kind === "rejected")
    return { kind: "blocked", message: request.message };
  if (request.kind === "session") {
    const catalog = await sources.catalog();
    const matches = catalog.sessions.filter(
      (session) => session.id === request.id,
    );
    if (matches.length !== 1)
      return {
        kind: "blocked",
        message: matches.length
          ? "More than one saved session has this ID. Open the intended session from the library."
          : "This session ID is not in the saved-session catalog. Refresh the library or open its folder explicitly.",
      };
    return { kind: "session", session: matches[0] };
  }
  const devices = await sources.discover();
  const device = devices.find((entry) => entry.serial === request.serial);
  if (device?.state !== "ready")
    return {
      kind: "blocked",
      message:
        "The requested Android device is not ready. Connect and authorize that device, then apply the link again.",
    };
  if (request.applicationId) {
    const apps = await sources.applications(device.serial);
    const app = apps.find(
      (entry) => entry.application_id === request.applicationId,
    );
    if (!app || app.shared_uid)
      return {
        kind: "blocked",
        message:
          "The requested app is not available for app-scoped capture on this device. Refresh installed apps and choose its scope explicitly.",
      };
  }
  return { kind: "device", device, applicationId: request.applicationId };
}

export function HandoffNotices({
  notices,
  available,
  apply,
  dismiss,
}: {
  notices: HandoffNotice[];
  available: boolean;
  apply: (notice: HandoffNotice) => Promise<void>;
  dismiss: (id: number) => void;
}) {
  return notices.map((notice) => (
    <section
      className="mx-4 flex flex-wrap items-start justify-between gap-3 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning select-text"
      role="status"
      key={notice.id}
      aria-label="Android launch link"
    >
      <div>
        <strong>Android launch link</strong>
        {notice.request.kind === "device" && (
          <p className="font-mono select-text break-all">
            {notice.request.serial}
            {notice.request.applicationId
              ? ` · ${notice.request.applicationId}`
              : ""}
          </p>
        )}
        {notice.request.kind === "session" && (
          <p className="font-mono select-text break-all">
            Session {notice.request.id}
          </p>
        )}
        <p>{notice.message}</p>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {notice.state === "received" && notice.request.kind !== "rejected" && (
          <Button disabled={!available} onClick={() => void apply(notice)}>
            Apply link
          </Button>
        )}
        <Button onClick={() => dismiss(notice.id)}>Dismiss</Button>
      </div>
    </section>
  ));
}
