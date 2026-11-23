# Persistent approval whitelists

The selected configuration (`.greenlight/config.yaml`, or a legacy root config) specifies `ask`, `allow` or `deny` for each tool type. A configured **`deny` always wins**, even if an approval was saved earlier. A configured `allow` needs no approval.

On a shell or native write approval prompt:

- `[y]` — approve just this request.
- `[a]` — trust this exact shell command for the current process; for `write`, trust native writes for this process.
- `[s]` — request a saved grant for this session only, then review and confirm its scope.
- `[p]` — request a grant for the current project, then review and confirm its scope.
- `[g]` — request a global grant across projects, then review and confirm its scope.
- `[n]` — deny; in the **CLI** an empty Enter also denies. In the **TUI**, Enter is deliberately inert while an approval is pending, and Escape closes the popup without deciding; the agent keeps waiting.

Session/project/global requests open a second typed prompt stating the exact command or all-write coverage and the affected project(s). Only a fresh `y` saves the grant and permits the operation. `n` or empty Enter (CLI) cancels the grant and denies the operation. EOF, disconnection or save failure leaves it unauthorized and saves no grant. TUI Escape hides the confirmation without deciding. TUI Enter and key repeats are ignored. Existing saved grants retain their scope and do not require repeated confirmation.

**Shell grants match the complete command string exactly.** Even globally approved commands remain confined to the Linux Bubblewrap sandbox; approval never grants an unrestricted host shell. `ls -la` will be allowed on subsequent executions, but `ls -l` and `ls -la && rm -rf data` remain separate commands requiring approval. We intentionally do not implement command-prefix matching or wildcard shell rules, since `sh -c` executes shell operators and expansion. In particular, `find`, command substitutions, and scripts are not automatically trusted by their executable name.

**Native write grants cover all writable paths allowed by Greenlight's path checks** in that project (or in any project for `[g]`). Every proposed write still prints its unified diff and is recorded in the edit journal; approval prompts are skipped for trusted writes. Shell commands run in the read-only Bubblewrap sandbox. Writable cache and temporary files are outside the native edit journal.

Session grants: `<project>/.greenlight/state/session-approvals/<session-UUID>.yaml`. They survive process restarts and apply to all steps/branches of that same session. Other sessions, including newly created ones, do not inherit them. Binding a session or opening/dismissing a prompt writes no grant; only a confirmed `s` choice saves one. Native read/web retain their existing once/process choices.

Project grants: `<project>/.greenlight/state/approvals.yaml` (ignored by Git and not writable using the native agent file tool).

Global grants: `${XDG_CONFIG_HOME}/greenlight/approvals.yaml` (when `XDG_CONFIG_HOME` is absolute), otherwise `~/.config/greenlight/approvals.yaml`.

Example:

```yaml
shell:
  - ls -la
  - python -m unittest discover
write: true
```

Use `greenlight --root /path/to/project approvals` to inspect project/global entries, or `approvals --session SESSION_ID` to also inspect one existing session and locate its grant file. Edit or delete individual YAML entries to **revoke** saved grants. Invalid permission files cause an explicit startup error; they are not silently ignored. File updates are written using a same-directory temporary file and atomic rename with Unix `0600` file permissions.

Security note: selecting `s`/`p`/`g` requests a persistent grant; the separate confirmation is mandatory in both CLI and TUI. Global `write: true` authorizes native edits for **every project using this user account** and should only be used if you understand that scope. A shell grant authorizes the entire exact command string within the sandbox, including operators and redirections; don't persistently approve compound commands unless you intend to trust them. Grants stored in the global file are not tied to a particular repository. Greenlight writers serialize the selected scope’s reload–merge–save operation with a stable sibling `approvals.lock` file, then sync the atomic replacement. Session, project and global scopes lock independently. Manual YAML edits do not participate in this advisory lock; stop active writers first. Running processes keep their loaded grant snapshot, so restart them after manual revocation.

## Command-level grants: design pending

Current shell grants still match the **entire command string**, including arguments, paths and operators. Approving `ls` does not approve `ls -la src`, and approving `cat notes.txt` does not approve another file. Process/session/project/global change the lifetime/scope, not the matching rule.

The TODO tracks a joint design for argument-varying executable grants. It must distinguish the executable and its argument/option/path restrictions from shell composition, executable lookup, wrappers/interpreters, redirections, substitutions and environment assignments. A `cat` grant must not auto-approve `cat something.txt | ./harmful_script.sh`; every executed component must be covered or the whole compound command must prompt. This is not implemented yet. The colored exact-command preview changes presentation only, not grant semantics.

Native read/write grants never expand the selected project root. In particular, `read: allow` permits authorized reads inside that project, not arbitrary host paths. Absolute paths, parent traversal, outside symlink targets and private settings/state remain blocked. Directory discovery uses that same read policy. See [project scope](22-installation-and-projects.md#native-read-scope).

## Automatic project inspection

GL-012's [project inspection profile](46-project-inspection-permissions.md) can authorize a limited read-only argv language without saving a grant. It uses a private filtered view inside Bubblewrap, rather than granting a command-name prefix over the original project. Existing exact grants retain their original sandbox behavior and take precedence over this automatic profile. Unsupported forms still use this document's exact-command approval flow; no wildcard or prefix grant schema is added. `permissions.project_inspection: false` disables the profile, and explicit deny/read-ask policies remain authoritative as described in that reference.
