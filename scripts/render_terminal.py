#!/usr/bin/env python3
"""Render captured terminal output as a terminal-style PNG (for the README screenshots).

Usage: render_terminal.py OUT.png TITLE BLOCK [BLOCK ...]
  where each BLOCK is "COMMAND::path/to/output.txt". Lines are rendered verbatim; "ok", "passed",
  "rejected as expected" are tinted green and failures red, nothing else is changed.
"""
import sys
from PIL import Image, ImageDraw, ImageFont

FONT = "/System/Library/Fonts/Menlo.ttc"
SIZE, PAD, LINE = 15, 24, 21
BG, FG, DIM, GREEN, RED, PROMPT = "#0d1117", "#e6edf3", "#8b949e", "#3fb950", "#f85149", "#79c0ff"


def color(line: str) -> str:
    low = line.lower()
    if "failed" in low and "0 failed" not in low or "unexpected" in low or "wrong error" in low:
        return RED
    if line.rstrip().endswith(" ok") or "rejected as expected" in low or "test result: ok" in low \
            or "matched their expected outcome" in low or ": ok " in low:
        return GREEN
    if line.startswith("     Running") or line.startswith("Status history") or line.startswith("F001 end-to-end"):
        return PROMPT
    return FG


def main() -> None:
    out, title, blocks = sys.argv[1], sys.argv[2], sys.argv[3:]
    rows = []  # (text, color)
    for b in blocks:
        cmd, path = b.split("::", 1)
        rows.append((f"$ {cmd}", PROMPT))
        rows += [(l.rstrip("\n"), None) for l in open(path, encoding="utf-8")]
        rows.append(("", None))
    font = ImageFont.truetype(FONT, SIZE)
    width = max(int(font.getlength(t)) for t, _ in rows) + 2 * PAD
    height = 36 + PAD + LINE * len(rows) + PAD
    img = Image.new("RGB", (max(width, 900), height), BG)
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, img.width, 36], fill="#161b22")
    for i, c in enumerate(["#ff5f56", "#ffbd2e", "#27c93f"]):
        d.ellipse([16 + i * 22, 12, 28 + i * 22, 24], fill=c)
    d.text((img.width // 2 - int(font.getlength(title)) // 2, 9), title, font=font, fill=DIM)
    y = 36 + PAD
    for text, c in rows:
        d.text((PAD, y), text, font=font, fill=c or color(text))
        y += LINE
    img.save(out, optimize=True)
    print(f"wrote {out} ({img.width}x{img.height})")


if __name__ == "__main__":
    main()
