# Manual installer verification

**Goal:** Run signed Windows installer and upgrade checks without publishing a release.
**Architecture:** Extend the existing release workflow with workflow_dispatch. A small
Node helper validates event, version and baseline before writing GitHub environment values.
Keep installation on the isolated Windows runner and retain the existing local guard.
**Execution:** Inline continuation of the installer validation backlog.

## Contracts

- Manual branch runs never execute the publish job, including runs targeting a tag.
- Tag-push releases still require the tag to match the application version.
- Baseline is a stable version older than the candidate; default is 0.5.0.
- No installer runs locally; no live proxy, USB or personal profile changes.
- Reports distinguish native window startup from verified WebView UI behavior.
- Offline bootstrap without WebView2 and physical device tests remain separate.

## Steps

- [x] Add failing behavioral tests for release context, invalid baseline and local installer guard.
- [x] Implement release context; wire manual workflow and explicit publish condition.
- [x] Parameterize installer baseline; require exact executable versions; correct report wording.
- [x] Run release tests, actionlint, PowerShell syntax checks and review the diff.
- [x] Document manual execution and pending runner evidence.

Local validation: 10 release/context tests passed; actionlint, PowerShell parser and
git diff --check passed. The initial context test run had four failures before the
helper existed; the pre-existing local installer guard passed in both runs.
Review was performed inline. No application runtime source was changed.

Remote execution needs the updated workflow on GitHub. Commit/push and merge are
separate from artifact publication; do not create a release tag as part of these checks.

## Runner finding and fixture isolation

Manual run 44252610520 stopped in Rust tests before installation. The second save
in `recovering_last_good_keeps_damaged_file_and_backup` returned Windows error 5.
The publish job was skipped as intended.

Recovery fixtures used only PID and wall-clock nanoseconds. A deterministic test
using the same timestamp reproduced error 5 when another fixture was readonly.
Adding an atomic serial isolates those fixtures; backup/service test fixtures now
use the existing serial-bearing storage stamp for the same reason. These changes
are test-only. Runner output did not include fixture paths, so timestamp collision
is consistent with that failure but cannot be proven from the original log alone.

Validation after the fix: the new regression failed before the fix and passed
after it; all 68 core tests and 21 service tests passed. Rustfmt and desktop Clippy
with all targets and warnings denied passed. A fresh runner run is still required.
