# Android desktop payloads

The Android Tauri product initially supports macOS ARM64 and Windows x64. Its identifier remains `dev.mobilecapture.tauri-poc`, and its visible name is Mobile Capture Android. Existing app data and private CA paths remain in the existing namespace.

`downloads.lock.json` pins complete upstream archives for ADB 37.0.1 and mitmproxy 12.2.3. macOS pins are reused from the reviewed Swift distribution lock. The Windows pins were computed from downloaded bytes on 2026-10-23.

| Windows archive | Observed bytes | SHA256 |
| --- | ---: | --- |
| `mitmproxy-12.2.3-windows-x86_64.zip` | 85,634,208 | `04a01ea95ae96df75058a893e774957d294e69012dab1f4e256ce2b0c6725483` |
| `platform-tools_r37.0.1-win.zip` | 8,044,989 | `45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99` |

The [official mitmproxy object listing](https://downloads.mitmproxy.org/list?delimiter=/&prefix=12.2.3/) lists the Windows standalone archive. Its [release build source](https://github.com/mitmproxy/mitmproxy/blob/v12.2.3/release/build.py) packages the three standalone executables. The Windows executable contains Python 3.14.4 and OpenSSL 3.0.19. The complete executable is retained. Embedded license records are also copied into accessible notices. The macOS archive retains the complete app, Python framework, third-party libraries, resources, and notices.

Google's [repository metadata](https://dl.google.com/android/repository/repository2-3.xml) names the Windows archive `platform-tools_r37.0.1-win.zip`. The complete platform-tools tree is retained, including `AdbWinApi.dll`, `AdbWinUsbApi.dll`, `libwinpthread-1.dll`, `NOTICE.txt`, and `source.properties`. Google describes the version in its [platform-tools release notes](https://developer.android.com/tools/releases/platform-tools).

`payload.py` verifies archive hashes before extraction. It refuses absolute paths, traversal, duplicate paths, ZIP symlinks, hard links, special files, and out-of-tree TAR symlinks. It bounds download and expanded sizes. It builds a manifest of exact installed bytes and relative symlink targets after macOS nested signing. The app and Doctor compile the manifest SHA256 into their binaries. Runtime resolution verifies that digest, all declared bytes, the complete file set, and executable architecture. Missing or damaged packaged components fail without external discovery.

A pinned archive hash is a reviewed byte identity, not an independent publisher signature. Preserve the complete notices and distribution metadata when redistributing the product. Dependency upgrades require downloaded-byte review and native acceptance on each supported target.
