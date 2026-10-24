import { useRef, type ReactNode } from "react";
import type { RunAction } from "./state";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { Alert, AlertDescription } from "./components/ui/alert";
import {
  Dialog as Modal,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "./components/ui/dialog";

export function Dialog({
  title,
  close,
  children,
  locked = false,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
  locked?: boolean;
}) {
  const returnFocus = useRef(document.activeElement);
  function requestClose() {
    if (!locked) close();
  }
  return (
    <Modal
      open
      onOpenChange={(open) => {
        if (!open) requestClose();
      }}
    >
      <DialogContent
        aria-modal={true}
        showCloseButton={false}
        className="flex max-h-[calc(100dvh-2rem)] w-[min(900px,calc(100vw-2rem))] max-w-none flex-col gap-0 overflow-hidden rounded-xl p-0 sm:max-w-none"
        onEscapeKeyDown={(event) => {
          if (locked) event.preventDefault();
        }}
        onPointerDownOutside={(event) => {
          if (locked) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          if (locked) event.preventDefault();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (
            returnFocus.current instanceof HTMLElement &&
            returnFocus.current.isConnected
          )
            returnFocus.current.focus();
        }}
      >
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4">
          <div className="flex items-center justify-between gap-3">
            <DialogTitle className="text-xl font-semibold">{title}</DialogTitle>
            <Button
              disabled={locked}
              onClick={requestClose}
              aria-label={`Close ${title}`}
            >
              Close
            </Button>
          </div>
          <DialogDescription className="sr-only">
            {title} tools for Mobile Capture Android.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto overscroll-contain p-5">
          {locked && (
            <Alert className="mb-4 border-[#e4dac0] bg-[#fbf8ef] text-[#665026]">
              <AlertDescription className="text-inherit">
                Finish the current check or recovery before closing setup.
              </AlertDescription>
            </Alert>
          )}
          {children}
        </div>
      </DialogContent>
    </Modal>
  );
}
export function CopyButton({
  value,
  run,
  label = "Copy",
}: {
  value: string;
  run: RunAction;
  label?: string;
}) {
  return (
    <Button
      onClick={() =>
        void run("Copying", () => navigator.clipboard.writeText(value))
      }
    >
      {label}
    </Button>
  );
}
export function NumberField({
  label,
  value,
  change,
  min,
  max,
  scale = 1,
  disabled = false,
}: {
  label: string;
  value: number;
  change: (value: number) => void;
  min: number;
  max: number;
  scale?: number;
  disabled?: boolean;
}) {
  return (
    <Label>
      {label}
      <Input
        type="number"
        min={min / scale}
        max={max / scale}
        step="any"
        value={value / scale}
        disabled={disabled}
        onChange={(event) => {
          const number = Number(event.target.value);
          if (Number.isFinite(number)) change(Math.round(number * scale));
        }}
      />
    </Label>
  );
}
export function timeLabel(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
