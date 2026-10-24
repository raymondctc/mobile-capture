import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { NativeSelect } from "./components/ui/native-select";
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
    <div className="space-y-4">
      <p>
        Doctor checks capture tools, their versions and integrity, and storage
        on this computer. It does not inspect or change your phone or its
        routing.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <NativeSelect
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
        </NativeSelect>
        <Button
          className="bg-primary text-primary-foreground hover:bg-primary/85"
          disabled={!!pending}
          onClick={() =>
            void run("Checking host prerequisites", async () =>
              setReport(await api.doctor(sources)),
            )
          }
        >
          Run host checks
        </Button>
        <Button onClick={settings}>Storage and tool settings</Button>
      </div>
      {report && (
        <>
          <div
            className={`rounded-md border p-3 ${report.hostPrerequisitesReady ? "border-[#c8e3d4] bg-[#edf7f1] text-[#23553e]" : "border-[#e4dac0] bg-[#fbf8ef] text-[#665026]"}`}
          >
            <strong>
              {report.hostPrerequisitesReady
                ? "Host prerequisites ready"
                : "Host action needed"}
            </strong>
            <p>Phone connection is not established by this result.</p>
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {report.hostTarget} · {timeLabel(report.checkedAt)}
          </p>
          {report.error && (
            <p
              className="rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive select-text"
              role="alert"
            >
              {report.error.message}
            </p>
          )}
          <div className="divide-y divide-border [&>article]:space-y-2 [&>article]:py-4">
            {report.checks.map((check) => (
              <article key={check.id}>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h3>{check.id.replaceAll(/([a-z])([A-Z])/g, "$1 $2")}</h3>
                  <Badge
                    variant="secondary"
                    className={
                      check.status === "ready"
                        ? "bg-[#edf5f1] text-[#156b58]"
                        : "bg-[#fbf8ef] text-[#665026]"
                    }
                  >
                    {check.status.replace("needsAction", "Action needed")}
                  </Badge>
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
                      <dd className="font-mono select-text break-all">
                        {check.path}
                      </dd>
                    </div>
                  )}
                </dl>
                {check.status !== "ready" && (
                  <p className="text-xs leading-relaxed text-muted-foreground">
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
          <p className="text-xs leading-relaxed text-muted-foreground">
            Device setup status · {report.deviceSetup.status}. Run Device setup
            for certificate preparation and a fresh phone connection check.
          </p>
        </>
      )}
    </div>
  );
}
