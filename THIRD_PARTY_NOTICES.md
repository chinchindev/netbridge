# References and dependencies

NetBridge uses Rust, Tokio, serde/serde_json, httparse, url and windows-sys.
Resolved dependency versions are pinned in Cargo.lock. Their respective licenses apply.

The portable-tools release also uses reqwest (Windows native TLS), roxmltree,
sha1 and zip. Dependency license texts are included in THIRD_PARTY_LICENSES.txt
in the portable package.

Android SDK Platform-Tools 37.0.1 for Windows is embedded from Google's official
archive: https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip
SHA-1: e03e78b1d80b396f1c3358e31251cb31740e1110.
Only ADB, its supporting DLLs, source.properties and NOTICE.txt are extracted for use.
Original notices: assets/adb/NOTICE.txt, shipped as ADB-NOTICE.txt.
ADB source: https://android.googlesource.com/platform/packages/modules/adb/
Release information and SDK terms: https://developer.android.com/tools/releases/platform-tools

Network measurements call Cloudflare's documented __down and __up endpoints.
The Cloudflare JavaScript SDK is not included; no SDK analytics requests are sent.
API reference: https://github.com/cloudflare/speedtest

PSNET (https://github.com/psmux/psnet) was inspected as an architectural reference,
especially src/network/networks/adapters.rs. No PSNET traffic capture, firewall,
third-party databases or application source are included in this binary.
The project is MIT licensed; its notice is reproduced here for attribution:

MIT License

Copyright (c) 2026 Godwin Sam Josh

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
