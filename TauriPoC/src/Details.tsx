import { useEffect, useRef, useState } from "react";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { product, type EventDetail, type BodySide } from "./product";
import { errorMessage } from "./api";
import { timeLabel } from "./ui";
import { BodyInspector } from "./BodyInspector";
import { DetailChunks } from "./DetailChunks";
import { LogDetails } from "./LogDetails";
import {
  initialInspectorPreferences,
  type FindFocusIntent,
  type InspectorPreferenceProps,
} from "./inspector-state";
export { hexChunk } from "./DetailChunks";

type Content =
  | { kind: "primary" }
  | { kind: "metadata" }
  | { kind: "raw"; side: BodySide };
type Reveal =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "error"; message: string };
export function Details({
  detail,
  offPage,
  wrap,
  visible = true,
  inspector,
}: {
  detail: EventDetail;
  offPage: boolean;
  wrap: boolean;
  visible?: boolean;
  inspector?: InspectorPreferenceProps;
}) {
  const [preferences, onPreferences] = useState(initialInspectorPreferences);
  const [findFocus, setFindFocus] = useState<FindFocusIntent | null>(null);
  const findToken = useRef(0);
  const controls: InspectorPreferenceProps = inspector ?? {
    preferences,
    onPreferences,
    findFocus,
    onFindFocusConsumed: (token) =>
      setFindFocus((previous) => (previous?.token === token ? null : previous)),
    onRequestFind: () => {
      onPreferences((previous) => ({
        ...previous,
        find: { ...previous.find, open: true },
      }));
      setFindFocus({
        event: { session_key: detail.session_key, event_id: detail.event.id },
        token: ++findToken.current,
      });
    },
  };
  const [content, setContent] = useState<Content>({ kind: "primary" });
  const [reveal, setReveal] = useState<Reveal>({ kind: "idle" });
  const revision = useRef(0);
  useEffect(() => {
    if (
      controls.findFocus?.event.event_id === detail.event.id &&
      controls.findFocus.event.session_key === detail.session_key
    )
      setContent({ kind: "primary" });
  }, [controls.findFocus, detail.event.id, detail.session_key]);
  useEffect(() => {
    if (inspector || !visible || detail.event.source !== "network") return;
    function key(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        controls.onRequestFind();
      }
    }
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, [inspector, visible, detail.event.source, controls.onRequestFind]);
  useEffect(
    () => () => {
      revision.current += 1;
    },
    [],
  );
  async function revealRaw() {
    const request = ++revision.current;
    setReveal({ kind: "pending" });
    try {
      await product.reveal("raw", detail.event.id);
      if (request === revision.current) setReveal({ kind: "idle" });
    } catch (cause) {
      if (request === revision.current)
        setReveal({ kind: "error", message: errorMessage(cause) });
    }
  }
  const network = detail.event.source === "network";
  const logs = detail.event.source === "logs";
  return (
    <section
      className="flex min-h-0 flex-1 flex-col select-text"
      aria-label="Event details"
    >
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto overscroll-contain px-4 py-4">
        {offPage && (
          <Badge variant="secondary">Selected outside this page</Badge>
        )}
        <ToggleGroup
          type="single"
          aria-label="Detail view"
          value={content.kind === "raw" ? "" : content.kind}
          onValueChange={(value) => {
            if (value === "primary" || value === "metadata")
              setContent({ kind: value });
          }}
        >
          <ToggleGroupItem value="primary">
            {network ? "Bodies" : logs ? "Log message" : "Event"}
          </ToggleGroupItem>
          <ToggleGroupItem value="metadata">Full metadata</ToggleGroupItem>
        </ToggleGroup>
        {network && (
          <div hidden={content.kind !== "primary"}>
            <BodyInspector
              reference={{
                session_key: detail.session_key,
                event_id: detail.event.id,
              }}
              wrap={wrap}
              visible={visible}
              paneVisible={content.kind === "primary"}
              {...controls}
              onRaw={(side) => setContent({ kind: "raw", side })}
            />
          </div>
        )}
        {content.kind === "primary" && logs && (
          <LogDetails detail={detail} wrap={wrap} />
        )}
        {content.kind === "primary" && !network && !logs && (
          <p className="break-words text-sm">{detail.event.summary}</p>
        )}
        {content.kind === "metadata" && (
          <DetailChunks
            reference={{
              session_key: detail.session_key,
              event_id: detail.event.id,
            }}
            wrap={wrap}
          />
        )}
        {content.kind === "raw" && (
          <DetailChunks
            key={content.side}
            reference={{
              session_key: detail.session_key,
              event_id: detail.event.id,
            }}
            side={content.side}
            wrap={wrap}
          />
        )}
        <details className="text-xs">
          <summary className="cursor-pointer py-2">Event metadata</summary>
          <dl className="grid gap-3 [&_dt]:text-muted-foreground [&_dd]:break-all [&_dd]:font-mono">
            <div>
              <dt>Time</dt>
              <dd>{timeLabel(detail.event.timestamp)}</dd>
            </div>
            <div>
              <dt>Source</dt>
              <dd>
                {detail.event.source} · {detail.event.kind}
              </dd>
            </div>
            <div>
              <dt>Raw reference</dt>
              <dd>{detail.event.raw_ref}</dd>
            </div>
            <div>
              <dt>Event ID</dt>
              <dd>{detail.event.id}</dd>
            </div>
            {detail.event.clock_uncertainty_ms !== null && (
              <div>
                <dt>Clock uncertainty</dt>
                <dd>{detail.event.clock_uncertainty_ms} ms</dd>
              </div>
            )}
            {detail.event.original_timestamp && (
              <div>
                <dt>Original time</dt>
                <dd>{detail.event.original_timestamp}</dd>
              </div>
            )}
            {detail.event.received_at && (
              <div>
                <dt>Received</dt>
                <dd>{detail.event.received_at}</dd>
              </div>
            )}
          </dl>
        </details>
      </div>
      <footer className="shrink-0 space-y-2 border-t border-border px-4 py-3">
        {reveal.kind === "error" && (
          <p role="alert" className="text-xs text-warning">
            {reveal.message}
          </p>
        )}
        <Button
          variant="outline"
          disabled={reveal.kind === "pending"}
          onClick={() => void revealRaw()}
        >
          {reveal.kind === "pending"
            ? "Revealing raw source…"
            : "Reveal raw source"}
        </Button>
      </footer>
    </section>
  );
}
