# Project inspection permissions (GL-012, first version)

Merged into `master` on 2026-11-23. This implements the agreed simple-command subset. It does not add wildcard/prefix grants. GL-032 extends this profile to [validated read-only pipelines](48-safe-inspection-pipelines.md).

## Policy and controls

By default, simple supported shell-tool calls can run without a prompt when `permissions.shell` is `ask`, `permissions.read` is `allow`, and the workflow enables `shell`. They run as validated argument vectors against a private filtered project view. Existing native read/discovery tools remain preferable when they fit the request.

```yaml
permissions:
  shell: ask
  read: allow
  project_inspection: true # optional; effective default is true
```

Set `project_inspection: false` to retain exact-command approvals for all shell calls. `read: ask` or `read: deny` also prevents this automatic profile. `shell: deny` still denies model shell execution. Explicit `shell: allow`, existing exact process/project/global grants and explicit user-requested reruns keep their existing ordinary sandbox behavior; they are not rewritten into filtered inspection calls. Native write/web permissions are unchanged.

An unsupported form follows the existing exact-command authorization path. The complete original command is presented before execution; granting one command does not grant a prefix, executable name, arbitrary arguments or subsequent operators. Existing saved grant schemas and semantics are unchanged. Profile decisions are recorded in permission audit evidence; output identifies its filtered snapshot scope.

## Supported commands

Only installed executables at fixed `/usr/bin/NAME` paths qualify. Project executables, aliases, wrappers and arbitrary executable paths do not. Short flags can be combined. The table describes the first version, not every read-only option a program supports.

| Command | Supported options / operands |
| --- | --- |
| `pwd` | No arguments; reports the sandbox project root `/workspace` |
| `ls` | `-a -A -1 -R -d -p`; project paths; defaults to `.` |
| `cat` | `-n -b -s -v -E -T -A`; explicit project files |
| `head`, `tail` | `-q -v`, `-n N`, `-c N`; explicit project files; no following |
| `wc` | `-l -w -c -m -L`; explicit project files |
| `grep` | `-n -H -h -i -I -v -w -x -l -c -r -F -E -o -s`; positional pattern followed by explicit project operands |
| `rg` | `-n -H -i -I -v -w -x -l -c -F -s`; positional pattern and project paths, defaults to `.`; `--no-config` is enforced |
| `sort` | `-n -r -u -f -b -d -h -V`; explicit input files, no output-file/temp/helper options |
| `uniq` | `-c -d -u -i`; exactly one input file, no output operand |
| `cut` | `-s`, separate `-f LIST`, `-b LIST`, `-c LIST`, `-d CHAR`; input files |
| `diff` | `-u -q -s -r`, separate `-U N`; exactly two project operands |
| `cmp` | `-s -l`, separate `-n N`; exactly two project operands |

Numeric option values are nonnegative ASCII integers up to 1,000,000. Cut lists accept digits, commas and hyphens; delimiters must be one byte. Required program semantics still apply: a malformed selection can produce an ordinary tool error rather than a permission failure.

`--` can terminate option parsing. Ordinary quoted arguments, including filenames with spaces, are supported. Input paths are normalized to project-relative operands; the real project-root prefix and sandbox `/workspace` prefix are accepted. Parent components, private paths and stdin's special `-` operand are excluded from this profile. A literal file named `-` can be addressed as `./-`.

The syntax filter is deliberately conservative, even inside quotes. Globs, substitutions, backticks, tilde expansion, backslashes, control characters, redirects, unsupported pipelines, sequences, logical operators and grouping syntax require exact approval. Some otherwise harmless quoted regular expressions therefore still prompt. This is not a general shell parser and never attempts to reconstruct an unsupported shell expression.

Examples:

```sh
cat src/main.rs                   # supported profile
head -n 20 'docs/file with spaces' # supported profile
ls -a src                        # supported profile
cat -- -filename                 # literal input filename
cat src/main.rs | ./danger.sh     # exact approval for the entire command
ls "$(./danger.sh)"               # exact approval; no partial execution
sort -o output.txt input.txt      # exact approval
uniq input.txt output.txt         # exact approval
```

`ls -l`, `stat`, `du` and `file` retain approval: a copied view must not pretend its inode/owner/block metadata is original project metadata, and more complex options need separate review. `tr` is available only as a constrained downstream stdin filter; see the pipeline guide. `find`, `sed`, `awk`, `xargs`, interpreters, Git, build/package tools and project scripts retain their existing permissions.

## Execution boundary

The parser returns an inspection plan with an immutable argv interface. It does not execute a shell. Every path operand passes the sensitive/generated/private-directory policy of native discovery plus the ordinary shell sandbox’s sensitive-name exclusions (for example `.netrc`, `.config` and `credentials.json`). These filename/path heuristics are not a general secret detector. The existing guard against machine-wide project roots (`/` or a root containing host HOME) is shared with ordinary shell execution. Before execution, descriptor-anchored non-following reads create a private temporary view containing only the requested permitted files/subtrees. Directory traversal omits symlinks and special files; an explicit symlink operand fails rather than following it. Replacing a project file with a symlink after capture cannot retarget the child's input.

File content capture checks size and metadata stability, including ctime. This is a bounded per-file snapshot, not an atomic multi-file checkout or a promise that the live project is unchanged. A later native write still needs its normal preimage checks. A changed/unstable source fails inspection rather than silently switching to broader shell execution.

`ls` captures names and directory structure without copying source bodies; metadata-dependent flags are excluded. Content commands copy at most 1 MiB per file / 8 MiB total. Traversal is capped at 2,000 entries, with recursion depth 16 per selected subtree; selected paths have at most 32 components. Commands are at most 16 KiB / 128 tokens. Over-budget capture returns a tool error; narrow the request, use native read/discovery, request an explicit user rerun or disable the profile. The model must not invent wrappers/options solely to evade the restriction.

Bubblewrap still supplies the existing network/PID/environment isolation and a read-only `/workspace`. Trusted system runtime directories remain available for executable/library loading; user file operands and file-opening options cannot address them through the supported language. The child receives the private view's descriptor, not the original project descriptor. Inspection uses a separate private scratch cache, so staging does not create application state inside the view or actual project.

Inspection stdout/stderr capture is bounded to 128 KiB plus one sentinel byte per stream, with an explicit incomplete-output note. Closing a full capture pipe can terminate a noisy command; do not interpret that as successful complete output. Existing tool presentation truncates to about 24 KiB plus scope/truncation notes. The 120-second execution timeout and user cancellation still apply. Isolation failure never retries on the host.

Temporary views/cache directories are private and removed on normal completion/error/cancellation. A process crash can leave private temporary files for OS cleanup; this is not a durable snapshot history or an expansion of GL-011 restore coverage. No model request is added by permission classification.

## Validation and remaining scope

Tests cover the supported option grammar, compound/substitution/wrapper/output-file rejection, private/outside/parent paths, symlink replacement, filtered traversal, content bounds, literal filenames and fixed-executable argv behavior. Policy tests cover opt-out, read-ask, shell-deny, existing exact grants, user reruns and stop-before-authorization. Sandbox argument tests retain network isolation/read-only mounts and prove no shell invocation; output-limit tests check bounded capture.

The normal suite passes 500 tests with three integration/profiling tests ignored. Explicitly attempting the new live Bubblewrap integration in this cloud host fails at `bwrap: setting up uid map: Read-only file system`, the same host restriction as the earlier integration test. It does not demonstrate successful live isolation here. Run on a suitable Linux host:

```bash
cargo test --locked sandbox::descriptor_tests::integration_project_inspection_executes_the_private_view -- --ignored
```

Supported pipe-only pipelines now share this boundary; see the [runtime guide](48-safe-inspection-pipelines.md) and [design](47-safe-inspection-pipeline-design.md). Wider syntax and option policies still require separate review.
