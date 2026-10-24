# Publish Android desktop updates

The Android desktop app checks the Android-only feed once at startup. A user chooses Download, then Install and restart. Downloads can run during capture. Installation requires a stopped capture with an existing published archive, or an idle capture owner. Saving, device setup, recovery, archive copying and readiness export exclude installation. A blocked installation keeps its verified download for retry. The app never stops or saves capture to force an update. If a failed update cannot restore the original application, capture remains blocked and the app explains how to restore its retained backup or reinstall. Quit remains available in that repair state.

Development builds without publisher configuration show that in-app updates are unavailable. Release packaging fails before building when publisher configuration is absent or mismatched.

## Provision the publisher

Create a protected GitHub environment named `android-release`. Require approval for tagged publication if your release policy needs it. Provision these environment secrets through your normal credential process.

| Secret | Value |
| --- | --- |
| `ANDROID_UPDATER_PRIVATE_KEY` | Tauri-generated private key file contents |
| `ANDROID_UPDATER_PRIVATE_KEY_PASSWORD` | That key's password |
| `ANDROID_APPLE_CERTIFICATE_BASE64` | Base64 Developer ID Application certificate and private key in a P12 file |
| `ANDROID_APPLE_CERTIFICATE_PASSWORD` | P12 password |
| `ANDROID_APPLE_SIGNING_IDENTITY` | Developer ID Application signing identity |
| `ANDROID_APPLE_NOTARY_KEY_BASE64` | Base64 App Store Connect notarization P8 key |
| `ANDROID_APPLE_NOTARY_KEY_ID` | Notarization key ID |
| `ANDROID_APPLE_NOTARY_ISSUER` | Notarization issuer ID |
| `ANDROID_WINDOWS_PFX_BASE64` | Base64 Authenticode certificate and private key in a PFX file |
| `ANDROID_WINDOWS_PFX_PASSWORD` | PFX password |

Create environment variable `ANDROID_UPDATER_PUBLIC_KEY` with the complete base64 contents of the matching Tauri `.pub` file. The public key is embedded in the native application through `MOBILE_CAPTURE_UPDATER_PUBLIC_KEY`. Keep the private key out of the repository. Back it up in the publisher's credential store. Replacing it requires a separately planned trust transition for existing installations.

An authorized publisher can generate the initial key on a trusted machine from `TauriPoC`.

```sh
npx tauri signer generate --write-keys /secure/location/android-updater.key
```

Copy the resulting private key and public key into the corresponding GitHub secret and variable. The workflow signs a temporary proof and verifies it with the public key before building either platform. It refuses an absent, malformed or mismatched key.

No publisher credentials or environment were available during implementation. Provisioning them and validating a real signed two-version upgrade remain deployment prerequisites.

## Prepare a version

From `TauriPoC`, run the version command with the intended stable version.

```sh
python3 scripts/set-android-version.py 0.2.0
npm run web:build
npm run test:delivery
npm run verify
```

The version command updates npm, both Cargo packages and lockfiles, and Tauri configuration together. Release validation rejects version disagreements, prereleases and a tag that differs from `android-vX.Y.Z`. The native build checks Cargo, npm and Tauri versions. Packaging checks the resulting macOS bundle or Windows executable version too.

Commit the version and source changes before packaging. Release builds require a clean checkout, including untracked files. Use a clean checkout for release builds if local audit files are present.

## Build and publish

Run **Prepare signed Android desktop release** manually to prepare signed packages without publication. Both matrix jobs use the protected environment and produce Actions artifacts. Missing credentials fail the build. Manual dispatch never promotes the feed.

To publish, create and push the reviewed `android-vX.Y.Z` tag through your normal release process. The workflow serializes Android releases, builds macOS arm64 and Windows x64, and waits for both packages. Only the publish job has repository write permission.

The macOS package path retains payload repair, Developer ID signing, notarization, stapling, installed-package smoke verification and distribution ZIP creation. It then creates the updater `.app.tar.gz` from the final stapled app, extracts it into a temporary directory, and verifies both the extracted code signature and stapled notarization ticket before signing the archive. The Windows package path retains final Authenticode signing and verification before signing the final NSIS `.exe` for the updater. Tauri's signer binds `--app-version` into each signature. `verify-update` uses the same `minisign-verify` implementation as the updater to verify the final bytes, global signature and signed version against the embedded public key.

The publish job validates both platform packages, matching commit and public-key identity, version, artifact names, hashes and signatures. It uploads a draft `android-vX.Y.Z` release and downloads all its assets to verify their final hashes while the release is still private. Only then does it publish that release without selecting it as the repository's latest release and promote `android-updater.json` on the dedicated `android-updates-stable` release. The feed refuses a lower version or different contents under the same version. A completed identical publication is a validated no-op. The existing Swift `v*` release workflow remains independent.

A retry can complete an existing draft release. An already published version keeps its immutable assets and must match the verified packages before feed promotion. If the feed already names that exact version and contents, remote assets are verified again and publication makes no changes. Retry a failed publish job with the original verified Actions artifacts. Rebuild failed platform jobs from the same reviewed commit before a release is published. Do not overwrite published version assets to repair a release. Publish a higher version instead.

Before replacing the canonical feed, the publisher stores and verifies immutable `android-updater-X.Y.Z.json` snapshots on the feed release. The first feed release stays a draft until its canonical manifest is uploaded and read back successfully. A failed canonical replacement restores the previous manifest. If GitHub is unavailable during both upload and restoration, the next run reconstructs the canonical manifest from the highest verified snapshot for that release. These durable snapshots also recover a process interruption between deletion and upload. GitHub asset replacement cannot be atomic, so clients can receive a transient HTTP error during replacement or an API outage. They keep their installed application and can retry the check.

The client feed is fixed to this URL.

```text
https://github.com/raymondctc/mobile-capture/releases/download/android-updates-stable/android-updater.json
```

Its supported platform keys are `darwin-aarch64` and `windows-x86_64`. Asset URLs contain the immutable `android-vX.Y.Z` release tag. The updater requires a signature that binds the announced version, so a modified feed cannot relabel an older signed package as a new version.

## Validate the first rollout

Install the first signed updater-enabled release manually. Earlier builds have no trusted update key and cannot bootstrap themselves through this feed. On isolated test installations, publish a higher test version with the intended production credentials and verify download progress, offline retry, signature refusal and install failure recovery. Verify that active capture, stopped unsaved capture, cancelled save, saving and device recovery all refuse installation without changing capture state. Finish a save, choose Install and restart, and confirm the new native version on macOS and Windows.

Do not treat the local fixture-key tests as evidence of Apple notarization, Windows certificate trust or a production two-version rollout.
