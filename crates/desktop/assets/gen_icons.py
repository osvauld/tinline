#!/usr/bin/env python3
"""Renders the tray icons (3 states x light/dark panel x 16/22/24/32 px) and the window icon from
the SVGs in docs/design/Main.dc.html. Needs rsvg-convert. The output is checked in; rerun only
when the design changes:  python3 crates/desktop/assets/gen_icons.py"""
import pathlib
import subprocess

OUT = pathlib.Path(__file__).parent / "tray"
OUT.mkdir(exist_ok=True)


def cans(c, string, sw):
    return (f'<rect x="1.5" y="8" width="6" height="8" rx="1.5" fill="{c}"/>'
            f'<rect x="16.5" y="8" width="6" height="8" rx="1.5" fill="{c}"/>'
            f'<path d="M8.5 12 Q12 14.5 15.5 12" fill="none" stroke="{string}" '
            f'stroke-width="{sw}" stroke-linecap="round"/>')


def hollow(c):
    return (f'<rect x="2.25" y="8.75" width="4.5" height="6.5" rx="1" fill="none" stroke="{c}" stroke-width="1.6"/>'
            f'<rect x="17.25" y="8.75" width="4.5" height="6.5" rx="1" fill="none" stroke="{c}" stroke-width="1.6"/>'
            f'<path d="M9 12h1.5M13.5 12H15" stroke="{c}" stroke-width="2" stroke-linecap="round"/>')


# panel theme -> (can colour, unavailable colour, in-call string colour)
THEMES = {"light": ("#0B6B5B", "#4D5853", "#B87400"), "dark": ("#6FD3BD", "#A6B2AD", "#E8B04A")}


def render(body, viewbox, size, path):
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {viewbox} {viewbox}">{body}</svg>'
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(path)],
                   input=svg.encode(), check=True)


for theme, (can, dim, thread) in THEMES.items():
    for size in (16, 22, 24, 32):
        sw = 2.2 if size <= 16 else 2.0
        render(cans(can, can, sw), 24, size, OUT / f"available-{theme}-{size}.png")
        render(hollow(dim), 24, size, OUT / f"unavailable-{theme}-{size}.png")
        render(cans(can, thread, 2.6 if size <= 16 else 2.4), 24, size, OUT / f"incall-{theme}-{size}.png")

# Window / app icon: the adaptive icon composed on a rounded square.
icon = ('<rect width="108" height="108" rx="24" fill="#0B6B5B"/>'
        '<rect x="20" y="43" width="20" height="22" rx="4" fill="#F5F6F3"/>'
        '<ellipse cx="40" cy="54" rx="4" ry="11" fill="#9FD6C8" stroke="#F5F6F3" stroke-width="2"/>'
        '<rect x="68" y="43" width="20" height="22" rx="4" fill="#F5F6F3"/>'
        '<ellipse cx="68" cy="54" rx="4" ry="11" fill="#9FD6C8" stroke="#F5F6F3" stroke-width="2"/>'
        '<path d="M44 54 Q54 62 64 54" fill="none" stroke="#E8B04A" stroke-width="3" stroke-linecap="round"/>')
render(icon, 108, 128, OUT / "app-128.png")
