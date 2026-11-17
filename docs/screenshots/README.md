# Actual Ratatui render captures

These PNG/SVG/text files come from the application's `view::draw` through Ratatui's `TestBackend`, with fictional completed-run fixtures. They are **actual implementation renders**, not the proportional drawings in [mockups](../mockups/README.md).

Each source cell's text, foreground, background and bold flag is copied to the image at its original grid position. The image renderer does not reposition widgets or add UI elements. PNG/SVG font rasterization can differ from your terminal; the `.txt` files show the exact character grid. These captures do not demonstrate interaction or substitute for PTY tests.

| View | 80×24 | 120×36 | 40×14 fallback |
| --- | --- | --- | --- |
| Result overview | [PNG](result-80x24.png) · [text](result-80x24.txt) | [PNG](result-120x36.png) | [PNG](result-40x14.png) |
| Artifacts | [PNG](artifacts-80x24.png) · [text](artifacts-80x24.txt) | [PNG](artifacts-120x36.png) | [PNG](artifacts-40x14.png) |
| Artifact reader | [PNG](artifact-reader-80x24.png) · [text](artifact-reader-80x24.txt) | — | — |
| Exact final-report reader | [PNG](report-reader-80x24.png) · [text](report-reader-80x24.txt) | — | — |
| Result after restore | [PNG](result-restored-80x24.png) · [text](result-restored-80x24.txt) | — | — |

Regenerate from the repository root:

```bash
GREENLIGHT_CAPTURE_DIR=/tmp/greenlight-captures cargo test tui::presentation_tests -- --test-threads=1
python tests/render_tui_captures.py /tmp/greenlight-captures docs/screenshots
```

The optional PNG renderer uses Python Pillow and the DejaVu Sans Mono system font; neither is a Greenlight runtime dependency. Cargo tests require neither. Intermediate JSON stays outside the repository. The text fixtures for the 80×24 result/artifact list also serve as rendering regression baselines.
