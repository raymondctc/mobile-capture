import { Checkbox } from "./components/ui/checkbox";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { NativeSelect } from "./components/ui/native-select";
import { useEffect, useState } from "react";
import {
  api,
  captureSafe,
  errorMessage,
  operationBusy,
  type Device,
  type Operation,
  type SetupIdentity,
  type Snapshot,
} from "./api";
import { product } from "./product";
import { checkFreshness, type RunAction, type SetupHistory } from "./state";
import { CopyButton, timeLabel } from "./ui";

type Step = "connect" | "prepare" | "check" | "done";
const steps: { id: Step; title: string }[] = [
  { id: "connect", title: "1 Connect" },
  { id: "prepare", title: "2 Prepare" },
  { id: "check", title: "3 Check" },
  { id: "done", title: "4 Done" },
];
export function Setup({
  serial,
  selectDevice,
  devices,
  refresh,
  snapshot,
  operation,
  onOperation,
  history,
  onHistory,
  run,
  pending,
  close,
  doctor,
}: {
  serial: string;
  selectDevice: (serial: string) => void;
  devices: Device[];
  refresh: () => Promise<void>;
  snapshot: Snapshot | undefined;
  operation: Operation;
  onOperation: (operation: Operation) => void;
  history: SetupHistory;
  onHistory: (history: SetupHistory) => void;
  run: RunAction;
  pending: string;
  close: () => void;
  doctor: () => void;
}) {
  const [step, setStep] = useState<Step>("connect");
  const [consent, setConsent] = useState(false);
  const [installed, setInstalled] = useState(false);
  const [identity, setIdentity] = useState<SetupIdentity>();
  const [identityError, setIdentityError] = useState("");
  const [now, setNow] = useState(Date.now());
  const [reportPath, setReportPath] = useState("");
  const ready = devices.some(
    (device) => device.serial === serial && device.state === "ready",
  );
  const busy = operationBusy(operation);
  const allowed = captureSafe(snapshot) && !busy && !pending && ready;
  const freshness = checkFreshness(history.check, serial, identity, now);
  const certificate =
    history.certificate?.serial === serial &&
    history.certificate.certificate_fingerprint ===
      identity?.certificate_fingerprint
      ? history.certificate
      : null;
  useEffect(() => {
    setConsent(false);
    setInstalled(false);
  }, [serial]);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const result = await api.identity();
        if (!disposed) {
          setIdentity(result);
          setIdentityError("");
          setNow(Date.now());
        }
      } catch (cause) {
        if (!disposed) {
          setIdentity(undefined);
          setIdentityError(errorMessage(cause));
        }
      }
      if (!disposed) timer = setTimeout(poll, 2000);
    }
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);
  useEffect(() => {
    if (operation.state === "checking_connection") setStep("check");
    if (operation.state === "preparing_certificate") setStep("prepare");
  }, [operation.state]);
  return (
    <div className="space-y-4">
      <p>
        Prepare one Android device for network capture. Host diagnostics and
        phone verification are separate. Your inspected session stays open.
      </p>
      <nav
        className="grid grid-cols-2 gap-1 sm:grid-cols-4 rounded-lg bg-muted p-1 [&_button]:h-9 [&_button[aria-current]]:bg-card [&_button[aria-current]]:text-foreground"
        aria-label="Setup steps"
      >
        {steps.map((item) => (
          <Button
            key={item.id}
            aria-current={step === item.id ? "step" : undefined}
            onClick={() => setStep(item.id)}
          >
            {item.title}
          </Button>
        ))}
      </nav>
      <div className="space-y-1 border-y border-border py-3 text-sm">
        <strong>
          {freshness
            ? "Phone connection is not verified"
            : "Phone connection verified"}
        </strong>
        <p>
          {freshness ||
            `Fresh HTTPS check for ${serial}. Expires ${history.check ? timeLabel(history.check.expires_at) : ""}.`}
        </p>
        {identityError && (
          <p
            className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
            role="alert"
          >
            Current certificate identity could not be read. {identityError}
          </p>
        )}
      </div>
      {step === "connect" && (
        <section>
          <h3>Connect your Android device</h3>
          <ol>
            <li>Connect the phone with USB and unlock it.</li>
            <li>
              Enable Developer options and USB debugging in Android Settings.
            </li>
            <li>
              Approve this computer on the phone, then refresh the device list.
            </li>
          </ol>
          <div className="flex flex-wrap items-center gap-2">
            <NativeSelect
              aria-label="Setup device"
              value={serial}
              disabled={busy || snapshot?.phase === "recording" || !!pending}
              onChange={(event) => selectDevice(event.target.value)}
            >
              <option value="">Choose a device</option>
              {serial &&
                !devices.some((device) => device.serial === serial) && (
                  <option value={serial}>{serial} · unavailable</option>
                )}
              {devices.map((device) => (
                <option key={device.serial} value={device.serial}>
                  {device.name} · {device.state}
                </option>
              ))}
            </NativeSelect>
            <Button
              disabled={busy || !!pending}
              onClick={() => void run("Refreshing devices", refresh)}
            >
              Refresh
            </Button>
          </div>
          <p className="font-mono select-text break-all text-xs text-muted-foreground">
            {serial || "No device selected"}
          </p>
          <Button onClick={doctor}>Check host prerequisites</Button>
          <p className="text-xs leading-relaxed text-muted-foreground">
            Doctor inspects this computer. A ready result does not prove
            certificate trust or traffic from your phone.
          </p>
          <Button
            variant="default"
            disabled={!ready}
            onClick={() => setStep("prepare")}
          >
            Continue to Prepare
          </Button>
        </section>
      )}
      {(step === "prepare" || step === "check") && (
        <Label className="flex items-start gap-2 rounded-md border border-border bg-muted/35 p-3 text-sm leading-relaxed">
          <Checkbox
            checked={consent}
            disabled={busy || !!pending}
            onCheckedChange={(checked) => setConsent(checked === true)}
          />
          <span>
            I consent to a separate whole-device network setup capture on{" "}
            {serial || "the selected Android device"} and temporary proxy
            routing. The check runs for up to 45 seconds, then restores routing.
            Captured setup evidence is separate from my session.
          </span>
        </Label>
      )}
      {step === "prepare" && (
        <section>
          <h3>Prepare the capture certificate</h3>
          <p>
            Copy the public capture CA certificate to Android Downloads. The app
            restores routing before you install it. The private key remains on
            this computer.
          </p>
          <Button
            variant="default"
            disabled={!allowed || !consent}
            onClick={() =>
              void run("Preparing Android certificate", async () => {
                onOperation(await api.certificate(serial));
                setInstalled(false);
              })
            }
          >
            Copy public certificate to phone
          </Button>
          {operation.state === "preparing_certificate" && (
            <p
              className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
              role="status"
            >
              {operation.message}
            </p>
          )}
          {certificate && (
            <div className="space-y-4 rounded-lg border border-border p-4">
              <h4>Public certificate</h4>
              <dl>
                <div>
                  <dt>Device</dt>
                  <dd>{certificate.serial}</dd>
                </div>
                <div>
                  <dt>SHA-256 fingerprint</dt>
                  <dd className="font-mono select-text break-all">
                    {certificate.certificate_fingerprint}
                  </dd>
                </div>
                <div>
                  <dt>Phone file</dt>
                  <dd className="font-mono select-text break-all">
                    {certificate.device_path}
                  </dd>
                </div>
                <div>
                  <dt>Routing</dt>
                  <dd>
                    {certificate.routing_restored
                      ? "Restored"
                      : "Recovery required"}
                  </dd>
                </div>
              </dl>
              <div className="flex flex-wrap items-center gap-2">
                <CopyButton
                  value={certificate.certificate_fingerprint}
                  run={run}
                  label="Copy fingerprint"
                />
                <Button
                  onClick={() =>
                    void run("Revealing public certificate", () =>
                      product.reveal("certificate"),
                    )
                  }
                >
                  Reveal public certificate
                </Button>
              </div>
              <ol>
                <li>
                  On Android, open Settings and search for "Install a
                  certificate" or "CA certificate".
                </li>
                <li>
                  Choose the public certificate in Downloads. Android may
                  require your device PIN and show a security warning.
                </li>
                <li>
                  Confirm the certificate fingerprint where Android provides it.
                  Certificate menus vary by Android version.
                </li>
                <li>
                  Return here and run a connection check. Some apps do not trust
                  user certificates or use certificate pinning.
                </li>
              </ol>
              <Label className="flex items-start gap-2 rounded-md border border-border bg-muted/35 p-3 text-sm leading-relaxed">
                <Checkbox
                  checked={installed}
                  onCheckedChange={(checked) => setInstalled(checked === true)}
                />
                <span>
                  I completed manual installation. This acknowledgment does not
                  verify phone trust.
                </span>
              </Label>
            </div>
          )}
          {!certificate && identity?.certificate_fingerprint && (
            <p className="font-mono select-text break-all text-xs leading-relaxed text-muted-foreground">
              Current CA · {identity.certificate_fingerprint}
            </p>
          )}
          <Button
            disabled={!certificate?.routing_restored || !installed}
            onClick={() => setStep("check")}
          >
            Continue to Check
          </Button>
        </section>
      )}
      {step === "check" && (
        <section>
          <h3>Verify HTTPS from this phone</h3>
          <p>
            Start the check, open its fresh HTTPS URL in the phone browser, and
            confirm you used this phone. A host request, TLS failure, or
            installation acknowledgment cannot pass the check.
          </p>
          <Button
            variant="default"
            disabled={
              !allowed || !consent || !identity?.certificate_fingerprint
            }
            onClick={() =>
              void run("Starting phone connection check", async () =>
                onOperation(await api.check(serial)),
              )
            }
          >
            Start 45-second connection check
          </Button>
          {operation.state === "checking_connection" && (
            <div className="space-y-3 rounded-lg border border-border p-4">
              <p className="text-lg font-medium tabular-nums" role="timer">
                {operation.remaining_seconds} seconds remaining
              </p>
              <Label>
                Open this URL on the selected phone
                <Input
                  className="font-mono select-text break-all"
                  readOnly
                  value={operation.url}
                />
              </Label>
              <div className="flex flex-wrap items-center gap-2">
                <CopyButton
                  value={operation.url}
                  run={run}
                  label="Copy check URL"
                />
                <Button
                  disabled={!!pending}
                  onClick={() =>
                    void run("Opening check on selected phone", () =>
                      api.openCheck(operation.id),
                    )
                  }
                >
                  Open on phone
                </Button>
              </div>
              <p className="font-mono select-text break-all text-xs text-muted-foreground">
                CA fingerprint · {operation.certificate_fingerprint}
              </p>
              <Label className="flex items-start gap-2 rounded-md border border-border bg-muted/35 p-3 text-sm leading-relaxed">
                <Checkbox
                  checked={operation.phone_origin_confirmed}
                  disabled={!!pending}
                  onCheckedChange={(checked) => {
                    const confirmed = checked === true;
                    void run("Confirming phone origin", async () =>
                      onOperation(
                        await api.confirmOrigin(operation.id, confirmed),
                      ),
                    );
                  }}
                />
                <span>I opened this URL in the browser on {serial}.</span>
              </Label>
              <p
                className={
                  operation.https_observed
                    ? "text-sm text-success"
                    : "text-xs text-muted-foreground"
                }
              >
                {operation.https_observed
                  ? "The exact fresh HTTPS response was observed."
                  : "Waiting for the exact fresh HTTPS response."}
              </p>
              <Button
                disabled={!!pending}
                onClick={() =>
                  void run("Finishing connection check", async () =>
                    onOperation(await api.cancel(operation.id)),
                  )
                }
              >
                Finish check and restore routing
              </Button>
            </div>
          )}
          {history.check?.serial === serial && (
            <div className="space-y-3 rounded-lg border border-border p-4">
              <p>{history.check.message}</p>
              <dl>
                <div>
                  <dt>HTTPS response</dt>
                  <dd>
                    {history.check.https_observed ? "Observed" : "Not observed"}
                  </dd>
                </div>
                <div>
                  <dt>Phone origin</dt>
                  <dd>
                    {history.check.phone_origin_confirmed
                      ? "Confirmed by you"
                      : "Not confirmed"}
                  </dd>
                </div>
                <div>
                  <dt>Routing</dt>
                  <dd>
                    {history.check.routing_restored
                      ? "Restored"
                      : "Recovery required"}
                  </dd>
                </div>
                <div>
                  <dt>Checked</dt>
                  <dd>{timeLabel(history.check.checked_at)}</dd>
                </div>
              </dl>
              <p className="text-xs leading-relaxed text-muted-foreground">
                This browser check does not establish every app's certificate
                trust, upstream internet availability, or app attribution.
              </p>
            </div>
          )}
          <Button disabled={busy} onClick={() => setStep("done")}>
            Review result
          </Button>
        </section>
      )}
      {step === "done" && (
        <section>
          <h3>
            {freshness
              ? "Setup needs a fresh phone check"
              : "Ready for a recording"}
          </h3>
          <p>
            {freshness ||
              "The current device and CA have a fresh verified browser connection check. Capture sources and consent are chosen separately for each recording."}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              disabled={busy || !!pending}
              onClick={() =>
                void run("Exporting readiness report", async () => {
                  const path = await api.report();
                  if (path) setReportPath(path);
                })
              }
            >
              Export readiness report
            </Button>
            <Button
              variant="default"
              disabled={busy}
              onClick={() => {
                onHistory({ ...history, deferred: true });
                close();
              }}
            >
              Finish setup
            </Button>
          </div>
          {reportPath && (
            <p
              className="rounded-md border border-success/25 bg-success/10 p-3 font-mono text-success select-text break-all"
              role="status"
            >
              Report saved · {reportPath}
            </p>
          )}
          <p className="text-xs leading-relaxed text-muted-foreground">
            The report includes public host and setup facts. It excludes
            captured bodies, logs, private keys, and routing files.
          </p>
        </section>
      )}
      {!busy && (
        <div className="flex flex-wrap items-center gap-3 border-t border-border pt-4">
          <Button
            variant="ghost"
            onClick={() => {
              onHistory({ ...history, deferred: true });
              close();
            }}
          >
            Set up later
          </Button>
          <span className="text-xs leading-relaxed text-muted-foreground">
            You can return through Device setup. Deferral never marks the phone
            verified.
          </span>
        </div>
      )}
    </div>
  );
}
