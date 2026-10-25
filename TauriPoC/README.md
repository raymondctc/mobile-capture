# Record Android evidence with Mobile Capture

Use Mobile Capture Android to record network traffic, app logs, or both on macOS and Windows. The desktop workspace opens saved evidence without capture tools. For installation and packaged dependencies, read [Android distribution](ANDROID-DISTRIBUTION.md).

## Connect and prepare a phone

1. Connect your Android phone with USB and unlock it.
2. Enable Developer options and USB debugging in Android Settings.
3. Approve this computer on the phone.
4. Open **Device setup**, refresh the devices, and explicitly select your phone.
5. Run **Host Doctor** to check the packaged tools and capture storage.

Doctor checks this computer. A ready result does not prove phone traffic or certificate trust. If a packaged dependency is damaged, replace the app with the official Android release. Use **Settings** to choose a writable capture folder or an explicit absolute tool override.

For HTTPS capture, continue through **Prepare** and **Check**.

1. Read and accept the setup network disclosure.
2. Choose **Copy public certificate to phone**. Wait for routing restoration.
3. On Android, open Settings and search for "Install a certificate" or "CA certificate".
4. Select the public certificate in Downloads. Complete Android's manual installation steps.
5. Compare the displayed public certificate fingerprint where Android provides it.
6. Start the 45-second connection check.
7. Open the check URL in the selected phone's browser. **Open on phone** opens that same URL through ADB.
8. Confirm that you opened the URL on that phone.
9. Finish the check and wait for routing restoration, or let its timer expire.

A successful check requires the exact fresh HTTPS response and your phone-origin confirmation. A host request, a TLS error, or an installation acknowledgment cannot pass. A different device, a changed certificate, or an expired result requires another check. The browser check does not prove that every app trusts user certificates. Apps with certificate pinning may reject interception.

Choose **Export readiness report** to save the current public host and setup facts. The report excludes captured bodies, logs, private keys, and routing files. **Set up later** leaves the phone unverified and keeps setup available in the toolbar.

## Record a task

1. Select a ready Android device. The app retains an unavailable selection so that reconnecting cannot silently target another phone.
2. Choose **Network only**, **Logs only**, or **Logs and network**. Network only is the initial default.
3. For logs, keep **Selected app** and choose **Refresh apps**. Search the installed apps, or enter the application ID manually.
4. Open **Task context (optional)** to add a title, notes, or reproduction steps if needed.
5. Review **Capture limits** and save changes before recording.
6. Review the notice beside **Start recording** in the upper right. For whole-device logs, accept the separate log consent. Choose **Start recording**.

Shared UID apps cannot be isolated. An app log recording checks identity before writing logs. Choosing **Start recording** authorizes whole-device network capture and temporary proxy routing for that recording when network is selected. HTTPS requires manual CA trust. Whole-device log consent remains separate and clears when recording stops or the selected device changes.

Task context and display choices persist for the next recording. Source and policy changes apply to a new recording. Log limits and network limits are independent. Reaching a log limit does not stop network recording. When the network admission duration or budget ends, forwarding continues until you stop and restore routing.

During a recording, add a marker to describe a reproduction step. If adding a marker fails, its text stays in the field. Closing the window keeps a normal recording alive. Reopen Mobile Capture Android to return.

## Inspect complete evidence

Open **Saved sessions** to choose a capture in the configured folder. Use **Open folder** for another saved session. Opening evidence does not replace the active capture controls.

Search covers the complete indexed session, including full details and decoded body text. Select a source to include network events, logs, or markers. **Log level** filters logs by their exact severity and keeps network events and markers visible. The indexing indicator shows any remaining backlog. Results can grow as indexing catches up.

Use the reading controls as follows.

- **Pause view** freezes the visible sequence while recording and indexing continue.
- **Follow latest** keeps the newest page visible. Scrolling away turns following off.
- **Jump to latest** returns to the newest page with the current search and source filter.
- **Clear view** shows subsequent events. It preserves the saved evidence.
- **Full session** removes the clear and pause boundaries.
- The order control and page buttons let you read earlier or later evidence.
- **Wrap lines** changes presentation without altering retained bytes.

If a page cursor becomes invalid, the workspace reports the reason and reloads the first page with your filters preserved. A selected event stays open when it is outside the current page.

Select an event to open the inspector on the right. Choose **Close event details** or press Escape inside the inspector to return to the timeline. The recording controls stay in the upper right.

Network events open **Response** when retained, including an empty response. **Request** is the fallback when the response was not recorded. Supported content encodings decode automatically. Valid JSON opens formatted. Formatting preserves number spellings, duplicate keys, key order, and string escapes. **Original** shows the exact unformatted decoded UTF-8 text. If formatting would exceed 64 MiB, the inspector shows the complete original with a reason.

**Copy original** and **Copy formatted** copy the complete body through the native clipboard. **Find**, Command+F, or Control+F searches both complete retained text bodies, including content outside the visible window. Find is literal and case insensitive. Counts appear for both tabs. Enter and Shift+Enter move between matches and switch tabs, with wraparound. Escape closes Find before the inspector.

The body view shows bounded character windows, including long single lines. Use **Previous window** and **Next window** to read the rest. Full-body Find and copy cover the complete text independently of this window. Binary bodies, unavailable content, decoding failures, and copy failures have visible reasons. **Retained bytes · hexadecimal** keeps original compressed or binary bytes accessible in 64 KiB chunks. **Full metadata and headers** provides the complete chunked metadata view.

Logs show severity and the complete message first. Long messages use the same bounded windows. Expand **Log metadata** for the tag, PID, and TID. Timeline rows label HTTP method, status, and recorded duration, or log severity, tag, and message. Duration is the request-start to response-end interval when valid collector timestamps are available. Older sessions and incomplete timing can have no duration.

The outlined **Reveal raw source** button stays in the inspector footer while content scrolls. Use it to locate the admitted source file for the selected event. Read-only inspection never changes raw evidence.

## Stop, recover, and save

Choose **Stop & Save** to restore routing, verify source shutdown, and publish an archive. Progress shows the current stage. **Cancel save** is available before publication. An accepted cancellation preserves raw evidence and any previous archive. After a save failure, use **Retry save**.

Use **Stop without archive** if you want to finish capture and inspect before saving. Choose **Save archive** for the inspected stopped session. **Copy archive to…** opens a native destination dialog. **Reveal archive** locates the published file.

If routing recovery is required, reconnect the original phone and choose **Retry safe recovery**. Export and new recordings remain blocked until restoration and shutdown are verified. Do not change the phone's proxy configuration while recovery owns it. Verified partial evidence can retain coverage gaps, unavailable bodies, interruptions, and source limits. Review those facts and the raw evidence before sharing an archive.

Quitting waits for safe shutdown and routing restoration. Unresolved recovery prevents exit. The app checks for Android desktop updates quietly at startup. Settings provides an explicit download and **Install and restart** action. Downloads can run during capture. Installation waits until capture has stopped, its archive is saved, and setup or recovery has finished. See [publishing Android updates](../docs/ANDROID_UPDATES.md) for the signed release pipeline and publisher prerequisites.

## Run the desktop workspace in development

Install Node 22, Rust, and the platform's Tauri build prerequisites. Prepare the packaged Android tools as described in [Android distribution](ANDROID-DISTRIBUTION.md), or deliberately configure supported absolute development overrides.

```sh
cd TauriPoC
npm ci
MOBILE_CAPTURE_EXTERNAL_TOOLS=1 npm run dev
```

Development mode requires installed ADB and mitmdump, or explicit absolute overrides after the app opens. The environment flag deliberately selects external tools. Complete distributed apps use their verified bundled payload. On Windows PowerShell, set `$env:MOBILE_CAPTURE_EXTERNAL_TOOLS = "1"` before `npm run dev`.

On the development Mac, prepend `/opt/homebrew/opt/node@22/bin` to `PATH` if Node 22 is not the default.

Build the frontend and run its behavior checks with these commands.

```sh
npm run web:build
node scripts/test-ui.mjs
```

The frontend checks exercise consent defaults, UTF-8 context budgets, operation schemas, routing admission, setup freshness, reading boundaries, and chunk decoding. They do not certify native dialogs, packaged lifecycle behavior, phone trust, device restoration, or Windows hardware support. Use the native fixture and installed-package verification described in [Android distribution](ANDROID-DISTRIBUTION.md) for those separate checks. Physical phone acceptance still requires supported Android hardware.
