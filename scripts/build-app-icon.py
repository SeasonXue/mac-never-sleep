#!/usr/bin/env python3
"""Apply the macOS rounded-square alpha mask and rebuild packaging/AppIcon.icns."""

from pathlib import Path
import subprocess
import tempfile

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
MASTER = ROOT / "packaging" / "AppIcon.png"
OUTPUT = ROOT / "packaging" / "AppIcon.icns"
CANVAS = 1024
CORNER_RADIUS = 220


def masked_master() -> Image.Image:
    image = Image.open(MASTER).convert("RGBA").resize((CANVAS, CANVAS), Image.Resampling.LANCZOS)
    scale = 4
    mask = Image.new("L", (CANVAS * scale, CANVAS * scale), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle(
        (0, 0, CANVAS * scale - 1, CANVAS * scale - 1),
        radius=CORNER_RADIUS * scale,
        fill=255,
    )
    mask = mask.resize((CANVAS, CANVAS), Image.Resampling.LANCZOS)
    image.putalpha(mask)
    image.save(MASTER, optimize=True)
    return image


def main() -> None:
    image = masked_master()
    sizes = {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }
    with tempfile.TemporaryDirectory(prefix="never-sleep-icon-") as tmp:
        iconset = Path(tmp) / "AppIcon.iconset"
        iconset.mkdir()
        for name, size in sizes.items():
            image.resize((size, size), Image.Resampling.LANCZOS).save(iconset / name, optimize=True)
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUTPUT)], check=True)


if __name__ == "__main__":
    main()
