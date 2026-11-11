# Workspace restoration and forks

Greenlight can reconstruct **native file edits** recorded in `.stepwright/state` at an event in the active session branch ancestry.

```bash
stepwright --root /path/to/project restore SESSION_ID --event 123 --dry-run
stepwright --root /path/to/project restore SESSION_ID --event 123
stepwright --root /path/to/project fork SESSION_ID --event 123 --step implement
stepwright --root /path/to/project fork SESSION_ID --event 123 --step implement --no-restore
```

The dry run shows each proposed file modification/deletion/creation and its unified diff. The apply command asks for confirmation, rechecks tracked snapshots after that confirmation, then applies changes. `fork` previews and **offers restoration before forking**; choosing no leaves the filesystem as-is. The explicit `--no-restore` option skips inspection and preserves the original fork behavior.

Restoration is **not a Git checkout**. It affects only files with native writes (or prior explicit restores) in the visible session history; files are reconstructed from historical before/after snapshots. Newly created native files may be removed. Restoration operations are independently journaled as `workspace.restored`, including deleted files, so a subsequent restore can detect the current tracked state. An event must be visible in the active branch; unrelated branch events are rejected.

## Safety limitations

- The entire tracked set is checked before any mutation. A file that differs from the last tracked snapshot is treated as a conflict, even if the difference came from a shell command or manual edit. A discontinuity between native-edit snapshots also blocks restore.
- Only plain UTF-8 regular files under the project root are supported. Symlinks, traversal, `.git`, `.env`, `.ssh`, and internal state paths are refused. Missing parent directories are refused rather than recreated.
- Applying updates is **not transactional across files**. Each restored file is applied and journaled separately; a later error leaves earlier files updated, and the error is surfaced. Concurrent external filesystem mutations cannot be fully prevented.
- Changes made only by shell commands, outside Greenlight or by other sessions are **not captured or reconstructed**. In particular, an untracked shell-modified file that was never natively edited is untouched; a successful restore does not certify that the whole repository matches its historical state.

For sensitive repositories, create a Git checkpoint or independent backup before restoring.
