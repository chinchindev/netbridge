# Windows releases and signed updates

## Repository setup

The public repository is https://github.com/chinchindev/netbridge. Direct GitHub
Releases update URLs use this repository. For private source,
use a separate public distribution repository or design authenticated distribution;
never embed a shared GitHub token in the application.

Configure the selected public release repository before building a distributable:

```powershell
node scripts/configure-updater.mjs OWNER/REPOSITORY
```

The release workflow sets the endpoint to its own `GITHUB_REPOSITORY` in the build
checkout. Local configuration uses https://github.com/chinchindev/netbridge/releases/latest/download/latest.json.

## Signing keys

The public verification key is committed in `desktop/src-tauri/tauri.conf.json`.
The private key must remain outside Git. Configure these repository Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: contents of the private key file.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: password, only if the key is encrypted.

Do not print either secret in CI logs or paste it into chat. Keep a secure backup of
the private key; already installed clients trust the corresponding public key.
The initial key for this workspace is stored under
`%LOCALAPPDATA%\NetBridgeReleaseKeys\netbridge-updater.key`.

Updater signatures are different from Windows Authenticode signing. They verify
downloaded package bytes and the signed version. This setup does not supply an
Authenticode certificate or claim that Windows SmartScreen trusts the installer.

## Bump and publish

```powershell
node scripts/version.mjs 0.3.1
node scripts/version.mjs --check
node --test scripts/release.test.mjs
git add desktop/package.json desktop/package-lock.json desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock desktop/src-tauri/tauri.conf.json
git commit -m "chore: release 0.3.1"
git push origin main
git tag v0.3.1
git push origin v0.3.1
```

The version script updates all application version files together. A mismatched
tag fails before signing. Core `netbridge` has its own version and is not renamed.

Push/PR runs Windows CI without signing secrets. A version tag runs the release
workflow with full tests, a signed NSIS installer including offline WebView2, and
publishes the assets only when the build succeeds. Prerelease tags such as
`v0.3.1-rc.1` are marked prerelease and are not offered through the stable `latest` URL.
Do not overwrite an existing version tag or release artifact to ship different bytes.

## Updating clients

Versions before 0.3.0 do not have the app updater; install the first updater-enabled
release manually. Later releases appear under Cài đặt → Cập nhật NetBridge.
Checking is manual. Installing requires confirmation; download/signature failures
leave the current proxy session running. Restoration failure prevents installation.

Verify on an isolated Windows user/VM: install version N, publish signed N+1,
check/download/install, confirm the new version and preserved config. Also test a
bad signature, unreachable endpoint, busy ADB job and failed proxy restoration.
Do not perform destructive lifecycle checks on a user's active network session.

## Troubleshooting

- `EmptyEndpoints`: configure the repository before local release builds.
- HTTP 404: no stable release/latest.json exists yet, or the release is private.
- Signature/version verification failure: correct the release signing/manifest;
  never disable verification to accept it.
- Missing key secret: the signing step fails rather than publishing unsigned updates.
- Runtime download timeout: inspect the Actions log; the previously published release
  remains available because publishing is a separate job after a successful build.
