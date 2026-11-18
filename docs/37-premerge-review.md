# Pre-merge review — 2026-11-19

The P0/P1 checklist and major UI slices are implemented on `feat/install-and-project-layout`. This review closes the two gaps found during real-project testing: native file discovery and action-envelope handling. The branch remains unmerged for user acceptance.

## Resolved findings

- Directory reads previously rejected `.` or failed as file reads, leaving the model to guess source paths after a shell denial. Existing `read` workflows now have bounded directory/path discovery under the same host read policy, with no configuration migration. See [discovery and limits](36-native-file-discovery.md).
- The old parser rejected commentary around an otherwise valid fenced action, stopped at a fence inside literal content, and could silently ignore a second action block. The new envelope parser accepts one unambiguous fenced mapping, preserves literal contents, rejects ambiguity and retains the raw response. Missing required fields now trigger protocol correction before dispatch. See [the current contract](13-agent-action-protocol.md).
- Regular file reads checked size after loading the entire file. Reads now reject oversized/non-regular files before loading and cap the actual read to handle growth after the check.
- Cleanup removed five existing test-code Clippy warnings, corrected stale README capability/key descriptions, and marked the original UI handoff as historical with current implementation references. Provider retries, approval dismissal, usage accounting and guarded restoration retain their existing behavior.

## Verification

- 355 Rust tests passed; one existing environment-dependent sandbox test remains ignored.
- All-target Clippy passed with warnings treated as errors; offline build passed.
- Six installer tests passed using local Git/build fixtures.
- A local HTTP/CLI workflow repaired malformed replies, discovered/read a real fixture source with shell denied, preserved nested artifact fences, executed no ambiguous write, and exported an exact prepared-message trace.
- Native PTY checks passed for permission popup capture/dismiss/reopen and HTTP 429 retry with identical messages, one prior tool execution, one branch/attempt and separate usage records.

No paid provider calls or claims of real-model compatibility beyond the tested contract are part of this verification.

## Acceptance before merge

In an existing project, start a short read-only investigation that needs to discover a source file. Check that Activity records `read` on `.` (and a returned source path) without a shell approval, and that the conclusion cites actual inspected source. Exercise Esc/reopen on a permission popup and Retry request after a provider failure when available. Review Edits/artifacts and the conversation dump before accepting the run.

Listings are scoped and may be truncated; they are not content search or a complete index. Ambiguous replies still require model correction. Tool-operation retries, durable provider continuations after restart, broader shell grants and manager live routing remain the explicitly tracked P2 work. No automatic replay of possibly completed tools is introduced here.
