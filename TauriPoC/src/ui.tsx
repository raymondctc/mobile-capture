import { useEffect, useRef, type ReactNode } from "react";
import type { RunAction } from "./state";

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
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      onCancel={(event) => {
        event.preventDefault();
        if (!locked) close();
      }}
    >
      <div className="dialog-heading">
        <h2>{title}</h2>
        <button disabled={locked} onClick={close} aria-label={`Close ${title}`}>
          Close
        </button>
      </div>
      {locked && (
        <p className="notice">
          Finish the current check or recovery before closing setup.
        </p>
      )}
      <div className="dialog-content">{children}</div>
    </dialog>
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
    <button
      onClick={() =>
        void run("Copying", () => navigator.clipboard.writeText(value))
      }
    >
      {label}
    </button>
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
    <label>
      {label}
      <input
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
    </label>
  );
}
export function timeLabel(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
