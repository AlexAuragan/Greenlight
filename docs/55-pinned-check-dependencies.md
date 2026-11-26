# Pinned dependencies and offline check preparation

GL-034 adds a separate artifact-fetch operation to [isolated checks](54-isolated-verification.md). Downloading bytes, installing them, and running a check have different authority. A check never acquires network access because dependencies are missing.

## Recipe configuration

Extend a named check in the host-owned project configuration. This template needs real, independently reviewed direct URLs and SHA-256 values before use:

```yaml
checks:
  python-tests:
    requires: [python3]
    preparation:
      artifacts:
        - name: example-1.0-py3-none-any.whl
          url: https://artifacts.example.com/example-1.0-py3-none-any.whl
          sha256: <64-lowercase-hex-from-reviewed-lock>
        # Include the complete pinned offline dependency set, including transitives.
      run: |
        python3 -m venv /cache/venv &&
        /cache/venv/bin/python -m pip install --no-index --find-links=/deps --require-hashes -r requirements.lock
    command: /cache/venv/bin/python -m pytest
```

`preparation.artifacts` holds unique filenames, exact public URLs and mandatory digests. `preparation.run` is an optional offline command executed afresh inside each disposable check workspace. Either artifacts or a setup command is required when `preparation` is present. Checks without preparation retain their previous behavior and recipe fingerprints.

Python wheels, reviewed source archives and complete offline Node/pre-commit cache bundles are possible inputs. Greenlight does not discover a package graph, generate pins, run `pip`/`npm`/Git on the host, extract archives on the host, or execute downloaded helpers there. A user-authored offline setup recipe must prepare the required environment inside the sandbox. `requires` lists installed sandbox bootstrap programs, such as `python3`, rather than executables that will only exist after setup.

Pre-commit remote hook environments need a complete pinned offline bundle or a suitable local/system-hook recipe. Running the framework's normal online installation does not gain network permission. Absolute interpreter paths in imported environment archives must match the sandbox layout; arbitrary host virtualenvs are not mounted. Supported-host tests and timing measurements remain GL-035.

URLs, commands and digests appear in approval/history/provenance records. They must not contain credentials. Model tools cannot alter the host-owned recipe configuration or substitute arbitrary URL/digest arguments.

## Fetch explicitly, then run offline

```sh
greenlight check-prepare SESSION_ID python-tests
greenlight check SESSION_ID python-tests
greenlight check-show SESSION_ID RUN_ID
```

`check-prepare` authorizes only the currently configured pinned fetch manifest. It does not install dependencies or execute a check. `check` consumes verified local artifacts and performs any configured offline installation before running its command. Missing, changed or corrupt preparation returns unavailable rather than automatically downloading. Explicit CLI selection provides user intent; it does not bypass `web: deny` for downloads, `shell: deny` for checks, or the sandbox boundary.

For an agent to request a fetch, explicitly enable `prepare_check` in the workflow step's tools. It is not in the default tool set:

```yaml
action: tool
tool: prepare_check
input: python-tests
```

The existing TUI approval popup shows the exact artifact URLs, names, digests and limits. A general web grant, `web: allow`, shell grant or check approval does not authorize artifact fetching. Fetch, offline installation and check grants are independent and scoped to the exact recipe fingerprint for the current process. Editing a command, pin, URL, setup or declared requirement invalidates those grants. Agent checks ask separately for offline installation and the check itself, before either executes. Dismissing a popup still waits rather than deciding.

Historical `prepare_check` reruns preserve and validate the original recipe fingerprint, including failures whose diagnostics contain external text. A changed recipe needs a fresh explicit request. No automatic network or installation retries occur. A stopped/failed preparation can leave already verified cache objects, but does not publish a new ready manifest; an older valid manifest, if any, remains subject to integrity rechecking.

## Fetch boundary and cancellation

Only direct HTTPS URLs on port 443 are accepted. IP literals, local hostnames, authentication/userinfo, queries, fragments, control characters and redirects are rejected. Signed/redirecting release URLs, credentialed registries, custom ports and proxy-only networks are not supported by this initial broker. Use a stable direct public endpoint instead of weakening the policy.

DNS results are checked against local/private/reserved/unsupported ranges; mixed public/private results refuse the fetch. The validated addresses are pinned into the HTTP client for the original hostname, retaining normal TLS certificate verification. Redirects, cookies/authentication configuration and environment proxies cannot introduce another destination. TLS roots are the existing Reqwest/Rustls defaults; no project-provided CA or downloader is loaded.

DNS uses only the fixed trusted system command `/usr/bin/getent ahosts -- HOST`, under the invocation supervisor with a cleared environment and `/` working directory. This is host name resolution, not execution of project/package code. The helper rejects other databases, arbitrary arguments and arbitrary host programs. Resolver execution has a ten-second deadline and GL-033 cleanup reporting; it does not leave an uncancellable Tokio blocking-pool resolver behind. Stop during DNS awaits that cleanup. After resolution, cancellation closes the asynchronous HTTP operation. Unconfirmed resolver cleanup keeps project execution quarantined.

Limits are 16 artifacts per recipe, 32 MiB per artifact, 128 MiB per preparation and 60 seconds per DNS/download operation (approval time is excluded). Filenames are short ASCII basenames and digests are 64 lowercase hex characters. Both advertised Content-Length and streamed bytes are bounded. Bodies must match the declared SHA-256 before publication. HTTP failures retain status and a body excerpt of at most 4 KiB; DNS, TLS, cancellation and integrity errors retain their actual reason. A digest match establishes identity, not that package code is safe.

The broker makes only fixed GET requests and never executes downloaded/project code. The trusted system resolver is the sole additional supervised host utility. There is no general host-command API or host fallback for checks.

## Offline installation and provenance

Each check gets its own private source copy, writable `/cache` and a **read-only `/deps`** containing freshly hash-verified copies of the declared artifacts. It never receives writable cache-object inodes or the original cache directory. Offline setup and the check share that one job-local environment. Another check gets a fresh cache/venv; model-modified environments are not reused on the host or by another invocation.

`PIP_NO_INDEX=1`, pip's version check disablement and `npm_config_offline=true` supplement the sandbox's physical network isolation. `PRE_COMMIT_HOME`, Python, XDG and Cargo writable cache paths remain job-local. Environment variables are an aid, not the security boundary: overriding them does not enable network or host writes.

Setup failure is an unavailable preparation, with the check marked not started. Its exit status/output and any permitted source modifications are retained separately. Cleanup errors remain fatal and cannot be hidden by composite verification. Setup writes do not become live edits; source diffs still require the existing guarded user import. A passing check against setup-modified source cannot approve unchanged live source.

Activity shows artifact fetching, integrity verification, offline installation and check execution separately. `check-show` retains the original setup command, full bounded setup output, verified artifact metadata and recipe/dependency fingerprints. Branch-visible verification exports include dependency events. Prepared/cleared cache facts also invalidate stale manager/continuation context.

## Cache lifecycle

Objects and ready manifests live under private `.greenlight/state/check-dependencies/{objects,manifests}`. Objects are content-addressed, names are private and records capture declared URLs, digests, verified bytes, preparation time and the full recipe fingerprint. This is declared origin plus integrity evidence; a cached object is labelled as cached, not as a fresh download.

A ready manifest is published only after every artifact verifies. Every later check rechecks the bytes before copying them into its job. Changing a recipe requires a new explicit preparation even if its objects can be reused without network requests. Artifacts can be shared within this project's cache; writable installed environments cannot. Another project's cache or host HOME is not mounted.

Clear the project's prepared artifacts explicitly when idle:

```sh
greenlight check-dependencies-clear SESSION_ID --yes
```

The operation takes normal project/session ownership, refuses outstanding cleanup and validates every cache entry before removing any. It removes only recognized dependency objects/manifests, leaves source and check evidence untouched, and makes subsequent checks require preparation again. There is no automatic garbage collection of active workspaces or a persistent installed-environment cache. Disk/CPU/PID quotas and global retention enforcement remain GL-023; these per-operation limits are not a whole-project disk quota.

## Validation scope

Tests use pinned local fixture bytes, fixed loopback HTTP responses and existing supervised process fixtures. They verify URL/address policy, digest and streaming bounds, exact HTTP error excerpts, independent authorization, unchanged legacy fingerprints, private staging, cache tampering, cancellation and cleanup. They do not make public artifact requests or paid provider calls.

Successful live sandbox installation, readonly artifact mounts, two fresh Python virtualenv runs, real public TLS/DNS downloads and representative Node/pre-commit recipes still need GL-035 on an appropriate host. The cloud host continues to reject UID-map setup; it tests explicit unavailability and cleanup, not successful isolation.
