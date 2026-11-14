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
