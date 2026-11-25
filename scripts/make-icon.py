#!/usr/bin/env python3
"""Draw Packager's app icon.

With no arguments, writes every size into Packager/Assets.xcassets/AppIcon.appiconset.
Pass a .png path to write a single 1024-pixel image instead.
"""

import math
import pathlib
import struct
import sys
import zlib

SIZE = 1024


def paint_round_rect(buffer, left, top, width, height, radius, color):
    radius = min(radius, width // 2, height // 2)
    row_bytes = SIZE * 4
    for y in range(top, top + height):
        if y < 0 or y >= SIZE:
            continue
        if y < top + radius:
            dy = top + radius - y
        elif y >= top + height - radius:
            dy = y - (top + height - radius - 1)
        else:
            dy = 0
        inset = 0 if dy <= 0 else radius - int(math.sqrt(max(0, radius * radius - dy * dy)))
        start = left + inset
        end = left + width - inset
        start = max(0, start)
        end = min(SIZE, end)
        if end <= start:
            continue
        offset = y * row_bytes + start * 4
        buffer[offset : offset + (end - start) * 4] = color * (end - start)


def png_bytes(buffer):
    raw = bytearray()
    row_bytes = SIZE * 4
    for y in range(SIZE):
        raw.append(0)
        raw.extend(buffer[y * row_bytes : (y + 1) * row_bytes])

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def render():
    blue = bytes((36, 99, 235, 255))
    white = bytes((255, 255, 255, 255))
    ink = bytes((36, 99, 235, 255))
    buffer = bytearray(SIZE * SIZE * 4)
    paint_round_rect(buffer, 48, 48, 928, 928, 210, blue)
    paint_round_rect(buffer, 250, 390, 524, 300, 36, white)
    paint_round_rect(buffer, 300, 300, 424, 130, 28, white)
    paint_round_rect(buffer, 470, 250, 84, 70, 16, white)
    paint_round_rect(buffer, 300, 500, 424, 28, 8, ink)
    return png_bytes(buffer)


def write_appiconset(directory, png):
    import subprocess

    directory.mkdir(parents=True, exist_ok=True)
    master = directory / "icon_512x512@2x.png"
    master.write_bytes(png)
    sizes = [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ]
    images = []
    for pixels, name in sizes:
        destination = directory / name
        if destination != master:
            subprocess.check_call(
                ["sips", "-z", str(pixels), str(pixels), str(master), "--out", str(destination)],
                stdout=subprocess.DEVNULL,
            )
        point = pixels // 2 if "@2x" in name else pixels
        images.append(
            {
                "filename": name,
                "idiom": "mac",
                "scale": "2x" if "@2x" in name else "1x",
                "size": f"{point}x{point}",
            }
        )
    contents = {"images": images, "info": {"author": "xcode", "version": 1}}
    (directory / "Contents.json").write_text(json_contents(contents), encoding="utf-8")
    print(directory)


def json_contents(value):
    import json

    return json.dumps(value, indent=2) + "\n"


def main():
    default = pathlib.Path(__file__).resolve().parents[1] / "Packager/Assets.xcassets/AppIcon.appiconset"
    destination = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else default
    png = render()
    if destination.suffix.lower() == ".png":
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(png)
        print(destination)
        return
    write_appiconset(destination, png)


if __name__ == "__main__":
    main()
