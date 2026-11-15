# Manual integration smoke test: calculator workflow

**Status:** reproducible *exercise*, not an automated integration test or a deterministic assertion about model behavior. Based on a disposable calculator project exercised during development on 2026-11-11–13. The original run used Qwen via DeepInfra; other models may choose different actions or stop at different points. **Never use a valuable repository as the scratch project.**

The checks below cover CLI/TUI navigation, artifacts, permission prompts, native-write recording, workspace drift and guarded restore; they also identify what still requires proper automated tests.

## 1. Prepare an isolated fixture

From the Greenlight repository root, on Linux with Rust and Bubblewrap:

```bash
cargo test --locked
cargo clippy --locked -- -D warnings
cargo run -- sandbox-check

TEST_ROOT=$(mktemp -d /tmp/greenlight-smoke.XXXXXX)
cp greenlight.yaml "$TEST_ROOT/"
cp -R workflows shared "$TEST_ROOT/"

cat > "$TEST_ROOT/calculator.py" <<'PY'
def add(a, b):
    return a - b  # Deliberately wrong: the test expects addition.
PY

cat > "$TEST_ROOT/test_calculator.py" <<'PY'
import unittest
from calculator import add

class CalculatorTests(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

if __name__ == "__main__":
    unittest.main()
PY

cargo run -- --root "$TEST_ROOT" validate add-feature
```

The fixture's `add` test initially fails **by design**. It makes the review step more likely to uncover an existing bug unrelated to the requested new feature; it is not a guarantee of exactly how a language model will respond. Keep `TEST_ROOT` in the same shell for later commands.

## 2. Run an agent session (requires a configured model/API key)

```bash
export DEEPINFRA_API_KEY='...'
cargo run -- --root "$TEST_ROOT" start add-feature \
  --goal "Add square(a) to calculator.py without changing existing functions. Add tests for positive, negative and zero inputs. Verify with python -m unittest discover."
```

Use the actual configured `provider.model` in `greenlight.yaml` (or pass an available `--model`). The `add-feature` workflow has **explore → plan → human approve → implement → review**. For each requested shell/write permission, inspect the exact operation and choose an appropriate scope; there is no need to grant global trust for a smoke test. At the human plan gate, inspect `plan.md` before deciding.

Copy the printed UUID into the **current shell**:

```bash
SESSION_ID=PASTE_SESSION_ID_HERE
cargo run -- --root "$TEST_ROOT" sessions
cargo run -- --root "$TEST_ROOT" history "$SESSION_ID"
cargo run -- --root "$TEST_ROOT" artifacts "$SESSION_ID"
cargo run -- --root "$TEST_ROOT" changes "$SESSION_ID"
cargo run -- --root "$TEST_ROOT" workspace-status "$SESSION_ID"
cargo run -- --root "$TEST_ROOT" tui "$SESSION_ID"
```

In the TUI, verify that opening does **not** focus the composer; Right/Enter navigates from sessions into the activity/artifact list and detail; Left/Escape backs out; Up/Down or `j/k` selects/scrolls, including preview dialogs; `i` enters the composer; `1`/`2` switches views; `m` toggles rendered/raw Markdown. **Do not rely on PageUp/PageDown.** Approval choices require an explicit key; Enter must not silently approve.

## 3. Inspect file history and drift without overwriting files

If and only if an agent performed native writes, take an event ID from `changes` and inspect the immutable recorded diff:

```bash
cargo run -- --root "$TEST_ROOT" changes "$SESSION_ID"
cargo run -- --root "$TEST_ROOT" diff "$SESSION_ID" --edit EDIT_EVENT_ID
```

Create a **new**, untracked manual note in this disposable fixture, then re-run `workspace-status`:

```bash
printf 'Manual smoke-test note\n' > "$TEST_ROOT/manual-note.txt"
cargo run -- --root "$TEST_ROOT" workspace-status "$SESSION_ID"
```

The note is out-of-band drift within the scanned scope. The manifest cannot attribute its author or recover prior bytes. To test **restore conflict refusal**, pick an event that actually predates a native write to a tracked file, back up that file **outside the project**, modify it manually, and run `restore ... --dry-run`; the guarded planner should reject a mismatched tracked snapshot. Restore the manual edit from the backup before proceeding. This step is conditional: don't claim it passed if the selected event would not change that file.

For a positive restore exercise, choose a visible historical event and **first** run the read-only preview:

```bash
cargo run -- --root "$TEST_ROOT" restore "$SESSION_ID" --event EVENT_ID --dry-run
```

Only perform a real restore/fork if you intend to change this disposable fixture, have reviewed the preview and understand that other files, external effects and cache writes are **not** reverted. A successful restore is not a full Git checkout.

## 4. What the original development exercise established

In a prior disposable calculator session, the coding agent added `square()` and tests, while the intentionally broken `add()` caused review to stop. Native edit events were recorded; a manual `calculator.py` drift caused restore refusal; after undoing that manual drift, a historical native-edit restore succeeded. A separate `manual-note.txt` remained. This establishes observed behavior **for that session only**, not a guarantee that every model run reaches the same events or that all restoration failure modes have been tested.

## Automated baseline and remaining coverage

`cargo test --locked tui::p0_tests` runs deterministic engine/UI acceptance checks at 80×24 with a scripted provider, including the blocked → inspect → instruct → approve/deny → verify/recover path. It also checks persistent-grant confirmation, malformed events/missing artifacts and partial-restore reporting. The real model-driven calculator exercise above remains a separate manual check.

## Further coverage

The deterministic suite now covers the core controlled/provider-free paths, ownership, pending-request expiry, native-write interruption and partial-restore error reporting. Extend it with actual terminal resize tests, broader fault injection and live-provider/network scenarios as those capabilities evolve. The integration test requiring Bubblewrap/user namespaces remains ignored by default and must be run explicitly in a suitable environment.

See [validation status](06-validation.md), [restore limitations](09-workspace-restore.md), [workspace recording](14-workspace-recording.md) and the [README TODO](../README.md#roadmap--todo).


## P1 navigation and resize check

1. Open `?`, scroll with arrows/j/k or PgUp/PgDn, and close with Escape. Enter `/help` in the composer and verify it opens the same panel.
2. Draft different instructions in two sessions, switch between them, filter to no results and clear the filter. Each session should retain its own draft; creating a new session should open an empty draft. Exit/reopen discards drafts.
3. Open the latest event's evidence while another process appends activity. The event and reading position should stay fixed. Return to the activity list and press End to follow new events again.
4. Resize through 120×40, 80×24, 60×20 and 40×14. Verify the active pane remains usable, long message endings stay visible, and restoring the size retains drafts/selections. Scroll to End, resize wider and press Up: content should remain visible and scroll back immediately.
5. With a pending approval, shrink below 40×20. Decision keys should do nothing; Escape may deny. Resize to 60×20 and confirm the original request is still present. Enter must never approve.


## Markdown preview hardening

1. Register a document containing CJK, combining accents and emoji in a simple table. Compare Markdown and literal modes (`m`); ordinary table borders should line up in rendered mode.
2. Include literal ANSI/OSC escape bytes, C1 characters, a lone carriage return and a bidirectional formatting control. Both modes should show visible code-point escapes, not execute controls. CRLF becomes newline and tabs become four spaces.
3. Use a four-backtick fence containing three-backtick lines and Markdown-looking content. It should stay code until a matching closing fence.
4. With an appropriately increased artifact-size policy, register a document over a preview limit. Scroll to the end in both modes: a truncation notice should appear. Export with `greenlight artifact SESSION NAME > artifact.md` and confirm the complete stored content remains available; preview limits must not modify it.


## Artifact history and diffs

1. Register two versions of one artifact with `put-artifact`. Open Artifacts, select its name and press `v`; both creation IDs should be visible. Read the older body with Enter, then inspect `p` provenance.
2. Left returns to versions. Select the newer version and press `d`; inspect removed/added lines. The oldest visible version should explain that it has no predecessor. `o` returns to content; `m` toggles its preview.
3. Publish a third version from another terminal. Refresh should keep the inspected creation ID and diff pinned. Escape returns to the latest artifact list; the composer draft should be retained.
4. Fork at an earlier event and reopen versions. Parent versions after the cutoff must be absent; inherited versions should identify their source branch. Switching branch while browsing closes the browser.
5. Export each side with `artifact SESSION NAME --event ID > version.md`. Compare originals externally if they exceed the diff limit. Version inspection/export must not add events, approve anything, or resume work.


## Provider failure handling (use a mock endpoint)

1. Return 401/invalid_api_key, 402 or 429/insufficient_quota. CLI/TUI should explain the key or billing problem, block the run and send no retry. Put a fake secret in the error message and verify it is absent from history/usage/diagnostics.
2. Return 429/rate_limit_exceeded with Retry-After: 0, then a valid completion. Verify two request UUIDs in `usage`, one failed and one successful. Repeated limits stop after three requests; Retry-After: 60 stops immediately rather than shortening the wait.
3. During an eligible retry wait, press x in TUI navigation. It should cancel promptly, retain the first call record and send no second request. Resuming creates a fresh worker. x must remain text in the composer/search and must not answer an approval.
4. Return HTML/non-JSON 503 or simulate a timeout. The message should explain service/network uncertainty without exposing the body/URL or automatically repeating inference.
5. Fail an agent-verification request with an authentication error in a workflow with verification_failed retry/detector rules. It should block immediately with no additional provider calls.


## Background document previews

1. Open an artifact over 8 KiB. A preparation message may appear briefly; type an unsent draft or navigate while the preview is pending. Input should remain usable and the stored artifact must stay unchanged.
2. Switch versions/modes and resize rapidly. A ready viewport must correspond to the selected creation ID and mode; older content must not flash into the new panel. Resize back to a previous width: the panel must finish preparing rather than wait forever.
3. Scroll to End, resize, then scroll upward. The prepared viewport should match normal wrapping and clamp to content as before. Repeat in literal mode and in artifact-version content/provenance/diff panels.
4. Trigger an approval while a large preview is pending. The request must take priority, retain its explicit decision keys and never be replaced by a preview result.


## Semantic activity cards

1. Run a tool or command step, then open its activity card. The request/output IDs and complete recorded arguments/output should agree with raw history (`v`). Read/web content beginning with exit_code or saying “tests passed” must not turn into a shell/verification success.
2. Inspect an agent reviewer report and the aggregate verification verdict separately. Under an any-of policy, a failed check must not overwrite the meaning of a later aggregate pass.
3. Select an older artifact-created card and press o after publishing another same-name version. It must open the selected creation ID, not latest-by-name. Use d on a file card to inspect the journal-backed diff.
4. Open raw mode, return to cards, resize to 40×14 and expand evidence. Selection/following and scrolling should retain their established behavior; no approval or execution action should be triggered by inspection.
5. Legacy tool results should say unlinked; malformed/missing/cross-fork tool references should remain inspectable as diagnostics. Routine tool-permission decisions remain hidden until raw mode is enabled.


## Reading positions and local exports

1. Read an activity event at a nonzero scroll offset, switch to Artifacts, read at another offset and switch back. Each tab should preserve its place. Repeat across sessions/branches, including a journal diff, and check that help/context scroll does not replace the saved tab offset.
2. Replace an artifact with a newer version of the same name. Its new body should start at offset zero. Unsent drafts should remain unchanged.
3. F filters activity metadata/summaries, Enter keeps the query, and F then Escape clears it. Test an accented filename and no matches. C/E typed inside the filter/search/composer must be text rather than export actions.
4. With a clipboard utility and a desktop session, press C on an artifact longer than the preview limit and inspect the copied source. It should be the full original, not clipped/escaped display text. On a server without a clipboard backend, expect an actionable diagnostic, not a success claim.
5. Set VISUAL/EDITOR to a command that waits, press E and close the review snapshot. The TUI should regain input/appearance; stored artifacts/history/project files must remain unchanged. Repeat with a nonexistent editor. Own-worker execution should refuse editor handoff while leaving approvals available.
6. For automated/local smoke checks, prepend fake clipboard/editor programs to PATH and capture their stdin/file in a private temporary fixture. Verify exact UTF-8/control-byte content, snapshot deletion, unchanged event counts and zero model calls; do not overwrite the real desktop clipboard.

## Optional model-authored activity titles

1. With a local scripted/mock provider, return a coding tool action with `activity_title: Exploring call sites`, then an artifact/finish action with a different title. Confirm each gets a separate **Agent intent** card before its action, with exact response IDs and model-authored labelling. Enter expands the original response; F can filter by the title. Raw mode retains both `model.output` and `activity.title` records.
2. Deny or disable the requested write. Its intent annotation may remain, but no file should change and the tool-error card must stay distinct. A title saying “All tests passed” must not override rejected verification or turn the annotation green.
3. Omit the field on older-style responses: no annotation should be inferred and the existing factual cards should still work. Use a title longer than 80 Unicode scalar values, an empty string, numeric value or an escaped ANSI/bidirectional control; expect a bounded protocol retry with raw output retained, and no dispatched action from the invalid response.
4. Inspect multiple responses within one step and a branch fork. Titles belong to their exact branch-visible source; malformed/mismatched/cross-attempt links must be diagnostics. There must be no extra model request to generate a title and no title record in selected verification/detector evidence.

## Pricing discovery and billing estimates

1. On the default DeepInfra base URL, use model-info for an exact public model ID without an explicit pricing override. Check the public source, retrieved_at, USD/million rates and freshness message; effective_date should remain unknown. Repeat offline to check cache reuse. refresh-pricing forces a new public lookup without inference.
2. Configure explicit rates, including one null field; discovery must not fill or overwrite that field. Change to a proxy/unsupported provider URL or set pricing_discovery: false: rates must remain unknown unless configured.
3. With mocked reported usage, compare exactly at/above a context-tier boundary. The higher tier replaces the whole request's rates; fallback input estimates must not select a tier. Test cache-read/creation totals and mixed 5-minute/1-hour creation counts without double charging input. Unknown TTL/rates or inconsistent splits must leave cost unavailable.
4. Use a stale pricing date and inspect model-info plus a new usage report; the warning should be explicit. Refresh/change rates after recording a request: its original metadata and estimated cost must survive unchanged. Mock refresh failures and secret-like response bodies; errors must retain old retrieval timestamps and exclude body text.

## Workspace journal correlation and scan metrics

1. Run an isolated workflow that makes native writes and completes a step. Its Workspace audit card should show journal-explained paths, exact native event IDs and scan metrics; Enter expands the report. Verify the before/after hashes agree with the blobs and raw manifests.
2. Manually edit/create/remove a scanned file, then run workspace-status SESSION_ID with and without --json. These paths should be unexplained, with no claim of who edited them. Files, history and model-call counts must stay unchanged. A human-only fixture is sufficient to create scan baselines without paid inference.
3. Test a native A→B→A chain and an intervening manual change before a native edit restores A. Both may have no net scan delta, but only the continuous native chain is explained. A later manual change must fail the final-state comparison.
4. Cross a fork/retry cutoff, omit a journal row, remove an in-interval blob, or grow a previously scanned file beyond 20 MiB. Expect qualified unknown/error results rather than confident explanation/deletion. A missing blob outside the comparison interval must not prevent inspection of a new complete interval.
5. Run workspace-benchmark --runs 7 --json on stable representative source trees. Compare files, hashed bytes, microseconds and manifest payload bytes; no content is saved. Old manifest metrics remain unavailable. Recheck native-write recovery and guarded restore tests; this audit does not add broader rollback.

## Read-only manager Q&A

1. Use manager-context SESSION_ID to inspect model/provider and allowlisted sources without inference or appended history. A symlinked README/TODO, sensitive/config files and unavailable artifacts must not leak into the bundle; truncated sources must identify their captured-prefix digest and limits.
2. Against a local mock provider, ask a question via CLI --message/--file and TUI /ask. Check exact original whitespace/Unicode, saved snapshot/output/decision records, supplied citation IDs and purpose: manager usage. With manager.enabled, ordinary TUI text takes this route; /resume and approval keys remain deterministic.
3. Return a valid forward to an agent step. It must say delivery unavailable and leave branch/step/status, coding context, files and attempts unchanged. Invalid IDs, unknown/duplicate JSON fields, human-step destinations, made-up citations and a replacement instruction field must remain unresolved without a model repair loop or execution fallback.
4. Keep a coding owner active while asking; trigger an approval during the manager request. Only the approval's normal keys may answer it. Navigate/resize and inspect the saved result afterward. A new manager draft stays intact if one request is already running; /cancel-manager stops future retries without claiming an in-flight request was cancelled.
5. Change a workflow definition or control state during inference. Answers must retain their older snapshot qualification; proposed forwards become needs_review. Manager/provider failure must not block the coding attempt or leak raw provider errors. Reopen after an unfinished receipt/call: no automatic question replay. No paid calls are needed for these checks.

## Installed binary and independent projects

Install with explicit disposable `--data-dir` and `--bin-dir` paths; verify `--version` reports the selected commit. In an unrelated empty directory, run `greenlight init`, validate all three bundled workflows, and launch bare `greenlight`. From a nested source directory, `greenlight sessions` should find the same project. Initialize another project and confirm its sessions are independent. Edit a setting and workflow, rerun initialization and the installer, and confirm those edits and recorded sessions survive. Try an invalid installer ref/build and confirm the previously installed binary still runs. No model request is required for initialization, validation or browsing empty sessions. The Python installer tests exercise update/failure behavior using local Git and a fake compiler; a real native release installation should also be checked.
