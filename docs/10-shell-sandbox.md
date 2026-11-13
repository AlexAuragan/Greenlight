# Linux shell sandbox

All Greenlight-initiated shell commands run through Bubblewrap. **There is no unsandboxed fallback**. This applies to agent shell actions, `command` workflow steps and command verification checks. Shell approval prompts and persistent exact-command grants still apply, but approval never expands the sandbox.

## Requirements and health check

On Linux, install Bubblewrap (on Arch: `sudo pacman -S bubblewrap`; on other distributions use the `bubblewrap` package). The executable must exist at `/usr/bin/bwrap`, and the kernel must allow unprivileged user namespaces.

Run `cargo run -- sandbox-check`, or `greenlight --root PROJECT sandbox-check` when installed. It confirms that the project is read-only and the build cache and `/tmp` are writable. If Bubblewrap is missing or cannot establish namespaces, shell operations fail explicitly. Other commands such as `history` remain available.

## Filesystem, environment and network

- The project is mounted **read-only** at `/workspace`, which is the working directory. Greenlight's native `write` tool is the only supported way to edit project source through the agent. Its approval, diff and edit journal remain intact.
- Selected system binaries and libraries and a small set of files in `/etc` are mounted read-only. `/home` and `/run` are private temporary filesystems. The normal host home directory and arbitrary host paths are not mounted.
- A writable cache is mounted at `/cache`, backed by `<project>/.greenlight/state/sandbox-cache` (Git-ignored and hidden under the read-only project mount). `/tmp` is writable but ephemeral. Shell writes into these locations are **not journaled or restored**.
- The shell's environment is **cleared**. Explicit variables include `PATH`, `HOME=/home/agent`, `TMPDIR=/tmp`, `PYTHONDONTWRITEBYTECODE=1`, `CARGO_TARGET_DIR=/cache/cargo-target`, `CARGO_HOME=/cache/cargo-home`, and `CARGO_NET_OFFLINE=true`. API keys and other user environment variables are not inherited. Native web search remains a separately approved tool.
- For Rust projects, user-installed `.cargo/bin`, `.rustup` and `.cargo/registry` may be mounted read-only under sandbox-specific paths to support toolchains and offline dependencies. `.cargo/credentials.toml` and the rest of the host home directory are never mounted.
- Known secret/internal paths such as `.env`, `.env.*`, `.git`, `.ssh`, `.aws`, `.config` and `.greenlight/state` are masked inside the project. Very large dependency directories (e.g. `node_modules`, `target`, virtualenvs) are not exhaustively scanned. **This is not a full secret detector:** values embedded in source or unfamiliar credential filenames can still be visible.
- A fresh user, PID, IPC, UTS and network namespace is created, with a new session and parent-death handling. Network is disabled. `curl`, `pip install` and similar downloads inside the shell will fail.

## Compatibility and limits

This intentionally breaks shell commands that edit the project, write Git metadata, use arbitrary home files, or access the internet. Use the native `write` tool for edits. Run dependency installation or Git operations manually outside the agent. Tests that write snapshots or generated assets into the source tree must use `/tmp` or `/cache`, or run outside the agent.

The sandbox does **not** set cgroup CPU/memory quotas or provide a transactional filesystem. A malicious workload can still exhaust local resources, exploit vulnerabilities in the host kernel or interact with its writable cache. Inspect projects containing secrets before executing untrusted code.
