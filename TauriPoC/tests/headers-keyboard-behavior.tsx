import assert from "node:assert/strict";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { Timeline } from "../src/Timeline";
import { Details } from "../src/Details";
import {
  product,
  type BodyDocuments,
  type EventDetail,
  type TimelinePage,
  type TimelineQuery,
} from "../src/product";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}
const directory = "/synthetic/keyboard";
const descriptor = {
  version: 1,
  id: "keyboard",
  directory,
  device: null,
  started_at: "2026-10-23T00:00:00Z",
  legacy: false,
  metadata: null,
  warnings: [],
};
const event = (id: string) => ({
  id,
  timestamp: descriptor.started_at,
  source: "network",
  kind: "request",
  summary: `synthetic ${id}`,
  raw_ref: "network/events.jsonl#L1",
  detail: {},
  clock_uncertainty_ms: null,
  original_timestamp: null,
  received_at: null,
});
const detail = (id: string): EventDetail => ({
  session_key: directory,
  sequence: 1,
  event: event(id),
  detail_bytes: 0,
  detail_complete: true,
  bodies: [],
});
const body = (original: string) =>
  ({
    kind: "text",
    original,
    bytes: original.length,
    basis: "retained",
    retained: { kind: "available", bytes: original.length },
    media: {
      content_type: null,
      content_encoding: null,
      label: "Unspecified media type",
    },
  }) as const;
const pair = (id: string): BodyDocuments => ({
  reference: { session_key: directory, event_id: id },
  request_info: { method: "GET", url: "https://example.invalid/" },
  request_headers:
    id === "two"
      ? { kind: "absent" }
      : { kind: "present", rows: [{ name: "X-Needle", value: "needle" }] },
  response_headers: {
    kind: "present",
    rows: [{ name: "Response-Needle", value: "needle" }],
  },
  request:
    id === "two"
      ? {
          kind: "unavailable",
          retained: { kind: "unavailable", reason: "missing" },
          media: body("").media,
          reason: "missing",
        }
      : body('{"request":"needle"}'),
  response: body('{"response":"needle"}'),
});
const ids = ["one", "two", "three", "four", "five", "six", "seven"];
const allRows = ids.map((id, index) => ({
  sequence: 20 - index,
  event: event(id),
}));
const queries: TimelineQuery[] = [];
function queryPage(query: TimelineQuery): TimelinePage {
  const rows =
    query.source === "logs"
      ? []
      : query.order === "newest_first"
        ? allRows
        : [...allRows].reverse();
  const index = query.seek
    ? rows.findIndex((row) => row.event.id === query.seek?.event_id)
    : Number(query.cursor ?? 0);
  const start =
    query.seek?.relation === "after"
      ? index + 1
      : query.seek?.relation === "before"
        ? Math.max(0, index - 3)
        : index;
  const selected = rows.slice(
    start,
    query.seek?.relation === "before" ? index : start + 3,
  );
  return {
    session_key: directory,
    generation: "g",
    rows: selected,
    next_cursor:
      start + selected.length < rows.length
        ? String(start + selected.length)
        : null,
    has_previous: start > 0,
    has_next: start + selected.length < rows.length,
    total: rows.length,
    matching: rows.length,
    progress: {
      indexed: 7,
      raw_bytes: 100,
      indexed_bytes: 100,
      backlog_bytes: 0,
      complete: true,
      latest_sequence: 20,
    },
  };
}
const row = (id: string) =>
  screen.getByRole("button", { name: new RegExp(`synthetic ${id}$`) });

export async function checkHeadersKeyboard() {
  localStorage.clear();
  queries.length = 0;
  const user = userEvent.setup();
  let delayedQuery: ReturnType<typeof deferred<TimelinePage>> | null = null;
  let delayedFilter: ReturnType<typeof deferred<TimelinePage>> | null = null;
  product.query = async (query) => {
    queries.push(query);
    if (query.source === "logs" && delayedFilter) return delayedFilter.promise;
    if (query.seek?.relation === "after" && delayedQuery)
      return delayedQuery.promise;
    return queryPage(query);
  };
  const loadingDetail = deferred<EventDetail | null>();
  product.detail = async (id) =>
    id === "one" ? loadingDetail.promise : detail(id);
  product.bodyDocuments = async (reference) => pair(reference.event_id);
  const mounted = render(<Timeline inspected={descriptor} />);
  await screen.findByRole("button", { name: /synthetic one$/ });
  await user.click(row("one"));
  assert.equal(
    document.activeElement,
    row("one"),
    "Click previews without leaving the timeline",
  );
  await user.keyboard("{Control>}f{/Control}");
  await act(async () => loadingDetail.resolve(detail("one")));
  const find = await screen.findByRole("textbox", {
    name: "Find in headers, query, request and response",
  });
  await waitFor(() =>
    assert.equal(
      document.activeElement === find,
      true,
      "Find intent survives detail loading",
    ),
  );
  await user.keyboard("{Escape}{Escape}");
  assert.equal(document.activeElement, row("one"));
  await user.keyboard("{Enter}");
  assert.equal(
    document.activeElement,
    screen.getByRole("button", { name: "Close event details" }),
  );
  await user.keyboard("{Escape}");
  assert.equal(document.activeElement, row("one"));

  await user.click(screen.getByRole("button", { name: "Older page" }));
  await screen.findByRole("button", { name: /synthetic five$/ });
  act(() => row("five").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() =>
    assert.equal(document.activeElement === row("six"), true),
  );
  assert.equal(queries.at(-1)?.seek?.event_id, "four");
  assert.equal(queries.at(-1)?.seek?.relation, "at");
  assert.equal(queries.at(-1)?.through_sequence, 20);
  assert.equal(
    screen
      .getByRole("checkbox", { name: "Follow latest" })
      .getAttribute("data-state"),
    "unchecked",
  );
  assert.ok(screen.getByText(/page 2$/));
  await user.keyboard("{ArrowDown}");
  await waitFor(() =>
    assert.equal(document.activeElement === row("seven"), true),
  );
  assert.ok(screen.getByText(/page 3$/));
  const endRequests = queries.length;
  await user.keyboard("{ArrowDown}{ArrowDown}");
  assert.equal(document.activeElement, row("seven"));
  assert.equal(
    queries.length,
    endRequests,
    "Final row never wraps or fetches another page",
  );
  await user.keyboard("{ArrowUp}");
  await waitFor(() =>
    assert.equal(document.activeElement === row("six"), true),
  );
  assert.ok(screen.getByText(/page 2$/));
  await user.click(screen.getByRole("button", { name: "Newer page" }));
  await screen.findByRole("button", { name: /synthetic one$/ });
  assert.ok(screen.getByText(/page 1$/));
  assert.ok(screen.getByText("Selected outside this page"));
  await user.click(screen.getByRole("button", { name: "Older page" }));
  await screen.findByRole("button", { name: /synthetic six$/ });
  delayedQuery = deferred<TimelinePage>();
  act(() => row("six").focus());
  const beforeKeys = queries.length;
  await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
  assert.equal(
    queries.length,
    beforeKeys + 1,
    "Pending edge consumes repeated keys without duplicate requests",
  );
  const abandonedQuery = queries.at(-1)!;
  delayedFilter = deferred<TimelinePage>();
  const filterRequests = queries.length;
  act(() => {
    fireEvent.change(screen.getByLabelText("Source filter"), {
      target: { value: "logs" },
    });
    row("six").focus();
    fireEvent.keyDown(row("six"), { key: "ArrowUp" });
  });
  await waitFor(() => assert.equal(queries.length, filterRequests + 1));
  assert.equal(queries.at(-1)?.source, "logs");
  assert.equal(
    queries.at(-1)?.seek,
    null,
    "Old rendered row cannot seek under a new filter before the refresh accepts its page",
  );
  const filteredQuery = queries.at(-1)!;
  await act(async () => delayedFilter?.resolve(queryPage(filteredQuery)));
  delayedFilter = null;
  await screen.findByText("No matching events");
  await act(async () => delayedQuery?.resolve(queryPage(abandonedQuery)));
  assert.equal(
    screen.queryByRole("button", { name: /synthetic seven$/ }),
    null,
    "Filter change rejects delayed edge reply",
  );
  delayedQuery = null;
  await user.selectOptions(screen.getByLabelText("Source filter"), "");
  await user.click(screen.getByRole("button", { name: "Jump to latest" }));
  await screen.findByRole("button", { name: /synthetic one$/ });

  await user.click(row("one"));
  await screen.findByRole("radio", { name: "Response" });
  await user.click(screen.getByRole("radio", { name: "Request" }));
  await user.click(screen.getByRole("radio", { name: "Original" }));
  await user.click(screen.getByRole("button", { name: "Request headers (1)" }));
  await user.click(screen.getByRole("button", { name: "Find" }));
  const retainedFind = screen.getByRole("textbox", {
    name: "Find in headers, query, request and response",
  });
  await user.type(retainedFind, "needle");
  await waitFor(() =>
    assert.match(screen.getByRole("search").textContent!, /1 of 6 matches/),
  );
  act(() => row("one").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() =>
    assert.equal(document.activeElement === row("two"), true),
  );
  await screen.findByText("Headers were not recorded.");
  await waitFor(() =>
    assert.match(screen.getByRole("search").textContent!, /0 of 3 matches/),
  );
  assert.equal(
    screen.getByRole("textbox", {
      name: "Find in headers, query, request and response",
    }).value,
    "needle",
  );
  assert.ok(
    screen.getByRole("radio", {
      name: "Request (not searchable)",
      checked: true,
    }),
  );
  assert.equal(
    screen
      .getByRole("button", { name: "Request headers" })
      .getAttribute("aria-expanded"),
    "true",
  );
  assert.equal(
    document.activeElement,
    row("two"),
    "Retained query and new document readiness do not steal row focus",
  );
  await user.click(screen.getByRole("button", { name: "Next match" }));
  await screen.findByRole("table", { name: "Response header fields" });
  assert.ok(screen.getByRole("radio", { name: "Response (3)", checked: true }));
  assert.ok(screen.getByRole("radio", { name: "Original", checked: true }));
  await user.click(screen.getByRole("radio", { name: "Formatted" }));
  const checked = screen.getByRole("radio", {
    name: "Formatted",
    checked: true,
  });
  await user.keyboard("{ArrowUp}");
  assert.equal(
    row("two").getAttribute("aria-pressed"),
    "true",
    "ToggleGroup owns its arrow keys",
  );
  assert.notEqual(document.activeElement, row("three"));
  act(() => checked.focus());
  await user.click(
    screen.getByRole("button", { name: "Choose body copy format" }),
  );
  await screen.findByRole("menu");
  await user.keyboard("{Escape}");
  assert.ok(screen.getByRole("search"));
  await user.keyboard("{Escape}");
  assert.equal(screen.queryByRole("search"), null);
  await user.keyboard("{Escape}");
  assert.equal(screen.queryByRole("dialog"), null);
  assert.equal(document.activeElement, row("two"));
  mounted.rerender(
    <Timeline
      inspected={{ ...descriptor, directory: "/synthetic/new-session" }}
    />,
  );
  await screen.findByRole("button", { name: /synthetic one$/ });
  await user.click(row("one"));
  await screen.findByRole("radio", { name: "Response", checked: true });
  assert.equal(
    screen.queryByRole("search"),
    null,
    "New session resets Find preferences",
  );
  assert.equal(
    screen
      .getByRole("button", { name: "Response headers (1)" })
      .getAttribute("aria-expanded"),
    "false",
  );
  cleanup();
  await checkPendingIntents();
  await checkHeaderWindows();
  console.log(
    "Headers and keyboard DOM checks passed. Loading-time Find, retained preferences, row focus, later-page freeze, both manual directions, endpoints, duplicate edges, filter invalidation, menu and Escape ownership, and bounded complete header reveal.",
  );
}

async function checkPendingIntents() {
  localStorage.clear();
  const user = userEvent.setup();
  const oldBody = deferred<BodyDocuments>();
  let delayed: ReturnType<typeof deferred<TimelinePage>> | null = null;
  let held: TimelineQuery | null = null;
  product.query = async (query) => {
    queries.push(query);
    if (query.seek && delayed) {
      held = query;
      return delayed.promise;
    }
    return queryPage(query);
  };
  product.detail = async (id) => detail(id);
  product.bodyDocuments = async (reference) =>
    reference.event_id === "one" ? oldBody.promise : pair(reference.event_id);
  const mounted = render(<Timeline inspected={descriptor} />);
  await screen.findByRole("button", { name: /synthetic one$/ });
  await user.click(row("one"));
  await screen.findByText("Reading complete request and response bodies…");
  await user.keyboard("{Meta>}f{/Meta}");
  act(() => row("one").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() =>
    assert.equal(document.activeElement === row("two"), true),
  );
  await act(async () => oldBody.resolve(pair("one")));
  await screen.findByRole("textbox", {
    name: "Find in headers, query, request and response",
  });
  assert.equal(
    document.activeElement,
    row("two"),
    "Later row arrow cancels loading-time Find intent and disposed reply",
  );
  await user.keyboard("{Meta>}f{/Meta}");
  await waitFor(() =>
    assert.equal(
      document.activeElement?.getAttribute("aria-label"),
      "Find in headers, query, request and response",
    ),
  );
  await user.keyboard("{Escape}");
  await user.click(screen.getByRole("button", { name: "Jump to latest" }));
  await screen.findByRole("button", { name: /synthetic one$/ });
  delayed = deferred<TimelinePage>();
  act(() => row("two").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() => assert.ok(held));
  const oldOrder = held!;
  await user.selectOptions(
    screen.getByLabelText("Timeline order"),
    "oldest_first",
  );
  await screen.findByRole("button", { name: /synthetic seven$/ });
  const pendingOrder = delayed;
  delayed = null;
  await act(async () => pendingOrder.resolve(queryPage(oldOrder)));
  assert.equal(
    screen.queryByRole("button", { name: /synthetic one$/ }),
    null,
    "Order change rejects old frozen query",
  );
  delayed = deferred<TimelinePage>();
  held = null;
  act(() => row("six").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() => assert.ok(held));
  const hiddenQuery = held!;
  mounted.rerender(<Timeline inspected={descriptor} visible={false} />);
  const outside = render(<button>Outside timeline</button>);
  await user.click(screen.getByRole("button", { name: "Outside timeline" }));
  const pendingHidden = delayed;
  delayed = null;
  await act(async () => pendingHidden.resolve(queryPage(hiddenQuery)));
  assert.equal(
    document.activeElement?.textContent,
    "Outside timeline",
    "Hidden timeline cannot consume pending movement focus",
  );
  mounted.rerender(<Timeline inspected={descriptor} />);
  await screen.findByRole("button", { name: /synthetic seven$/ });
  assert.equal(document.activeElement?.textContent, "Outside timeline");
  outside.unmount();
  delayed = deferred<TimelinePage>();
  held = null;
  act(() => row("six").focus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() => assert.ok(held));
  const crossSession = held!;
  mounted.rerender(
    <Timeline
      inspected={{ ...descriptor, directory: "/synthetic/next-session" }}
    />,
  );
  await screen.findByRole("button", { name: /synthetic seven$/ });
  const pendingSession = delayed;
  delayed = null;
  await act(async () => pendingSession.resolve(queryPage(crossSession)));
  assert.equal(
    screen.queryByRole("dialog"),
    null,
    "Session change rejects queued selection",
  );
  cleanup();
}

async function checkHeaderWindows() {
  const user = userEvent.setup();
  const documents = pair("long");
  documents.request_headers = {
    kind: "present",
    rows: Array.from({ length: 120 }, (_, index) => ({
      name: "X-Duplicate",
      value: index === 119 ? "x".repeat(100000) + "tail-needle" : "same",
    })),
  };
  documents.response_headers = { kind: "present", rows: [] };
  product.bodyDocuments = async () => documents;
  render(<Details detail={detail("long")} offPage={false} wrap />);
  await screen.findByRole("button", { name: "Response headers (0)" });
  await user.click(
    screen.getByRole("button", { name: "Response headers (0)" }),
  );
  assert.ok(await screen.findByText("Recorded empty header list."));
  await user.keyboard("{Meta>}f{/Meta}");
  const input = screen.getByRole("textbox", {
    name: "Find in headers, query, request and response",
  });
  await user.type(input, "tail-needle");
  const content = await screen.findByLabelText("Request header value");
  await waitFor(() => assert.match(content.textContent!, /tail-needle/));
  assert.ok(content.textContent!.length <= 16384);
  assert.ok(content.querySelector("mark.bg-primary"));
  assert.equal(document.activeElement, input);
  const table = screen.getByRole("table", { name: "Request header fields" });
  assert.equal(within(table).getAllByRole("row").length, 21);
  assert.ok(screen.getByText("Rows 101–120 of 120"));
  await user.click(
    within(
      screen.getByRole("navigation", { name: "Request header rows" }),
    ).getByRole("button", { name: "Previous rows" }),
  );
  await screen.findByText("Rows 51–100 of 120");
  assert.equal(within(table).getAllByRole("row").length, 51);
  cleanup();
}
