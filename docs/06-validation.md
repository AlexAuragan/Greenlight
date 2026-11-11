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

- Deny reading `.env`, `.git`, `.ssh` and `.stepwright/state` through built-in file tools. The shell is an unrestricted OS operation when approved, and may still access them.
- Inspect command permissions and avoid process-wide approvals during first smoke tests.
- Confirm that leaked credentials are not included in model output, tool results, event logs, artifacts or webhook messages.
- Test Ctrl-C while a shell command runs; timeout kills the immediate shell child, but not necessarily every descendant process.
- Do not assume that changing the active execution branch restores code files.
