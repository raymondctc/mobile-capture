# Packaged tool discovery verification

[Issue12](https://github.com/raymondctc/mobile-capture/issues/12) was filed before the fix branch. [PR13](https://github.com/raymondctc/mobile-capture/pull/13) is stacked on11. Tested source is `806ca2f18abe022b0102cc3c9d3847b8893a12b1`, following regression commit `62010f5ba744c2cb9538fa1ee8a7ba6c664bbdc4`.

## Reproduction and focused checks

The coordinator reproduced the user's missing-ADB error through native Refresh. That running app had `PATH=/usr/bin:/bin:/usr/sbin:/sbin` and no SDK environment variables. The installed `~/Library/Android/sdk/platform-tools/adb` returned version37.0.1 when addressed absolutely under the restricted PATH. The Tauri adapter selected bare `adb`; its mitmdump lookup also searched only PATH.

The committed `finder_path_finds_home_android_sdk` regression compiled, then failed with `left: None` versus the expected canonical SDK executable. `/private/tmp/tauri-adb-red.log` preserves that failure. The corrected resolver passes eight macOS filesystem tests. `/private/tmp/tauri-adb-green.log` and `/private/tmp/tauri-adb-verify-focused.log` cover the standalone harness and execution of the exact new verification-script block. Tests use synthetic environments and temporary files; they do not mutate process environment or execute fake tools. The normal verification script invokes this harness on both existing platform jobs.

The coordinator compiled `/private/tmp/tauri-adb-host-probe.rs` against the actual final resolver module and ran it with only the restricted PATH and `HOME=/Users/raymond`. It selected the actual SDK ADB and the installed mitmdump12.2.3 executable in the Homebrew cask. It did not execute either tool. Output is `/private/tmp/tauri-adb-host-probe.log`. Formatting and resolver Clippy pass in `/private/tmp/tauri-adb-integrated-{fmt,clippy}.log`. This is a tool-location proof, not a capture or a full application-environment proof.

## Independent review

`/private/tmp/tauri-adb-correctness-review.md` independently records hashes of all four changed files, a private-copy eight-test run, script syntax validation and review of selected-path propagation, fixture/override precedence, fallback and CI wiring. It found no actionable issues. `/private/tmp/tauri-adb-no-comments.md` found zero comment or suppression flags. Both reviewers and the author were explicitly stopped before cleanup. The clean author checkout was removed after preserving its branch, commits and logs.

## Native build and launch boundary

The final macOS release app built successfully at `/Users/raymond/workspace/mobile-capture/TauriPoC/target/release/bundle/macos/Mobile Capture PoC.app`. Build output is `/private/tmp/tauri-adb-native-build.log`. CUA opened it and its Device list showed the connected Pixel4a with no ADB error, zero evidence events, unselected consent and disabled Start. No capture was started.

The coordinator checked the running environment rather than assuming the launch matched Finder. CUA's direct launch inherited developer PATH and ANDROID_HOME. `/private/tmp/tauri-adb-native-identity.json` records that observation, its explicit limitation and all three packaged binary hashes. That native success is excluded from exact restricted-Finder acceptance. The app was closed through CUA. Two attempts to select Finder for a matching launch timed out; a later system-app selection was also unusually slow. No shell or AppleScript UI workaround was used. The user was asked to reopen the concrete rebuilt app through Finder. Confirmation was pending at this checkpoint and is recorded below.

## Limits and delivery

Windows-specific lookup is source-reviewed and wired into the new [platform CI run](https://github.com/raymondctc/mobile-capture/actions/runs/39502749803); it has not completed at this checkpoint. Its focused test module also compiles for the Windows GNU target with Rust1.99 metadata-only output; `/private/tmp/tauri-adb-windows-compile.log` records that check. Compilation is not Windows execution. The unchanged core/network suite was not repeated locally. Its prior platform results do not count as execution of this new adapter. Exact Finder reopen confirmation is a separate gate, closed below. Do not infer either platform execution or matching launch behavior from the CUA developer-environment launch.

This change touches the desktop resolver, its setup call site, focused verification and documentation. It changes no shipping Swift/Python code, core collection/routing/recovery/evidence logic, shell profile, global PATH, tool installation, real-device routing or certificate trust. The final evidence commit must preserve the reviewed product source.

## Final Finder acceptance

The user confirmed that reopening through Finder lists the Pixel4a. The coordinator then inspected that actual running app before attaching CUA. PID50753 had the original restricted `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME=/Users/raymond`, no Android SDK environment variables, no fixture flag and no mitmdump override. `/private/tmp/tauri-adb-manual-reopen-environment.json` preserves only those selected environment facts. This is the original failing launch condition, not the earlier developer-environment launch.

CUA attached to the already running app and observed `Ready to capture`, `Pixel 4a · ready`, no ADB error, no evidence and unselected capture consent. Clicking Refresh kept that successful state. The matching native gate now passes without starting a capture or changing routing/trust. The app remains open for the user.

The fresh delivery audit at `/private/tmp/tauri-adb-delivery-audit.md` accepted the earlier source/evidence record and flagged Finder confirmation and Windows CI as open. The evidence above closes its Finder flag. The plan now removes its unsupported earlier-platform assertion rather than treating prior network runs as proof of this adapter. Windows CI remains pending. All author, design and review agents were explicitly stopped.
