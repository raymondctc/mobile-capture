# Finder tool discovery

Issue12 was created before the fix branch. Base is c5535eb, the Tauri network PoC. No shipping Swift/Python code changes.

## Observed trigger

Native Refresh reproduces the user's ADB spawn error. Running GUI PID94017 has PATH=/usr/bin:/bin:/usr/sbin:/sbin and no ANDROID_HOME/ANDROID_SDK_ROOT. Installed ~/Library/Android/sdk/platform-tools/adb reports version37.0.1 when invoked absolutely under that PATH. Existing /opt/homebrew/bin/mitmdump is also outside the GUI PATH. The adapter selects bare adb and searches PATH only for mitmdump.

## Boundary

Resolve ordinary installed tools once at desktop setup. CaptureOwner continues to consume ToolSelection and pass the same ADB path to discovery, log workers and the network guardian. Keep fixture ADB and explicit engine overrides authoritative. Preserve existing graceful missing-ADB UI behavior and core absolute recovery validation. Do not change shell profiles, global PATH, device routing, certificate trust, or tool versions.

## Candidate rubric

Score each candidate against these five criteria before selection.

1. Finds actual standard SDK ADB and installed mitmdump under the observed GUI environment.
2. Supplies one selected absolute executable to every downstream consumer while preserving fixture/override semantics.
3. Preserves configured PATH priority and supports documented SDK variables and Windows executable naming/default paths.
4. Rejects invalid candidate files and keeps missing-tool behavior graceful without global environment races in tests.
5. Keeps the interface and blast radius small, with a focused regression that runs in normal verification.

## Documentation

Three Context7 commands were used for this question. The initial Android SDK name returned unrelated vendor SDKs. Resolving Android Developers found the official Android Studio documentation. Its environment-variable page documents ANDROID_HOME, deprecated ANDROID_SDK_ROOT and the macOS SDK location. Official emulator archive documentation names standard Windows/macOS/Linux SDK directories. These are candidate paths, not a claim that every machine uses them. No installation, quarantine or other mutation instructions in the retrieved material are part of this fix.

Sources: https://developer.android.com/studio/command-line/variables and https://developer.android.com/studio/emulator_archive .

## Verification

Land a focused failing installed-SDK/Finder-PATH regression before its fix. Test pure discovery against synthetic path/environment snapshots without global env changes. Check relevant desktop code with Rust1.99, build the app, then verify Refresh without injecting developer PATH. Discover devices only; do not start a real recording. Open a narrow follow-up PR stacked on11. The existing core/network suite is unchanged. Focused resolver checks and the release app build target this fix; the new CI runs the existing broad matrix. Earlier network results do not count as execution of this new adapter.

## Synthesis accepted before implementation

Use the boundary candidate at /private/tmp/tauri-adb-design-boundary.md. Parent scores it5/5 on each rubric criterion. The environment candidate is4/4/3/4/3: immutable child PATH still needs separate parent-side absolute ADB resolution and spreads installation policy into subprocess/recovery APIs. The independent GPT6Luna cross-judge at /private/tmp/tauri-adb-design-judge.md agrees. Graft its concise explanation of why child-only PATH cannot satisfy pre-launch absolute resolution; no code graft or core API change is needed.

The selected data shape is a private immutable DiscoveryEnv plus two tool-specific lookups. Keep PATH first, then SDK environment/default candidates for ADB and known executable directories. Canonicalize only regular executable files. A missing ADB returns None and preserves the existing adapter bare-name fallback, keeping the UI usable. Fixture ADB and the explicit mitmdump override remain unchanged. Do not add a DesktopOwner state, settings UI, persistent config or a new dependency.

Keep the module std-only. Its regression harness runs via rustc --test from the existing npm verify script so both platform jobs execute it without a Tauri test build. The release app build validates adapter integration. Core routing, process ownership and evidence code are outside the fix. No global environment mutation is accepted, including in tests.
