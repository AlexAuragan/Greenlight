# Actual Ratatui render captures

These PNG/SVG/text files come from the application's `view::draw` through Ratatui's `TestBackend`, with fictional run, failure and accounting fixtures. They are **actual implementation renders**, not the proportional drawings in [mockups](../mockups/README.md).

Each source cell's text, foreground, background and bold flag is copied to the image at its original grid position. The image renderer does not reposition widgets or add UI elements. PNG/SVG font rasterization can differ from your terminal; the `.txt` files show the exact character grid. These captures do not demonstrate interaction or substitute for PTY tests.

| View | 80×24 | 120×36 | 40×14 fallback |
| --- | --- | --- | --- |
| Result overview | [PNG](result-80x24.png) · [text](result-80x24.txt) | [PNG](result-120x36.png) | [PNG](result-40x14.png) |
| Artifacts | [PNG](artifacts-80x24.png) · [text](artifacts-80x24.txt) | [PNG](artifacts-120x36.png) | [PNG](artifacts-40x14.png) |
| Artifact reader | [PNG](artifact-reader-80x24.png) · [text](artifact-reader-80x24.txt) | — | — |
| Exact final-report reader | [PNG](report-reader-80x24.png) · [text](report-reader-80x24.txt) | — | — |
| Result after restore | [PNG](result-restored-80x24.png) · [text](result-restored-80x24.txt) | — | — |
| Tab focus: report action | [PNG](focus-report-80x24.png) · [text](focus-report-80x24.txt) | — | — |
| Tab focus: header Sessions | [PNG](focus-sessions-80x24.png) · [text](focus-sessions-80x24.txt) | — | — |
| Workflow selection | [PNG](create-workflow-80x24.png) | [PNG](create-workflow-120x36.png) | [PNG](create-workflow-40x14.png) |
| Goal editor | [PNG](create-goal-80x24.png) | [PNG](create-goal-120x36.png) | [PNG](create-goal-40x14.png) |
| Manager question | [PNG](compose-manager-80x24.png) | [PNG](compose-manager-120x36.png) | [PNG](compose-manager-40x14.png) |
| Step instructions | [PNG](compose-step-80x24.png) | [PNG](compose-step-120x36.png) | [PNG](compose-step-40x14.png) |


Attention/recovery captures use recorded fixture failures and real rendering; no provider request is made.

| State | 80×24 | 120×36 | 40×14 |
| --- | --- | --- | --- |
| Blocked | [PNG](attention-blocked-80x24.png) | [PNG](attention-blocked-120x36.png) | [PNG](attention-blocked-40x14.png) |
| Provider | [PNG](attention-provider-80x24.png) | [PNG](attention-provider-120x36.png) | [PNG](attention-provider-40x14.png) |
| Owner | [PNG](attention-owner-80x24.png) | [PNG](attention-owner-120x36.png) | [PNG](attention-owner-40x14.png) |
| Write | [PNG](attention-write-80x24.png) | [PNG](attention-write-120x36.png) | [PNG](attention-write-40x14.png) |
| Observing | [PNG](attention-observing-80x24.png) | — | — |
| Recovered | [PNG](attention-recovered-80x24.png) | — | — |
| Evidence | [PNG](attention-evidence-80x24.png) | — | — |


Run/usage captures use the existing ledger with fixture data; these screens do not dispatch requests or reprice historical calls.

| View | 80×24 | 120×36 | 40×14 |
| --- | --- | --- | --- |
| Run overview | [PNG](run-details-80x24.png) | [PNG](run-details-120x36.png) | [PNG](run-details-40x14.png) |
| Usage summary | [PNG](usage-summary-80x24.png) | [PNG](usage-summary-120x36.png) | [PNG](usage-summary-40x14.png) |
| Technical details | [PNG](run-technical-80x24.png) | — | — |
| Recorded requests | [PNG](usage-requests-80x24.png) | — | — |
| Exact request | [PNG](usage-request-80x24.png) | — | — |
| No recorded usage | [PNG](usage-empty-80x24.png) | — | — |

Regenerate from the repository root:

```bash
GREENLIGHT_UPDATE_CAPTURE_BASELINES=1 GREENLIGHT_CAPTURE_DIR=/tmp/greenlight-captures cargo test tui::presentation_tests -- --test-threads=1
python tests/render_tui_captures.py /tmp/greenlight-captures docs/screenshots
```

The optional PNG renderer uses Python Pillow and the DejaVu Sans Mono system font; neither is a Greenlight runtime dependency. Cargo tests require neither. Intermediate JSON stays outside the repository. The text fixtures for the 80×24 result/artifact list also serve as rendering regression baselines.

The explicit update flag skips only the two text-baseline comparisons while producing replacement captures. Review the generated files, then rerun tests without that flag to verify the checked-in baselines. Other render/focus assertions still run during capture updates.
