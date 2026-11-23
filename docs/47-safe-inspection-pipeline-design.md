# Safe inspection pipelines — GL-031 design

**Design complete; runtime support is not implemented.** GL-012 still sends every pipeline to existing exact-command approval. GL-032 tracks implementation of this design. This document adds no accepted configuration or permission grant.

## Selected first scope

Support only 2–4 validated read-only stages connected by plain stdout pipes (`|`). Reuse the existing inspection policy gate, sensitive-path rules, fixed system executables and filtered snapshot boundary. No shell interpreter is involved.

A complete pipeline either qualifies for automatic inspection or follows exact-command approval as one original string. Do not run an approved prefix, ask about the remaining stages, or save grants for fragments. Existing exact grants, explicit shell allow/deny, read policy, user-requested reruns and profile opt-out retain their current precedence.

Examples describe the proposed behavior, not behavior available today:

| Input | Proposed handling |
| --- | --- |
| `cat src/main.rs \| grep -n TODO` | Eligible source plus stdin filter |
| `rg TODO src \| head -n 20` | Eligible source plus bounded reader |
| `cat notes \| sort \| uniq -c \| head -n 20` | Four eligible stages |
| `cat notes \| tr a-z A-Z` | Eligible source plus constrained character filter |
| `ls -a src \| head -n 20` | Names-only source; no unnecessary source-body capture |
| `cat notes \| ./danger.sh` | Exact approval for the entire original command |
| `cat notes \| grep TODO /etc/passwd` | Exact approval: a downstream stage is not stdin-only |
| `cat notes \| grep TODO other-project-file` | Exact approval: downstream file operands are excluded even when project-relative |
| `cat notes \| sort -o output.txt` | Exact approval: output-file option |
| `cat notes \| sh -c something` | Exact approval: interpreter/wrapper |
| `cat notes \|\| echo fallback` | Exact approval: logical OR, not a pipe |
| `cat notes \| grep TODO > result.txt` | Exact approval: redirection |
| `cat notes \|& grep TODO` | Exact approval: stderr piping is outside scope |
| `cat notes \| grep 'a\|b'` | Exact approval: quoted pipe literals remain outside the initial grammar |
| `cat notes \| grep TODO \|` | Exact approval: malformed/empty stage; run nothing beforehand |

Commands unavailable at the fixed trusted executable location cannot qualify. Pipeline support does not expand metadata-sensitive commands such as `ls -l`, `stat` or `du`.

## Input and command roles

Represent input explicitly in host plans rather than inferring it later from command text:

- **First stage:** one of GL-012's supported sources with its validated project file/directory operands, or `pwd`. Preserve the existing option restrictions and explicit-file requirements. `ls` needs only a names/structure view; content sources need bounded file bodies.
- **Later stages:** supported stdin filters only. Start with `grep`, `rg`, `head`, `tail`, `wc`, `sort`, `uniq`, `cut` and `tr`. They cannot independently open user-specified files or directories.
- **No disconnected branches:** a downstream file operand is rejected rather than silently ignoring or combining the incoming stream. Disallow recursive/file-discovery options in filter mode. In particular, piped `rg PATTERN` must read its incoming stream; it must not default to searching `.` as a first-stage `rg` can.

Keep GL-012's supported numeric/selection flags where meaningful. Exclude filename-listing and follow/early-search modes from the first filter grammar. `tr` is stdin-only: permit supported `-d`, `-s`, `-c` combinations and the required one or two literal character-set arguments; these are data, not paths. Unsupported quoted escapes or set syntax still require exact approval. Validate cardinality and option combinations before launch; do not introduce arbitrary file-reading flags or helper programs.

Use distinct source/filter validators and an immutable pipeline plan. Share the existing executable, option, path and budget policies; do not copy a second drifting allowlist. Neither the model nor saved grant files can provide a trusted serialized plan. The host derives it from the original command and captures its own evidence.

## Parsing before execution

Use a small explicit lexer for the supported language. It must distinguish unquoted `|` from quoted text and detect unsupported operators before any filesystem capture or child launch. **Do not split the raw string on `|` and assume each fragment is safe.**

Retain ordinary quoting for arguments with spaces. Backslashes, expansion, substitutions, redirects, grouping, newlines/control characters and other unsupported shell forms keep the conservative exact-approval fallback. Quoted pipe literals also fall back initially. Reject empty stages, `||`, `|&` and more than four stages.

Validate every stage's executable identity, flags, operand roles and input/output connection, then build the shared view. A validation or capture failure must start no process. Classification failure may use the original exact-approval path; a capture/resource failure after classification returns an inspection error instead of silently widening filesystem access.

## Execution architecture

Use one private filtered project view for the entire request and a separate private scratch cache. Capture it once under the existing anchored-read/stability rules. Do not capture a fresh live tree for each stage, expose the original project descriptor, or persist these temporary bodies as restore snapshots.

Start each fixed executable in its own existing Bubblewrap isolation, passing OS pipes between those sandboxed stages. The parent only coordinates validated processes and bounded byte streams. This reuses the current isolation machinery without adding an executable shell wrapper or a new privileged helper command. Every stage sees the same read-only view; no stage receives additional filesystem authority merely because it is downstream.

All validation/capture happens first, but process or namespace startup can still fail during launch. If any launch fails, kill and reap every already-started stage, close all pipes and return an explicit startup failure. Earlier validated read-only stages may have begun; do not claim that nothing ran or that the pipeline completed. Never retry outside isolation.

Drain stderr concurrently so a full error pipe cannot deadlock a producer. Close unused pipe ends in the parent and children, and avoid collecting entire intermediate outputs in memory. On cancellation, timeout, output-limit termination or partial startup failure, stop and reap all stages; do not leave upstream producers running or automatically replay the command. Measure the extra namespace startup cost before considering a more complex single-sandbox supervisor.

## Result semantics

A single shell tool result retains the original request and reports final stdout, labelled stderr and each stage's exit code or signal. Record pipeline inspection scope, truncation and cancellation explicitly. Do not present a successful final `head`/`wc` as evidence that all earlier stages succeeded.

Use a failure-aware aggregate status: the rightmost genuine nonzero stage status wins; otherwise the pipeline succeeds. Preserve each underlying status so ordinary meanings such as grep's no-match or diff's differences are visible rather than converted into permission denials.

Treat upstream SIGPIPE as an expected early close only when a validated downstream `head` completed successfully and there was no cancellation, timeout, output cap or other genuine stage failure. Keep that signal in the stage results with an explanatory label. Do not suppress arbitrary exit code 141, other signals, stderr, or every early consumer exit. Other early-close cases must remain visible and conservative until specifically supported. Output-limit closure is incomplete output, never a successful short read.

Each pipeline remains one recorded shell tool request/result, not several user-approved calls. No model inference is needed for parsing/authorization, and no stage output becomes an approval or workflow verification verdict.

## Shared limits

Retain GL-012's 16 KiB / 128-token input ceiling, now across the entire pipeline; allow at most four stages. Share the 1 MiB per-file, 8 MiB total body and 2,000-entry capture budgets across all stages. Names-only sources must not cause source-body copying merely because filters follow them.

Use a **single 120-second deadline** for launch plus execution, not 120 seconds per stage. Cap final stdout and aggregate labelled stderr to 128 KiB each plus fixed annotations/sentinel bookkeeping. Intermediate data stays in bounded OS pipes rather than unbounded vectors. Keep the existing roughly 24 KiB tool-result presentation limit and scope/truncation notes. These are capture/time limits, not a new claim of per-process CPU/address-space quotas; GL-023 remains separate.

## Acceptance checks before shipping GL-032

- Ordinary `cat | grep | head` and `sort | uniq` produce the expected data with no prompt under the inspection policy.
- Any invalid stage/operator/operand prevents all automatic stage launches; fallback approval receives the exact complete original command.
- Quoting, adjacent pipes, empty stages, `||`, `|&`, redirects, substitutions, wrappers, private/outside/symlink paths and leading-dash operands have regression coverage.
- Downstream `rg` consumes stdin; file operands and discovery/helper/output options cannot make filters read unrelated inputs or write files.
- One stable filtered view is used even if the live project changes while stages run; original-project descriptors never reach children.
- A first-stage error remains visible when the final stage exits zero. Successful short `head`, real upstream failure, grep no-match, binary data and output-cap SIGPIPE cases remain distinguishable.
- Partial launch failure, namespace failure, blocked producer, stderr pressure, timeout and cancellation stop/reap the full process set without host fallback or automatic replay.
- Existing exact grants, opt-out, deny/read-ask policies, user reruns and native-write permissions retain their behavior.
- Exercise live Bubblewrap isolation on a suitable Linux host. This cloud host's known read-only UID-map restriction cannot establish that integration result.

No runtime changes or new tests are introduced by this design. The current [simple-command policy](46-project-inspection-permissions.md) remains authoritative until GL-032 is implemented and validated.
