import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, type Preferences } from "./api";
import { product } from "./product";
import { Policies } from "./Policies";
import type { RunAction } from "./state";
export function Settings({
  preferences,
  save,
  idle,
  editable,
  run,
  pending,
  setup,
  doctor,
}: {
  preferences: Preferences | undefined;
  save: (preferences: Preferences) => Promise<void>;
  idle: boolean;
  editable: boolean;
  run: RunAction;
  pending: string;
  setup: () => void;
  doctor: () => void;
}) {
  const [draft, setDraft] = useState(preferences);
  const [version, setVersion] = useState("");
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => setVersion("Unavailable outside the installed app"));
  }, []);
  useEffect(() => setDraft(preferences), [preferences]);
  const locked = !editable || !!pending;
  function update(next: Preferences) {
    setDraft(next);
    setSaved(false);
  }
  const overridesValid =
    draft &&
    [draft.adb_override, draft.mitmdump_override].every(
      (path) =>
        path === null ||
        path.startsWith("/") ||
        path.startsWith("\\\\") ||
        /^[A-Za-z]:[\\/]/.test(path),
    );
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={setup}>Device setup</Button>
        <Button onClick={doctor}>Host Doctor</Button>
      </div>
      <p>
        Storage and tool changes apply to the next recording. Capture, saving,
        setup, and recovery must finish first.
      </p>
      {!editable && (
        <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
          Settings are read-only until capture ownership and device routing are
          safely idle.
        </p>
      )}
      {editable && !idle && (
        <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
          Correct loaded preferences if a tool override prevents initialization.
          The native app verifies capture ownership before saving.
        </p>
      )}
      {draft ? (
        <form
          className="space-y-5"
          onSubmit={(event) => {
            event.preventDefault();
            if (draft && overridesValid)
              void run("Saving settings", async () => {
                await save(draft);
                setSaved(true);
              });
          }}
        >
          <fieldset
            className="space-y-3 rounded-lg border border-border p-3"
            disabled={locked}
          >
            <legend className="px-1 text-sm font-medium">
              Capture storage
            </legend>
            <Label>
              Session folder
              <Input
                className="font-mono select-text break-all"
                value={draft.output_root}
                onChange={(event) =>
                  update({ ...draft, output_root: event.target.value })
                }
                required
              />
            </Label>
            <Button
              type="button"
              onClick={() =>
                void run("Choosing capture storage", async () => {
                  const directory = await product.chooseFolder();
                  if (directory) update({ ...draft, output_root: directory });
                })
              }
            >
              Choose folder
            </Button>
          </fieldset>
          <Policies value={draft} change={update} disabled={locked} />
          <fieldset
            className="space-y-3 rounded-lg border border-border p-3"
            disabled={locked}
          >
            <legend className="px-1 text-sm font-medium">
              Explicit tool overrides
            </legend>
            <p className="text-xs leading-relaxed text-muted-foreground">
              Packaged tools are the default. Enter an absolute executable path
              only when you deliberately use external tools. A damaged packaged
              payload does not silently fall back to installed tools.
            </p>
            <Label>
              ADB executable
              <Input
                className="font-mono select-text break-all"
                value={draft.adb_override ?? ""}
                placeholder="Use packaged ADB"
                onChange={(event) =>
                  update({ ...draft, adb_override: event.target.value || null })
                }
              />
            </Label>
            <Label>
              mitmdump executable
              <Input
                className="font-mono select-text break-all"
                value={draft.mitmdump_override ?? ""}
                placeholder="Use packaged network engine"
                onChange={(event) =>
                  update({
                    ...draft,
                    mitmdump_override: event.target.value || null,
                  })
                }
              />
            </Label>
            {!overridesValid && (
              <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
                Each override must be an absolute executable path.
              </p>
            )}
          </fieldset>
          <Button
            type="submit"
            variant="default"
            disabled={locked || !overridesValid}
          >
            Save settings
          </Button>
          {saved && (
            <p
              className="rounded-md border border-success/25 bg-success/10 p-3 text-success"
              role="status"
            >
              Settings saved for future recordings.
            </p>
          )}
        </form>
      ) : (
        <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text">
          Preferences could not be loaded. Retry connecting to the native app.
        </p>
      )}
      <section className="space-y-3 border-t border-border pt-4">
        <h3>Mobile Capture Android</h3>
        <p>Version {version || "…"}</p>
        <p>
          Updates use manual installation. Finish capture, save, setup, and
          recovery before opening the Android release page.
        </p>
        <Button
          disabled={!idle || !!pending}
          onClick={() => void run("Opening Android releases", api.updates)}
        >
          Check for updates
        </Button>
        <p className="text-xs leading-relaxed text-muted-foreground">
          Closing the window keeps a normal recording alive. Reopen the app to
          return. Quitting waits for routing restoration and safe shutdown.
        </p>
      </section>
    </div>
  );
}
