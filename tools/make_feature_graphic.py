"""Generate EasyScan's Play Store feature graphic.

Usage (from the repository root): python tools/make_feature_graphic.py
Requires Pillow, as does make_icon.py. Writes store/feature-1024x500.png as
an opaque RGB PNG (1024 × 500 pixels), using the app icon's existing glyph.
"""
from pathlib import Path
import sys

from PIL import Image, ImageDraw, ImageFont

sys.dont_write_bytecode = True
from make_icon import COLOR, render

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "store" / "feature-1024x500.png"
BG = COLOR
PAPER = "#FFF8EF"
INK = "#352018"
LABEL = "#60301F"


def font(size: int) -> ImageFont.FreeTypeFont:
    # Pillow's bundled scalable default face keeps regeneration independent
    # of platform-specific system fonts.
    return ImageFont.load_default(size=size)


def draw_lines(draw: ImageDraw.ImageDraw, lines: list[str], xy: tuple[int, int], face: ImageFont.FreeTypeFont, fill: str, gap: int, *, bold: bool = False) -> None:
    x, y = xy
    for line in lines:
        box = draw.textbbox((x, y), line, font=face, stroke_width=1 if bold else 0)
        draw.text((x, y), line, font=face, fill=fill, stroke_width=1 if bold else 0)
        y += box[3] - box[1] + gap


def main() -> None:
    image = Image.new("RGB", (1024, 500), BG)
    # The familiar launcher glyph, on the same orange field as the app icon.
    mark = render(350, BG, "full")
    image.paste(mark, (8, 75))

    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((370, 42, 984, 458), radius=34, fill=PAPER)

    draw.text((426, 80), "EASYSCAN  |  HOMEWORK SCANNER", font=font(20), fill=LABEL, stroke_width=1)
    draw_lines(
        draw,
        ["Homework", "in one clean PDF."],
        (426, 127),
        font(54),
        INK,
        3,
        bold=True,
    )
    draw_lines(
        draw,
        ["Straighten pages, add notes and export a", "multi-page PDF, all from your phone."],
        (428, 278),
        font(25),
        LABEL,
        8,
    )

    # A clear action-like label, not a Play Store button or a false claim.
    pill = (428, 376, 666, 422)
    draw.rounded_rectangle(pill, radius=23, fill=INK)
    draw.text((450, 389), "SCAN  |  EDIT  |  SHARE", font=font(17), fill="#FFFFFF", stroke_width=1)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    image.save(OUT, format="PNG", optimize=True)
    print(f"Wrote {OUT.relative_to(ROOT)} ({image.width}×{image.height}, RGB)")


if __name__ == "__main__":
    main()
