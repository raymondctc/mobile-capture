import type { Dispatch, SetStateAction } from "react";
import type { Representation } from "./body-document";
import type { BodySide, EventRef } from "./product";

export type InspectorPreferences = {
  side: BodySide | null;
  representation: Representation;
  headersOpen: Record<BodySide, boolean>;
  find: { open: boolean; query: string };
};
export const initialInspectorPreferences: InspectorPreferences = {
  side: null,
  representation: "formatted",
  headersOpen: { request: false, response: false },
  find: { open: false, query: "" },
};
export type FindFocusIntent = { event: EventRef; token: number };
export type InspectorPreferenceProps = {
  preferences: InspectorPreferences;
  onPreferences: Dispatch<SetStateAction<InspectorPreferences>>;
  findFocus: FindFocusIntent | null;
  onFindFocusConsumed: (token: number) => void;
  onRequestFind: () => void;
};
