# Renaming Stepwright to Greenlight

The project, Cargo package, and installed CLI command are named **Greenlight**. The command is `greenlight`, and the sample configuration is `greenlight.yaml`.

## Existing projects

- Existing `stepwright.yaml` continues to work. If both files exist, `greenlight.yaml` wins. For clarity, rename the old config file yourself when convenient; it is not rewritten automatically.
- Project instructions now live at `.greenlight/instructions.md`. The old `.stepwright/instructions.md` remains a read-only fallback if the new file is absent.
- On the first command that needs project state (including TUI, session history, approvals, sandbox cache), Greenlight **moves** an existing `.stepwright/state` directory to `.greenlight/state`. The complete directory is moved, preserving SQLite data, WAL files, artifact blobs, saved project approvals and sandbox caches. State is not discarded or rebuilt.
- If both state directories are present, the command **fails** with an explicit error. Do not delete either directory without inspecting their contents and confirming which history you want to keep.
- State paths and both legacy/new internal state directories remain blocked from agent native file access, restore paths, workspace hash recording and sandbox visibility.

**Before the first run after upgrading**, close older instances of the application. Don't operate the old executable against a project simultaneously with Greenlight while its state is being moved. For important projects, back up the existing `.stepwright/state` directory first.

## Quick local smoke test

From the Greenlight checkout:

```bash
git switch master
cargo build --locked
cargo test --locked
cargo clippy --locked -- -D warnings
cargo run -- --root "$PWD/.local-smoke" sessions
```

If your smoke fixture still has `stepwright.yaml`, it remains valid. After inspecting the sessions, you can rename that config file to `greenlight.yaml`. Inspect `.local-smoke/.greenlight/state` to confirm the migrated history is present.

An installed old `stepwright` binary will not be deleted by changing the package name. Remove it separately once Greenlight is installed and verified.
