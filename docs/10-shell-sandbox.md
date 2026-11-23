# Linux shell sandbox

All Greenlight-initiated shell commands run through Bubblewrap. **There is no unsandboxed fallback**. This applies to agent shell actions, `command` workflow steps and command verification checks. Shell approval prompts and persistent exact-command grants still apply, but approval never expands the sandbox.

## Requirements and health check

On Linux, install Bubblewrap (on Arch: `sudo pacman -S bubblewrap`; on other distributions use the `bubblewrap` package). The executable must exist at `/usr/bin/bwrap`, support `--ro-bind-fd` and `--bind-fd`, and the kernel must allow unprivileged user namespaces. Project and cache sources are pinned directory descriptors; unsupported Bubblewrap versions fail closed rather than reverting to pathname binds.

Run `cargo run -- sandbox-check`, or `greenlight --root PROJECT sandbox-check` when installed. It confirms that the project is read-only and the build cache and `/tmp` are writable. If Bubblewrap is missing or cannot establish namespaces, shell operations fail explicitly. Other commands such as `history` remain available.

## Filesystem, environment and network

- The project is mounted **read-only** at `/workspace`, which is the working directory. Greenlight's native `write` tool is the only supported way to edit project source through the agent. Its approval, diff and edit journal remain intact.
- Selected system binaries and libraries and a small set of files in `/etc` are mounted read-only. `/home` and `/run` are private temporary filesystems. The normal host home directory and arbitrary host paths are not mounted.
- A writable cache is mounted at `/cache`, backed by `<project>/.greenlight/state/sandbox-cache` (Git-ignored and hidden under the read-only project mount). `/tmp` is writable but ephemeral. Shell writes into these locations are **not journaled or restored**.
- The shell's environment is **cleared**. Explicit variables include `PATH`, `HOME=/home/agent`, `TMPDIR=/tmp`, `PYTHONDONTWRITEBYTECODE=1`, `CARGO_TARGET_DIR=/cache/cargo-target`, `CARGO_HOME=/cache/cargo-home`, and `CARGO_NET_OFFLINE=true`. API keys and other user environment variables are not inherited. Native web search remains a separately approved tool.
- For Rust projects, user-installed `.cargo/bin`, `.rustup` and `.cargo/registry` may be mounted read-only under sandbox-specific paths to support toolchains and offline dependencies. `.cargo/credentials.toml` and the rest of the host home directory are never mounted.
- Known secret/internal paths such as `.env`, `.env.*`, `.git`, `.ssh`, `.aws`, `.config`, `.greenlight/state` and legacy `.stepwright/state` are masked inside the project. Very large dependency directories (e.g. `node_modules`, `target`, virtualenvs) are not exhaustively scanned. **This is not a full secret detector:** values embedded in source or unfamiliar credential filenames can still be visible.
- A fresh user, PID, IPC, UTS and network namespace is created, with a new session and parent-death handling. Network is disabled. `curl`, `pip install` and similar downloads inside the shell will fail.

## Process lifecycle and cleanup

Each invocation runs under a small dedicated supervisor, established **before** Bubblewrap starts. It is a Linux child subreaper: orphaned descendants belong to this invocation even if namespace setup fails or they call `setsid`. It kills and reaps only its own children, repeating as deeper descendants are adopted. It never makes the main application a subreaper or uses global process-name killing.

The supervisor is compiled at build time and embedded in Greenlight. At runtime it executes from a sealed anonymous `memfd`; there is no second executable to install, temporary helper directory to retain, or host-command fallback. Linux must permit executable memfds and provide readable `/proc` process records. The production helper accepts only `/usr/bin/bwrap`; only test binaries embed the unrestricted fixture helper.

Single commands and structured pipelines use the same runner:

- Launch/execution has a shared 120-second deadline. User stop, capture overflow and partial launch failure cancel all started stages.
- Final stdout and combined stderr each have a 128 KiB hard capture cap, including ordinary shell commands. Exceeding a cap reports incomplete output and stops execution, rather than allocating an unbounded response. The tool's separate 24,000-byte presentation limit still applies.
- Capture and intermediate-pipe drains use nonblocking IO. Cancellation cannot leave a blocking-pool read holding the async runtime open.
- After cancellation or failure, Greenlight waits at most five seconds for supervisors to finish and pipes to close. A dropped execution future or exited Greenlight process closes a private control pipe; the supervisor continues cleanup independently.
- The supervisor also cleans up leftover background descendants after its primary command exits. Background shell work cannot survive as intentional detached work.

Private `.greenlight/state/sandbox-jobs/*.lease` records hold a lock and pending/complete state. Diagnostic supervisor/primary PIDs are advisory, never recovery targets. Only a supervisor that has reaped all its children records completion. Normal completed records are removed by the next ownership/cleanup check. An unused command or failed supervisor spawn is recorded clean because no sandbox child started.

If cleanup is incomplete or unconfirmed, the tool raises a fatal cleanup error; the workflow retains its durable owner. Owner release, new execution and session recovery refuse to proceed while a job is active or lacks completion evidence. A slow supervisor keeps its lock and continues cleanup after the five-second wait; once it finishes, explicit normal session recovery becomes possible. An unexpectedly killed/crashed supervisor leaves unconfirmed evidence even after its lock disappears. Inspect that record and the actual process state before manual reconciliation; do not delete pending records merely to bypass the quarantine. There is no automatic PID-based kill or unsafe force-recovery command.

This is lifecycle ownership, not a resource quota. Kernel-stuck processes, a compromised supervisor or power loss cannot be guaranteed to terminate on a timer; the application reports uncertainty and blocks further execution rather than declaring cleanup successful.

## Compatibility and limits

This intentionally breaks shell commands that edit the project, write Git metadata, use arbitrary home files, or access the internet. Use the native `write` tool for edits. Run dependency installation or Git operations manually outside the agent. Tests that write snapshots or generated assets into the source tree must use `/tmp` or `/cache`, or run outside the agent.

The sandbox does **not** set cgroup CPU/memory quotas or provide a transactional filesystem. A malicious workload can still exhaust local resources, exploit vulnerabilities in the host kernel or interact with its writable cache. Inspect projects containing secrets before executing untrusted code.
