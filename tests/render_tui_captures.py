"""Render exact Ratatui TestBackend cells; this does not design or rearrange the UI.

Usage: python tests/render_tui_captures.py INPUT_JSON_DIRECTORY OUTPUT_DIRECTORY
Requires Pillow for PNG output. SVG and text preserve the same terminal-cell grid.
"""
import json
import sys
from html import escape
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

source, output = map(Path, sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
fonts = {bold: ImageFont.truetype(
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono" + ("-Bold" if bold else "") + ".ttf", 16
) for bold in (False, True)}
for path in sorted(source.glob("*.json")):
    data = json.loads(path.read_text())
    width, height = data["width"], data["height"]
    image = Image.new("RGB", (width * 10, height * 20))
    draw = ImageDraw.Draw(image)
    svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width*10}" height="{height*20}" viewBox="0 0 {width*10} {height*20}">',
           f'<title>Actual Ratatui fixture render: {escape(path.stem)}</title>']
    for index, cell in enumerate(data["cells"]):
        x, y = (index % width) * 10, (index // width) * 20
        draw.rectangle((x, y, x+9, y+19), fill=cell["bg"])
        svg.append(f'<rect x="{x}" y="{y}" width="10" height="20" fill="{cell["bg"]}"/>')
    # Paint glyphs after backgrounds so a wide glyph is not erased by the next cell.
    for index, cell in enumerate(data["cells"]):
        if not cell["text"].strip():
            continue
        x, y = (index % width) * 10, (index // width) * 20
        draw.text((x, y), cell["text"], fill=cell["fg"], font=fonts[cell["bold"]])
        weight = 700 if cell["bold"] else 400
        svg.append(f'<text x="{x}" y="{y+16}" fill="{cell["fg"]}" font-family="DejaVu Sans Mono,monospace" font-size="16" font-weight="{weight}" xml:space="preserve">{escape(cell["text"])}</text>')
    svg.append('</svg>')
    image.save(output / (path.stem + ".png"))
    (output / (path.stem + ".svg")).write_text("\n".join(svg) + "\n")
    (output / (path.stem + ".txt")).write_text(path.with_suffix(".txt").read_text())
print(f"Rendered {len(list(source.glob('*.json')))} actual terminal-cell captures.")
