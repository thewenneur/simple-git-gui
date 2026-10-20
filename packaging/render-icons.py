#!/usr/bin/env python3
"""Renders the app icon files from assets/icon/simple-git.svg.

Needs the `resvg` command (`cargo install resvg`) and Pillow (`pip install pillow`).
Run from anywhere: python3 packaging/render-icons.py
"""

import pathlib
import subprocess
import tempfile

from PIL import Image

ICON_DIR = pathlib.Path(__file__).resolve().parent.parent / "assets" / "icon"
SVG = ICON_DIR / "simple-git.svg"


def render(svg: pathlib.Path, size: int, out: pathlib.Path) -> Image.Image:
    subprocess.run(["resvg", "-w", str(size), "-h", str(size), str(svg), str(out)], check=True)
    return Image.open(out)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        # macOS icons leave a margin around the shape (about 9% on each side).
        padded = tmp / "padded.svg"
        padded.write_text(SVG.read_text().replace('viewBox="0 0 1024 1024"', 'viewBox="-112 -112 1248 1248"'))

        full = {n: render(SVG, n, tmp / f"{n}.png") for n in (16, 24, 32, 48, 64, 128, 256)}
        full[64].save(ICON_DIR / "simple-git-64.png")
        full[256].save(ICON_DIR / "simple-git-256.png")
        ico = [full[n] for n in (256, 128, 64, 48, 32, 24, 16)]
        ico[0].save(ICON_DIR / "simple-git.ico", sizes=[i.size for i in ico], append_images=ico[1:])

        mac = [render(padded, n, tmp / f"mac-{n}.png") for n in (1024, 512, 256, 128, 64, 32, 16)]
        mac[0].save(ICON_DIR / "simple-git.icns", append_images=mac[1:])


if __name__ == "__main__":
    main()
