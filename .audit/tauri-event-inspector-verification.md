# Event inspector and timeline verification

The reviewed implementation is `0b6b87cda5930c6dde93ce444bf5d0b07814d305`, based on `4543db908541646679542d79c53c366d985743b8`.

## Automated checks

`npm run web:build` and `node scripts/test-ui.mjs` exited successfully on the final implementation. These include the real module-worker protocol, lexical JSON fidelity, complete two-body Find and copy, large match counts, bounded windows, stale replies, clipboard errors, body availability, log messages, level filters, and keyboard focus.

`npm run verify` exited successfully outside the sandbox. Its native source was `d392af4`, which is unchanged through the reviewed implementation. The run passed 72 core unit tests, 23 capture integration tests, 36 network integration tests, five setup integration tests, four Unix process-group tests, two doctor tests, one fixture test, and 22 Tauri library tests. One existing core test was ignored. Titlebar checks and exported evidence/checksum verification passed. The initial sandbox run could not bind a local test socket; the outside-sandbox run resolved that environment restriction.

A real mitmdump 12.2.3 probe checked 32 response durations against retained native request-start and response-end timestamps. Request events and older evidence without timing do not receive invented durations.

Independent review found and verified fixes for escaped preview size budgets, empty gzip HEAD responses, and Find focus through hidden panes and pending loads. The final independent source review passed at the implementation commit above.

## Native macOS acceptance

Run `python3 .audit/tauri-event-inspector-fixture.py` to recreate the two synthetic saved sessions under `/private/tmp`. Launch the app through `node scripts/fixture.mjs` and use **Open folder** to inspect each session. The acceptance run used the final debug executable in a distinct local app wrapper with the development frontend, so automation could select it among older installed copies. This is development fixture verification, not an installed release or Windows hardware certification.

The native acceptance run verified these behaviors.

- Response opens first with formatted JSON. Large integers, duplicate keys, numeric-looking key order, and exponent spelling remain intact.
- Cmd+F searches both bodies. Forward and reverse navigation changes tabs. Search finds a response-tail keyword after character 69,830, beyond the initial window.
- Native Copy original and Copy formatted reach the macOS clipboard. Pasting back into Find shows the complete lexical text and the expected formatting difference.
- Cmd+F returns from metadata and retained bytes to the visible input with focus. Escape closes visible Find while retaining the inspector. Settings remains outside the inspector shortcut scope.
- Supported gzip content decodes automatically. An empty gzip HEAD response stays selected as an empty response. Missing responses fall back to the request with an explicit reason.
- Error-level filtering returns nine of the 14 mixed events, preserving all seven network events and the marker. Log details show only level and message initially, with metadata collapsed.
- Selected body and format controls are visible. Short content omits paging controls. The outlined raw-source action stays in the footer.
- The dense fixture reports 2,097,152 complete matches, split equally across request and response. Reverse wrap reaches the final response match and its tail window.

Both fixture directories remained byte-for-byte unchanged after inspection. Their SHA-256 digests were `b5b3465f642b9e25e4e214b898c04016fd81423593c1649755308755475dcaf4` and `844eef35891259bf8ed9485632210fba731fbad0c6d87076a083e786681005ca`.

## Design choices

Model the Domain keeps text, binary, empty, unavailable, loading, and error outcomes explicit. One native paired read avoids repeatedly parsing and decompressing a full record for each body window. A worker owns complete text formatting and search; bounded windows keep rendering separate from whole-body results and copying.

Separate Before Serializing Shared State gave the implementation owner an isolated worktree. Root drove native acceptance after explicit handoff. Test Behavior, Not Implementation guided the interaction and raw-evidence tests. Prove It Works required the real native clipboard, keyboard, worker, footer, and immutable-fixture checks above.

The architecture comparison chose the paired body read over a native document-cache/search service. A correctness-checked dense-match stress probe justified the worker. No end-to-end speedup is claimed.
