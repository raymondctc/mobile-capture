import { useState } from "react";
import { api, type DoctorReport } from "./api";
import type { RunAction } from "./state";
import { timeLabel } from "./ui";
export function Diagnostics({
  run,
  pending,
  settings,
}: {
  run: RunAction;
  pending: string;
  settings: () => void;
}) {
  const [sources, setSources] =
    useState<DoctorReport["sources"]>("logsAndNetwork");
  const [report, setReport] = useState<DoctorReport>();
  return (
    <div className="diagnostics">
      <p>
        Doctor checks capture tools, their versions and integrity, and storage
        on this computer. It does not inspect or change your phone or its
        routing.
      </p>
      <div className="button-row">
        <select
          aria-label="Doctor capture sources"
          value={sources}
          onChange={(event) => {
            const value = event.target.value;
            if (
              value === "logs" ||
              value === "network" ||
              value === "logsAndNetwork"
            )
              setSources(value);
          }}
        >
          <option value="logsAndNetwork">Logs and network</option>
          <option value="logs">Logs only</option>
          <option value="network">Network only</option>
        </select>
        <button
          className="primary"
          disabled={!!pending}
          onClick={() =>
            void run("Checking host prerequisites", async () =>
              setReport(await api.doctor(sources)),
            )
          }
        >
          Run host checks
        </button>
        <button onClick={settings}>Storage and tool settings</button>
      </div>
      {report && (
        <>
          <div className={report.hostPrerequisitesReady ? "success" : "notice"}>
            <strong>
              {report.hostPrerequisitesReady
                ? "Host prerequisites ready"
                : "Host action needed"}
            </strong>
            <p>Phone connection is not established by this result.</p>
          </div>
          <p className="hint">
            {report.hostTarget} · {timeLabel(report.checkedAt)}
          </p>
          {report.error && (
            <p className="error" role="alert">
              {report.error.message}
            </p>
          )}
          <div className="doctor-checks">
            {report.checks.map((check) => (
              <article key={check.id}>
                <div className="section-heading">
                  <h3>{check.id.replaceAll(/([a-z])([A-Z])/g, "$1 $2")}</h3>
                  <span className={`badge ${check.status}`}>
                    {check.status.replace("needsAction", "Action needed")}
                  </span>
                </div>
                <p>{check.message}</p>
                <dl>
                  <div>
                    <dt>Source</dt>
                    <dd>{check.source}</dd>
                  </div>
                  {check.version && (
                    <div>
                      <dt>Version</dt>
                      <dd>{check.version}</dd>
                    </div>
                  )}
                  {check.path && (
                    <div>
                      <dt>Path</dt>
                      <dd className="mono">{check.path}</dd>
                    </div>
                  )}
                </dl>
                {check.status !== "ready" && (
                  <p className="hint">
                    {check.id.toLocaleLowerCase().includes("storage")
                      ? "Choose a writable capture folder in Settings."
                      : check.source === "bundled"
                        ? "Replace the app with the official Android release if its packaged tools are missing, incompatible, or damaged."
                        : "Review the explicit absolute tool override in Settings. Remove it to use the packaged tools."}
                  </p>
                )}
              </article>
            ))}
          </div>
          <h3>Manual device actions</h3>
          <ul>
            {report.deviceSetup.manualActions.map((action) => (
              <li key={action}>{action}</li>
            ))}
          </ul>
          <p className="hint">
            Device setup status · {report.deviceSetup.status}. Run Device setup
            for certificate preparation and a fresh phone connection check.
          </p>
        </>
      )}
    </div>
  );
}
