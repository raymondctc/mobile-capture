import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { Info } from "lucide-react";
import { Button } from "./components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "./components/ui/popover";

const routineNotices = new Set([
  "Device clock uncertainty is unknown. Time proximity does not establish causality.",
  "Timestamp proximity does not prove causation.",
  "Device clock uncertainty unknown",
  "Android logs scoped as recorded in capture.json and context.json",
  "Whole-device proxy evidence. Manual CA trust, certificate pinning, and omissions limit coverage.",
  "Guardian hard death may interrupt the listener and original evidence.",
]);
const applicationId = "[A-Za-z][A-Za-z0-9_]*(?:\\.[A-Za-z0-9_]+)+";
const logScopeNotice = new RegExp(
  `^Android log scope: (?:WholeDevice|App \\{ application_id: "${applicationId}" \\})\\. All available emitted levels from main, system, and crash buffers\\.$`,
);
const networkNotice = new RegExp(
  `^(?:Device logs were not selected\\.|Device logs cover all apps and system processes\\.|Device logs are limited to ${applicationId}\\.) Network capture covers whole-device HTTP\\(S\\)\\. HTTPS requires manual CA trust; certificate pinning can prevent observation\\.$`,
);

export function isRoutineNotice(message: string): boolean {
  return (
    message === message.trim() &&
    (routineNotices.has(message) ||
      logScopeNotice.test(message) ||
      networkNotice.test(message))
  );
}

export function CaptureInfo({
  label = "Capture information",
  children,
  visible = true,
}: {
  label?: string;
  children: ReactNode;
  visible?: boolean;
}) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!visible) setOpen(false);
  }, [visible]);
  return (
    <Popover open={visible && open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={label}>
          <Info aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        aria-label={label}
        align="end"
        className="max-h-[min(28rem,75dvh)] w-[min(26rem,calc(100vw-2rem))] overflow-y-auto p-4 text-xs leading-relaxed text-muted-foreground select-text"
      >
        {children}
      </PopoverContent>
    </Popover>
  );
}

export function Notices({
  messages,
  visible = true,
  label,
}: {
  messages: string[];
  visible?: boolean;
  label: string;
}) {
  const routine = messages.filter(isRoutineNotice);
  return (
    <>
      {routine.length > 0 && (
        <CaptureInfo label={label} visible={visible}>
          {routine.map((message) => (
            <p key={message}>{message}</p>
          ))}
        </CaptureInfo>
      )}
      {messages
        .filter((message) => !isRoutineNotice(message))
        .map((message) => (
          <p
            key={message}
            className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
          >
            {message}
          </p>
        ))}
    </>
  );
}
