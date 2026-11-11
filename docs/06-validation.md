# Validation status and smoke-test plan

## Validation performed for this source release

- All 13 Markdown step files parsed as YAML frontmatter with matching filenames and declared verification policies.
- All workflow step lists, transitions, conditional goto targets, detector prompts and Markdown includes were checked for file existence and syntactic consistency.
- The release ZIP passed an integrity read.

**Not performed:** Rust compilation, Rust unit tests, provider API integration, host command execution, terminal approval interaction, Discord webhook delivery or end-to-end workflow smoke tests. The authoring environment did not contain Rust/Cargo and had no package-download network access. Any compilation or runtime issues remain possible.

## Required validation on a machine with Rust

```bash
cargo fmt --check
cargo check
cargo test
cargo run -- validate fix-bug
cargo run -- validate add-feature
cargo run -- validate optimize-performance
```

If `cargo fmt --check` reports differences, run `cargo fmt` to normalize source formatting. This does not alter the workflow YAML files.

With `DEEPINFRA_API_KEY` configured:

```bash
cargo run -- models
cargo run -- start fix-bug --goal 'Investigate why my test suite fails'
```

For the first live session, use a disposable, trusted Git repository, and keep the host tool approval policy at `ask`. Validate artifacts, blocked-state behavior, provider output handling and event history before running on an important repository.

## Security checks before wider use

- Install Bubblewrap and run `cargo run -- sandbox-check`; the sandbox must refuse to start when user namespaces are unavailable.
- Verify sandboxed commands cannot write project source files and do not inherit `DEEPINFRA_API_KEY`, the host home directory or its SSH credentials.
- Check that pre-approved shell commands still run with network isolation; use native write for project edits.
- Project source files remain readable; masking known secret filenames is not comprehensive secret scanning. Avoid embedding secrets in source.
- Test Ctrl-C during a sandboxed process and verify descendants terminate.
- A successful restore does not certify files modified by processes outside the harness.
