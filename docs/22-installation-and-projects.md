# Installation and projects

Greenlight runs on Linux. Building requires Git, Rust/Cargo 1.89 or newer and a C compiler. Shell execution additionally requires `/usr/bin/bwrap` and enabled unprivileged user namespaces; use `greenlight sandbox-check`. There is no unsandboxed fallback.

Run `bash install.sh` from a checkout to fetch GitHub's master, build a native release and install a user-local `greenlight`. During feature review, use `--ref feat/install-and-project-layout`; this branch must be available remotely. After merge, use `--ref master` to select the default release line.

## Installer behavior

```bash
bash install.sh
bash install.sh --bin-dir "$HOME/.local/bin"
bash install.sh --ref master
# Isolated installation or alternate checkout:
bash install.sh --repository /path/to/Greenlight --ref branch-name \
  --data-dir /tmp/greenlight-install --bin-dir /tmp/greenlight-bin
```

Defaults: cache `${XDG_DATA_HOME:-$HOME/.local/share}/greenlight`; binary at the previous installation location, an existing writable user-local Greenlight on PATH, or `~/.local/bin`. The cache records repository, selected ref, binary directory and source/installed revisions. `greenlight --version` reports the compiled revision. Add the printed directory to PATH and run `hash -r` if your shell still selects an older executable.

Rerunning the installer fetches and builds the remembered ref. Updates happen when you run the installer, not at application startup. Restart running processes to use the new executable. The installer never initializes or rewrites projects, installs system dependencies, uses sudo or edits shell profiles. Fetch/build/executable-validation failures preserve the installed binary. Local source modifications, origin changes, same-ref rewinds, unmanaged directories and concurrent installations are refused. If an interrupted process leaves `install.lock`, check that no installation is running before removing that empty lock directory. Keep custom development work in a separate checkout.

## Project setup

```bash
cd /path/to/project
greenlight init
greenlight validate fix-bug
greenlight
```

Initialization creates `.greenlight/config.yaml`, `instructions.md`, `.env.example`, `.gitignore`, three workflows and their shared prompt fragments, plus `state/`. All resources are embedded in the executable; no source checkout is needed afterward. Reinitializing fills missing files and preserves existing content, settings and session history. Initialization refuses symlinked control directories/template files. It is not a migration that overwrites custom workflows.

Edit provider/model and host policies in `.greenlight/config.yaml`. Supply credentials through exported environment variables or `.greenlight/.env`; exported values win, followed by local dotenv values and then the selected project's root `.env`. The caller's unrelated dotenv file is not read. Secrets and runtime state are ignored by the generated `.gitignore`; project configuration, workflow definitions and instructions can be committed. Agent tools cannot read/write private config, dotenv or internal runtime state, and workspace restore cannot modify those settings.

Bare `greenlight` opens the TUI. Other commands discover the nearest ancestor project marked by `.greenlight`, `.stepwright` or a supported root config; discovery stops at a Git boundary. `greenlight init` always initializes the current directory. `greenlight --root /path/to/project ...` explicitly selects that directory. Projects have separate histories, approvals and caches under `.greenlight/state/`.

Configuration precedence: `.greenlight/config.yaml`, `greenlight.yaml`, then `stepwright.yaml`. An invalid preferred file is an error. Workflow lookup uses `.greenlight/workflows/` when present, otherwise root `workflows/`. Existing legacy configuration and root workflows remain selected when initializing a legacy project; existing legacy instructions are preserved too. Moving old workflow trees also requires moving their shared include fragments or updating relative include paths. Existing `.stepwright/state` migration retains its conflict checks; see [migration notes](15-name-migration.md).
