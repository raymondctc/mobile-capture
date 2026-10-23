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
4. Enter a task title, notes, and reproduction steps if needed.
5. Review **Capture limits** and save changes before recording.
6. Accept each applicable consent disclosure and choose **Start recording**.

Shared UID apps cannot be isolated. An app log recording checks identity before writing logs. If you choose whole-device logs, consent applies to that recording. Whole-device network capture has a separate disclosure. Stopping or changing the selected device clears both consent choices.

Task context and display choices persist for the next recording. Source and policy changes apply to a new recording. Log limits and network limits are independent. Reaching a log limit does not stop network recording. When the network admission duration or budget ends, forwarding continues until you stop and restore routing.

During a recording, add a marker to describe a reproduction step. If adding a marker fails, its text stays in the field. Closing the window keeps a normal recording alive. Reopen Mobile Capture Android to return.

## Inspect complete evidence

Open **Saved sessions** to choose a capture in the configured folder. Use **Open folder** for another saved session. Opening evidence does not replace the active capture controls.

Search covers the complete indexed session, including full details and decoded body text. Select a source to include network events, logs, or markers. The indexing indicator shows any remaining backlog. Results can grow as indexing catches up.

Use the reading controls as follows.

- **Pause view** freezes the visible sequence while recording and indexing continue.
- **Follow latest** keeps the newest page visible. Scrolling away turns following off.
- **Jump to latest** returns to the newest page with the current search and source filter.
- **Clear view** shows subsequent events. It preserves the saved evidence.
- **Full session** removes the clear and pause boundaries.
- The order control and page buttons let you read earlier or later evidence.
- **Wrap lines** changes presentation without altering retained bytes.

If a page cursor becomes invalid, the workspace reports the reason and reloads the first page with your filters preserved. A selected event stays open when it is outside the current page.

Select an event to read full metadata, retained request bodies, or retained response bodies. Content loads in bounded 64 KiB chunks. Use the chunk buttons to read the complete retained content. Text uses UTF-8. Binary content has a hexadecimal view. Compressed bodies offer a decoded view when safe decoding is available. Original retained body bytes remain separate from decoded content. Unavailable bodies and decoding limits have explicit reasons.

Use **Reveal raw source** to locate the admitted source file for the selected event. Read-only inspection never changes raw evidence.

## Stop, recover, and save

Choose **Stop & Save** to restore routing, verify source shutdown, and publish an archive. Progress shows the current stage. **Cancel save** is available before publication. An accepted cancellation preserves raw evidence and any previous archive. After a save failure, use **Retry save**.

Use **Stop without archive** if you want to finish capture and inspect before saving. Choose **Save archive** for the inspected stopped session. **Copy archive to…** opens a native destination dialog. **Reveal archive** locates the published file.

If routing recovery is required, reconnect the original phone and choose **Retry safe recovery**. Export and new recordings remain blocked until restoration and shutdown are verified. Do not change the phone's proxy configuration while recovery owns it. Verified partial evidence can retain coverage gaps, unavailable bodies, interruptions, and source limits. Review those facts and the raw evidence before sharing an archive.

Quitting waits for safe shutdown and routing restoration. Unresolved recovery prevents exit. Updates use manual installation. Finish capture, save, setup, and recovery before choosing **Check for updates** in Settings.

## Run the desktop workspace in development

Install Node 22, Rust, and the platform's Tauri build prerequisites. Prepare the packaged Android tools as described in [Android distribution](ANDROID-DISTRIBUTION.md), or deliberately configure supported absolute development overrides.

```sh
cd TauriPoC
npm ci
npm run dev
```

On the development Mac, prepend `/opt/homebrew/opt/node@22/bin` to `PATH` if Node 22 is not the default.

Build the frontend and run its behavior checks with these commands.

```sh
npm run web:build
node scripts/test-ui.mjs
```

The frontend checks exercise consent defaults, UTF-8 context budgets, operation schemas, routing admission, setup freshness, reading boundaries, and chunk decoding. They do not certify native dialogs, packaged lifecycle behavior, phone trust, device restoration, or Windows hardware support. Use the native fixture and installed-package verification described in [Android distribution](ANDROID-DISTRIBUTION.md) for those separate checks. Physical phone acceptance still requires supported Android hardware.
