# Persistent approval whitelists

The selected configuration (`.greenlight/config.yaml`, or a legacy root config) specifies `ask`, `allow` or `deny` for each tool type. A configured **`deny` always wins**, even if an approval was saved earlier. A configured `allow` needs no approval.

On a shell or native write approval prompt:

- `[y]` — approve just this request.
- `[a]` — trust this exact shell command for the current process; for `write`, trust native writes for this process.
- `[p]` — request a grant for the current project, then review and confirm its scope.
- `[g]` — request a global grant across projects, then review and confirm its scope.
- `[n]` — deny; in the **CLI** an empty Enter also denies. In the **TUI**, Enter is deliberately inert while an approval is pending, and Escape denies.

Project/global requests open a second typed prompt stating the exact command or all-write coverage and the affected project(s). Only a fresh `y` saves the grant and permits the operation. `n`, Escape (TUI), empty Enter/EOF (CLI), disconnection or save failure denies the operation and saves no grant. TUI Enter and key repeats are ignored. Existing saved grants retain their scope and do not require repeated confirmation.

**Shell grants match the complete command string exactly.** Even globally approved commands remain confined to the Linux Bubblewrap sandbox; approval never grants an unrestricted host shell. `ls -la` will be allowed on subsequent executions, but `ls -l` and `ls -la && rm -rf data` remain separate commands requiring approval. We intentionally do not implement command-prefix matching or wildcard shell rules, since `sh -c` executes shell operators and expansion. In particular, `find`, command substitutions, and scripts are not automatically trusted by their executable name.

**Native write grants cover all writable paths allowed by Greenlight's path checks** in that project (or in any project for `[g]`). Every proposed write still prints its unified diff and is recorded in the edit journal; approval prompts are skipped for trusted writes. Shell commands run in the read-only Bubblewrap sandbox. Writable cache and temporary files are outside the native edit journal.

Project grants: `<project>/.greenlight/state/approvals.yaml` (ignored by Git and not writable using the native agent file tool).

Global grants: `${XDG_CONFIG_HOME}/greenlight/approvals.yaml` (when `XDG_CONFIG_HOME` is absolute), otherwise `~/.config/greenlight/approvals.yaml`.

Example:

```yaml
shell:
  - ls -la
  - python -m unittest discover
write: true
```

Use `greenlight --root /path/to/project approvals` to inspect effective project/global entries and locate the files. Edit or delete individual YAML entries to **revoke** saved grants. Invalid permission files cause an explicit startup error; they are not silently ignored. File updates are written using a same-directory temporary file and atomic rename with Unix `0600` file permissions.

Security note: selecting `p`/`g` requests a persistent grant; the separate confirmation is mandatory in both CLI and TUI. Global `write: true` authorizes native edits for **every project using this user account** and should only be used if you understand that scope. A shell grant authorizes the entire exact command string within the sandbox, including operators and redirections; don't persistently approve compound commands unless you intend to trust them. Grants stored in the global file are not tied to a particular repository. Concurrent changes to the same approval file from independent processes are not fully locked.

## Command-level grants: design pending

Current shell grants still match the **entire command string**, including arguments, paths and operators. Approving `ls` does not approve `ls -la src`, and approving `cat notes.txt` does not approve another file. Process/project/global change the lifetime/scope, not the matching rule.

The TODO tracks a joint design for argument-varying executable grants. It must distinguish the executable and its argument/option/path restrictions from shell composition, executable lookup, wrappers/interpreters, redirections, substitutions and environment assignments. A `cat` grant must not auto-approve `cat something.txt | ./harmful_script.sh`; every executed component must be covered or the whole compound command must prompt. This is not implemented yet. The colored exact-command preview changes presentation only, not grant semantics.

Native read/write grants never expand the selected project root. In particular, `read: allow` permits authorized reads inside that project, not arbitrary host paths. Absolute paths, parent traversal, outside symlink targets and private settings/state remain blocked. Directory discovery uses that same read policy. See [project scope](22-installation-and-projects.md#native-read-scope).
