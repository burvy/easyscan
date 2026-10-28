"""Generates the EasyScan launcher icon from one geometry definition.

Usage: python tools/make_icon.py [--color #RRGGBB] [--preview DIR]
Writes (default): adaptive-icon vectors + background colour, legacy mipmap
PNGs under android/app/src/main/res, and store/icon-512.png (no alpha).
With --preview DIR only renders preview PNGs (no project files touched).
Needs Pillow. Geometry is on the 108dp adaptive canvas; the glyph stays inside
the 66dp safe zone (21..87).
"""
import argparse, os, sys
from PIL import Image, ImageDraw

COLOR = "#E64A19"  # orange, picked by burvy (028)
PAPER = "#FFFFFF"
LINE_ALPHA = None
RES = os.path.join("android", "app", "src", "main", "res")

# Page 32x42 centred on 108x108, top-right corner folded (cut 11), inside four
# viewfinder brackets (arm 10, thickness 3) spanning 28..80 so "scan a page".
PAGE = [(38, 33), (59, 33), (70, 44), (70, 75), (38, 75)]
FOLDT = [(59, 33), (59, 44), (70, 44)]
LINES = [(43, 53, 65, 56), (43, 62, 65, 65)]  # x1,y1,x2,y2
_A, _B, _T, _L = 28, 80, 3, 11  # bracket box, thickness, arm length
BRACKETS = [  # each an L-shaped polygon
    [(_A, _A), (_A + _L, _A), (_A + _L, _A + _T), (_A + _T, _A + _T), (_A + _T, _A + _L), (_A, _A + _L)],
    [(_B, _A), (_B, _A + _L), (_B - _T, _A + _L), (_B - _T, _A + _T), (_B - _L, _A + _T), (_B - _L, _A)],
    [(_A, _B), (_A, _B - _L), (_A + _T, _B - _L), (_A + _T, _B - _T), (_A + _L, _B - _T), (_A + _L, _B)],
    [(_B, _B), (_B - _L, _B), (_B - _L, _B - _T), (_B - _T, _B - _T), (_B - _T, _B - _L), (_B, _B - _L)],
]


def fold_color(color):
    """Darker tint of the white page toward the background colour (30%)."""
    c = rgb(color)
    return "#%02X%02X%02X" % tuple(round(255 * 0.7 + v * 0.3) for v in c)


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def draw_glyph(d, s, color, paper=PAPER, lines=True):
    k = s / 108
    P = lambda pts: [(x * k, y * k) for x, y in pts]
    for br in BRACKETS:
        d.polygon(P(br), fill=paper)
    d.polygon(P(PAGE), fill=paper)
    d.polygon(P(FOLDT), fill=fold_color(color))
    if lines:
        for x1, y1, x2, y2 in LINES:
            d.rounded_rectangle([x1 * k, y1 * k, x2 * k, y2 * k], radius=1.5 * k, fill=color)


def render(size, color, shape="full"):
    """shape: full (square bleed), circle, squircle, round (rounded square)."""
    ss = size * 4
    img = Image.new("RGB", (ss, ss), rgb(color))
    draw_glyph(ImageDraw.Draw(img), ss, color)
    img = img.resize((size, size), Image.LANCZOS)
    if shape == "full":
        return img
    mask = Image.new("L", (ss, ss), 0)
    md = ImageDraw.Draw(mask)
    if shape == "circle":
        md.ellipse([0, 0, ss - 1, ss - 1], fill=255)
    else:
        r = ss * (0.30 if shape == "squircle" else 0.18)
        md.rounded_rectangle([0, 0, ss - 1, ss - 1], radius=r, fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    return out


def path(pts):
    return "M" + " L".join(f"{x},{y}" for x, y in pts) + " Z"


def vector(name, body):
    return f"""<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="108dp" android:height="108dp"
    android:viewportWidth="108" android:viewportHeight="108">
{body}</vector>
"""


def write(p, text):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", newline="\n") as f:
        f.write(text)


def lines_xml(color_attr, rest):
    return "".join(
        f'    <path android:fillColor="{color_attr}" android:pathData="M{x1},{y1} L{x2},{y1} L{x2},{y2} L{x1},{y2} Z"/>\n'
        for x1, y1, x2, y2 in LINES)


def generate(color):
    res = RES
    write(f"{res}/values/ic_launcher_background.xml",
          f'<?xml version="1.0" encoding="utf-8"?>\n<resources>\n    <color name="ic_launcher_background">{color}</color>\n</resources>\n')
    fg = "".join(
        f'    <path android:fillColor="{PAPER}" android:pathData="{path(b)}"/>\n' for b in BRACKETS)
    fg += (f'    <path android:fillColor="{PAPER}" android:pathData="{path(PAGE)}"/>\n'
           f'    <path android:fillColor="{fold_color(color)}" android:pathData="{path(FOLDT)}"/>\n'
           + lines_xml(color, None))
    write(f"{res}/drawable/ic_launcher_foreground.xml", vector("fg", fg))
    # Themed icon (Android 13+): single-colour silhouette; the system tints it.
    # The fold and text lines are cut out of the page by even-odd fill.
    mono = (f'    <path android:fillColor="#FFFFFF" android:fillType="evenOdd" android:pathData="{path(PAGE)} '
            + " ".join(path(b) for b in BRACKETS) + " "
            + " ".join(f"M{x1},{y1} L{x2},{y1} L{x2},{y2} L{x1},{y2} Z" for x1, y1, x2, y2 in LINES)
            + '"/>\n')
    write(f"{res}/drawable/ic_launcher_monochrome.xml", vector("mono", mono))
    write(f"{res}/mipmap-anydpi-v26/ic_launcher.xml",
          '<?xml version="1.0" encoding="utf-8"?>\n<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
          '    <background android:drawable="@color/ic_launcher_background"/>\n'
          '    <foreground android:drawable="@drawable/ic_launcher_foreground"/>\n'
          '    <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>\n</adaptive-icon>\n')
    for d, px in {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}.items():
        render(px, color, "round").save(f"{res}/mipmap-{d}/ic_launcher.png")
    os.makedirs("store", exist_ok=True)
    render(512, color, "full").convert("RGB").save("store/icon-512.png")


def preview(outdir, colors):
    os.makedirs(outdir, exist_ok=True)
    shapes = ["circle", "squircle", "round", "full"]
    cw = 230
    sheet = Image.new("RGB", (len(shapes) * cw + 20 + 4 * 70, len(colors) * cw + 20), "#EEEEEE")
    for r, c in enumerate(colors):
        for i, sh in enumerate(shapes):
            im = render(200, c, sh).convert("RGBA")
            sheet.paste(im, (20 + i * cw, 20 + r * cw), im)
        # real-size row: 48px (mdpi) as shipped, plus a 96px reference (2x)
        for j, sh in enumerate(["circle", "round"]):
            im = render(48, c, sh).convert("RGBA")
            sheet.paste(im, (20 + 4 * cw + j * 60, 20 + r * cw), im)
        im = render(96, c, "circle").convert("RGBA")
        sheet.paste(im, (20 + 4 * cw, 20 + r * cw + 70), im)
    sheet.save(os.path.join(outdir, "icon-preview.png"))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--color", default=COLOR)
    ap.add_argument("--preview")
    ap.add_argument("--options", nargs="*")
    a = ap.parse_args()
    if a.preview:
        preview(a.preview, a.options or [a.color])
    else:
        generate(a.color)
