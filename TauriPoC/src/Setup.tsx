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
    <div className="setup">
      <p>
        Prepare one Android device for network capture. Host diagnostics and
        phone verification are separate. Your inspected session stays open.
      </p>
      <nav className="setup-steps" aria-label="Setup steps">
        {steps.map((item) => (
          <button
            key={item.id}
            aria-current={step === item.id ? "step" : undefined}
            onClick={() => setStep(item.id)}
          >
            {item.title}
          </button>
        ))}
      </nav>
      <div className="setup-summary">
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
          <p className="notice" role="alert">
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
          <div className="button-row">
            <select
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
            </select>
            <button
              disabled={busy || !!pending}
              onClick={() => void run("Refreshing devices", refresh)}
            >
              Refresh
            </button>
          </div>
          <p className="mono muted">{serial || "No device selected"}</p>
          <button onClick={doctor}>Check host prerequisites</button>
          <p className="hint">
            Doctor inspects this computer. A ready result does not prove
            certificate trust or traffic from your phone.
          </p>
          <button
            className="primary"
            disabled={!ready}
            onClick={() => setStep("prepare")}
          >
            Continue to Prepare
          </button>
        </section>
      )}
      {(step === "prepare" || step === "check") && (
        <label className="consent setup-consent">
          <input
            type="checkbox"
            checked={consent}
            disabled={busy || !!pending}
            onChange={(event) => setConsent(event.target.checked)}
          />
          <span>
            I consent to a separate whole-device network setup capture on{" "}
            {serial || "the selected Android device"} and temporary proxy
            routing. The check runs for up to 45 seconds, then restores routing.
            Captured setup evidence is separate from my session.
          </span>
        </label>
      )}
      {step === "prepare" && (
        <section>
          <h3>Prepare the capture certificate</h3>
          <p>
            Copy the public capture CA certificate to Android Downloads. The app
            restores routing before you install it. The private key remains on
            this computer.
          </p>
          <button
            className="primary"
            disabled={!allowed || !consent}
            onClick={() =>
              void run("Preparing Android certificate", async () => {
                onOperation(await api.certificate(serial));
                setInstalled(false);
              })
            }
          >
            Copy public certificate to phone
          </button>
          {operation.state === "preparing_certificate" && (
            <p className="notice" role="status">
              {operation.message}
            </p>
          )}
          {certificate && (
            <div className="certificate-card">
              <h4>Public certificate</h4>
              <dl>
                <div>
                  <dt>Device</dt>
                  <dd>{certificate.serial}</dd>
                </div>
                <div>
                  <dt>SHA-256 fingerprint</dt>
                  <dd className="mono">
                    {certificate.certificate_fingerprint}
                  </dd>
                </div>
                <div>
                  <dt>Phone file</dt>
                  <dd className="mono">{certificate.device_path}</dd>
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
              <div className="button-row">
                <CopyButton
                  value={certificate.certificate_fingerprint}
                  run={run}
                  label="Copy fingerprint"
                />
                <button
                  onClick={() =>
                    void run("Revealing public certificate", () =>
                      product.reveal("certificate"),
                    )
                  }
                >
                  Reveal public certificate
                </button>
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
              <label className="consent">
                <input
                  type="checkbox"
                  checked={installed}
                  onChange={(event) => setInstalled(event.target.checked)}
                />
                <span>
                  I completed manual installation. This acknowledgment does not
                  verify phone trust.
                </span>
              </label>
            </div>
          )}
          {!certificate && identity?.certificate_fingerprint && (
            <p className="hint mono">
              Current CA · {identity.certificate_fingerprint}
            </p>
          )}
          <button
            disabled={!certificate?.routing_restored || !installed}
            onClick={() => setStep("check")}
          >
            Continue to Check
          </button>
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
          <button
            className="primary"
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
          </button>
          {operation.state === "checking_connection" && (
            <div className="check-card">
              <p className="countdown" role="timer">
                {operation.remaining_seconds} seconds remaining
              </p>
              <label>
                Open this URL on the selected phone
                <input className="mono" readOnly value={operation.url} />
              </label>
              <div className="button-row">
                <CopyButton
                  value={operation.url}
                  run={run}
                  label="Copy check URL"
                />
                <button
                  disabled={!!pending}
                  onClick={() =>
                    void run("Opening check on selected phone", () =>
                      api.openCheck(operation.id),
                    )
                  }
                >
                  Open on phone
                </button>
              </div>
              <p className="mono hint">
                CA fingerprint · {operation.certificate_fingerprint}
              </p>
              <label className="consent">
                <input
                  type="checkbox"
                  checked={operation.phone_origin_confirmed}
                  disabled={!!pending}
                  onChange={(event) => {
                    const confirmed = event.target.checked;
                    void run("Confirming phone origin", async () =>
                      onOperation(
                        await api.confirmOrigin(operation.id, confirmed),
                      ),
                    );
                  }}
                />
                <span>I opened this URL in the browser on {serial}.</span>
              </label>
              <p className={operation.https_observed ? "success" : "hint"}>
                {operation.https_observed
                  ? "The exact fresh HTTPS response was observed."
                  : "Waiting for the exact fresh HTTPS response."}
              </p>
              <button
                disabled={!!pending}
                onClick={() =>
                  void run("Finishing connection check", async () =>
                    onOperation(await api.cancel(operation.id)),
                  )
                }
              >
                Finish check and restore routing
              </button>
            </div>
          )}
          {history.check?.serial === serial && (
            <div className="check-card">
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
              <p className="hint">
                This browser check does not establish every app's certificate
                trust, upstream internet availability, or app attribution.
              </p>
            </div>
          )}
          <button disabled={busy} onClick={() => setStep("done")}>
            Review result
          </button>
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
          <div className="button-row">
            <button
              disabled={busy || !!pending}
              onClick={() =>
                void run("Exporting readiness report", async () => {
                  const path = await api.report();
                  if (path) setReportPath(path);
                })
              }
            >
              Export readiness report
            </button>
            <button
              className="primary"
              disabled={busy}
              onClick={() => {
                onHistory({ ...history, deferred: true });
                close();
              }}
            >
              Finish setup
            </button>
          </div>
          {reportPath && (
            <p className="success mono" role="status">
              Report saved · {reportPath}
            </p>
          )}
          <p className="hint">
            The report includes public host and setup facts. It excludes
            captured bodies, logs, private keys, and routing files.
          </p>
        </section>
      )}
      {!busy && (
        <div className="setup-footer">
          <button
            className="text"
            onClick={() => {
              onHistory({ ...history, deferred: true });
              close();
            }}
          >
            Set up later
          </button>
          <span className="hint">
            You can return through Device setup. Deferral never marks the phone
            verified.
          </span>
        </div>
      )}
    </div>
  );
}
