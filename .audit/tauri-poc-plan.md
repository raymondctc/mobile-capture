# Tauri proof of concept plan

## Definition of done

The PoC launches as a real Tauri desktop application. With a compiled fake ADB executable, the real Rust backend discovers an explicitly labelled fixture device, requires explicit whole-device scope, records raw log bytes, displays live timeline rows, adds a marker, stops its owned collector, and exports a schema 1 ZIP. A repeatable command checks raw bytes, event identities, line references, checksums, partial outcomes, and shutdown. Windows CI builds and exercises the same portable code. Local macOS evidence is separate from Windows and phone hardware acceptance.

The shipping Swift application and release configuration remain intact. The PoC has its own application identity and session root. It does not modify device routing, certificates, or trust. App scope, live network capture, iPhone capture, and production updates remain unavailable.

## Rigor

This is a reversible PoC with a narrow feature scope. Process ownership and evidence truth receive independent design and review because a working interface can otherwise hide incomplete shutdown or incorrect completeness claims. Real phones are unnecessary for this checkpoint.

## Designed phases

1. Record the current evidence and lifecycle contracts. Run the existing capture and release Python suites as a baseline.
2. Compare two architecture candidates and obtain an independent cross-judge verdict. Reconcile the chosen ownership and wire contracts before implementation.
3. Build a compiled fake ADB and a repeatable verification command before capture behavior. Keep fixture mode explicit. Add Windows CI for portable fixtures and a native build.
4. Implement bounded Android log capture, stop, disconnect, parent closure, markers, and schema 1 export in one Rust ownership model.
5. Bind the Rust API to a compact React and TypeScript interface through Tauri commands. Drive the native app with the compiled fixture tool.
6. Inspect the diff, remove redundant comments and abstractions, obtain independent correctness review, and check the decision trail.
7. Publish the branch and open a pull request linked to issue #7. Report observed results and unobserved acceptance separately.

## Design rubric

Score each candidate from 0 to 2 on each criterion.

| Criterion | Passing evidence |
| --- | --- |
| Process ownership | Parent closure, repeated Stop, disconnect, and child wait have one owner on both platforms. |
| Interface depth | The UI asks for capture outcomes without coordinating private stages. The shape avoids a second controller where a worker is enough. |
| Evidence truth | Raw bytes, stable IDs, references, limits, and partial outcomes match the existing package contract. |
| Useful PoC | Discovery, recording, timeline, markers, and export run through real code. Unsupported capabilities are explicit. |
| Delivery cost | The implementation remains isolated and has a repeatable macOS and Windows verification path. |

## Throughput checkpoint

Blocking first steps are the issue, branch, grounding, baseline, candidate comparison, and agreed data types. Architecture candidates use separate temporary files. One code owner works in an isolated branch and worktree because capture, shutdown, and export share lifecycle invariants. The coordinator alone owns the audit trail and delivery branch. Verification and review use immutable commits after the writer stops.

## Baseline

`bash Scripts/test-all.sh python` passed before implementation. The capture suite ran 68 tests with one optional test skipped. The release suite ran 75 tests with two optional tests skipped. The complete output is `/private/tmp/mobile-capture-tauri-baseline.log`.
