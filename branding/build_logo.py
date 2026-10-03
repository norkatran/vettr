#!/usr/bin/env python3
"""Generates the vettr logo masters (symbol, wordmark, lockups) as clean SVG paths.

Run from anywhere:  python3 branding/build_logo.py
Writes into branding/masters/. Dependency-free. The wordmark letters are constructed from arcs, polygons and
rectangles (no fonts, no <text>), so the files are final outlines.

Geometry notes
- Symbol: two rounded squares of the same size, the second shifted by one band width down and right.
  Only the non-overlapping parts ("the delta") are inked: an L for what was (ink) and an L for what
  is now (accent). Corners where the two Ls meet are rounded so they never touch at a point.
- Wordmark: geometric monoline lowercase, x-height 100, stem 22, rounds 23. The two t's share one crossbar,
  drawn in the accent colour. All shapes in a glyph are wound the same way and filled non-zero, so overlaps union.
"""

import math
import os

INK = "#12141A"
ACCENT = "#12A474"
PAPER = "#F4F5F7"
ACCENT_DARK = "#3DDC97"

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "masters")
EXPORTS = os.path.join(HERE, "exports")


def f(v):
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


# ---------------------------------------------------------------- symbol


def old_l(offset, size, r, centre, tip):
    """Segments of the 'what was' L: the first square minus its overlap with the second."""
    x0 = centre - (size + offset) / 2
    x1 = x0 + size
    o = x0 + offset
    r2 = offset - r if tip else 0
    seg = [("M", x0 + r, x0), ("H", x1 - r), ("A", r, 1, x1, x0 + r)]
    seg += [("A", r2, 1, x1 - r2, o)] if r2 else [("V", o)]
    seg += [("H", o + r), ("A", r, 0, o, o + r)]
    if r2:
        seg += [("V", x1 - r2), ("A", r2, 1, o - r2, x1)]
    else:
        seg += [("V", x1), ("H", x0 + r)]
    seg += [("A", r, 1, x0, x1 - r), ("V", x0 + r), ("A", r, 1, x0 + r, x0), ("Z",)]
    return seg


def emit(seg, fx=lambda x: x, fy=lambda y: y):
    out = []
    for s in seg:
        c = s[0]
        if c == "M":
            out.append(f"M{f(fx(s[1]))} {f(fy(s[2]))}")
        elif c == "H":
            out.append(f"H{f(fx(s[1]))}")
        elif c == "V":
            out.append(f"V{f(fy(s[1]))}")
        elif c == "A":
            out.append(f"A{f(s[1])} {f(s[1])} 0 0 {s[2]} {f(fx(s[3]))} {f(fy(s[4]))}")
        else:
            out.append("Z")
    return "".join(out)


def delta_paths(offset=32, size=144, r=24, centre=128, tip=True, dx=0, dy=0):
    """Returns (old_L, new_L) path strings. The new L is the old one turned 180 degrees."""
    seg = old_l(offset, size, r, centre, tip)
    old = emit(seg, lambda x: x + dx, lambda y: y + dy)
    new = emit(seg, lambda x: 2 * centre - x + dx, lambda y: 2 * centre - y + dy)
    return old, new


def svg(view, body, title, w=None, h=None):
    vb = view
    if w is None:
        parts = [float(p) for p in view.split()]
        w, h = parts[2], parts[3]
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="{f(w)}" height="{f(h)}" role="img">'
        f"<title>{title}</title>{body}</svg>\n"
    )


def p(d, fill):
    return f'<path fill="{fill}" d="{d}"/>'


def symbol_svg(ink, accent, title, offset=32, size=144, tip=True, view="0 0 256 256"):
    old, new = delta_paths(offset=offset, size=size, tip=tip)
    return svg(view, p(old, ink) + p(new, accent), title)


# ---------------------------------------------------------------- wordmark


class Pen:
    """Builds path data with a uniform scale and offset applied."""

    def __init__(self, s=1.0, tx=0.0, ty=0.0):
        self.s, self.tx, self.ty = s, tx, ty
        self.d = []

    def X(self, x):
        return f(self.tx + self.s * x)

    def Y(self, y):
        return f(self.ty + self.s * y)

    def R(self, r):
        return f(self.s * r)

    def rect(self, x, y, w, h):
        self.d.append(f"M{self.X(x)} {self.Y(y)}H{self.X(x + w)}V{self.Y(y + h)}H{self.X(x)}Z")

    def rrect(self, x, y, w, h, r):
        R = self.R(r)
        self.d.append(
            f"M{self.X(x + r)} {self.Y(y)}H{self.X(x + w - r)}A{R} {R} 0 0 1 {self.X(x + w)} {self.Y(y + r)}"
            f"V{self.Y(y + h - r)}A{R} {R} 0 0 1 {self.X(x + w - r)} {self.Y(y + h)}H{self.X(x + r)}"
            f"A{R} {R} 0 0 1 {self.X(x)} {self.Y(y + h - r)}V{self.Y(y + r)}A{R} {R} 0 0 1 {self.X(x + r)} {self.Y(y)}Z"
        )

    def ring(self, cx, cy, ro, ri):
        Ro, Ri = self.R(ro), self.R(ri)
        self.d.append(
            f"M{self.X(cx - ro)} {self.Y(cy)}A{Ro} {Ro} 0 0 1 {self.X(cx + ro)} {self.Y(cy)}"
            f"A{Ro} {Ro} 0 0 1 {self.X(cx - ro)} {self.Y(cy)}Z"
            f"M{self.X(cx - ri)} {self.Y(cy)}A{Ri} {Ri} 0 0 0 {self.X(cx + ri)} {self.Y(cy)}"
            f"A{Ri} {Ri} 0 0 0 {self.X(cx - ri)} {self.Y(cy)}Z"
        )

    def band(self, cx, cy, ro, ri, a0, a1):
        """Arc band from angle a0 to a1 (degrees, screen space, increasing = clockwise)."""

        def pt(r, a):
            return self.X(cx + r * math.cos(math.radians(a))), self.Y(cy + r * math.sin(math.radians(a)))

        large = 1 if (a1 - a0) > 180 else 0
        ox, oy = pt(ro, a0)
        ex, ey = pt(ro, a1)
        ix, iy = pt(ri, a1)
        sx, sy = pt(ri, a0)
        Ro, Ri = self.R(ro), self.R(ri)
        self.d.append(
            f"M{ox} {oy}A{Ro} {Ro} 0 {large} 1 {ex} {ey}L{ix} {iy}A{Ri} {Ri} 0 {large} 0 {sx} {sy}Z"
        )

    def poly(self, points):
        self.d.append("M" + "L".join(f"{self.X(x)} {self.Y(y)}" for x, y in points) + "Z")

    def path(self):
        return "".join(self.d)


XH, T, RO, RI = 100, 22, 50.75, 28  # x-height, straight stem, round outer/inner radius
ASC = 136  # the t stem is the tallest part of the word


def g_v(pen, x):
    # two diagonals, 24 units wide horizontally (about 22 across), meeting in a flat 24-unit foot
    pen.poly([(x + a, b) for a, b in ((0, -XH), (24, -XH), (48, -33.3), (72, -XH), (96, -XH), (60, 0), (36, 0))])
    return 96


def g_e(pen, x):
    cx, cy = x + RO, -50
    # the band runs on past the bar's lower edge (11.4 degrees) so the outer curve meets the bar cleanly
    pen.band(cx, cy, RO, RI, 48, 371.4)
    half = math.sqrt(RO**2 - 10**2)  # bar ends exactly on the outer curve
    pen.rect(cx - half, cy - 10, 2 * half, 20)
    return 2 * RO


def g_t(pen, x):
    """Stem, hook and tail. The crossbar is drawn separately: the two t's share one."""
    rc = 30
    pen.rect(x, -ASC, T, ASC - 41)
    pen.band(x + T / 2 + rc, -41, rc + T / 2, rc - T / 2, 90, 180)
    pen.rect(x + T / 2 + rc, -22, 21, 22)
    return 62


def g_r(pen, x):
    pen.rect(x, -XH, T, XH)
    pen.band(x + 40, -60, 40, 18, 180, 300)
    return 60


GLYPHS = [("v", g_v, 0), ("e", g_e, 14), ("t", g_t, 22), ("t", g_t, 18), ("r", g_r, 20)]


def wordmark_paths(s, tx, baseline):
    """Returns (ink_path, accent_path, width_in_px) for the word at scale s."""
    ink = Pen(s, tx, baseline)
    accent = Pen(s, tx, baseline)
    x = 0.0
    stems = []
    for ch, fn, gap in GLYPHS:
        x += gap
        if ch == "t":
            stems.append(x)
        x += fn(ink, x)
    # one crossbar runs through both t's, in the accent colour: the mark's "added" green
    left, right = stems[0] - 10, stems[1] + 50
    accent.rect(left, -XH, right - left, 20)
    return ink.path(), accent.path(), x * s


# ---------------------------------------------------------------- lockups

SYMBOL_H = 176  # drawn height of the symbol in its 256 box (40..216)
WORD_S = 1.1  # wordmark scale in the lockups


def lockup_horizontal(ink, accent, title):
    s = WORD_S
    tx = 40 + SYMBOL_H + 56
    baseline = 128 + 0.5 * ASC * s  # the word's ink box is centred on the symbol
    word, bar, w = wordmark_paths(s, tx, baseline)
    old, new = delta_paths()
    width = math.ceil(tx + w + 40)
    body = p(old, ink) + p(new, accent) + p(word, ink) + p(bar, accent)
    return svg(f"0 0 {f(width)} 256", body, title)


def lockup_stacked(ink, accent, title):
    s = WORD_S
    w_est = wordmark_paths(s, 0, 0)[2]
    width = math.ceil(w_est + 80)
    old, new = delta_paths(dx=width / 2 - 128)
    baseline = 40 + SYMBOL_H + 56 + ASC * s
    word, bar, _ = wordmark_paths(s, (width - w_est) / 2, baseline)
    height = math.ceil(baseline + 40)
    body = p(old, ink) + p(new, accent) + p(word, ink) + p(bar, accent)
    return svg(f"0 0 {f(width)} {f(height)}", body, title)


def wordmark_only(ink, accent, title):
    s = 1.2
    w_est = wordmark_paths(s, 0, 0)[2]
    width = math.ceil(w_est + 80)
    word, bar, _ = wordmark_paths(s, (width - w_est) / 2, 40 + ASC * s)
    return svg(f"0 0 {width} {math.ceil(ASC * s) + 80}", p(word, ink) + p(bar, accent), title)


def tile_svg(title, scale, offset, size, tip, radius):
    """Dark rounded tile with the on-dark symbol centred on it (app icon / favicon)."""
    old, new = delta_paths(offset=offset, size=size, tip=tip)
    body = (
        f'<rect width="256" height="256" rx="{radius}" fill="{INK}"/>'
        f'<g transform="translate({f(128 - 128 * scale)} {f(128 - 128 * scale)}) scale({scale})">'
        f"{p(old, PAPER)}{p(new, ACCENT_DARK)}</g>"
    )
    return svg("0 0 256 256", body, title)


def main():
    os.makedirs(OUT, exist_ok=True)

    def write(name, text):
        with open(os.path.join(OUT, name), "w", encoding="utf-8") as fh:
            fh.write(text)

    t = "vettr"
    os.makedirs(EXPORTS, exist_ok=True)
    # one-colour versions: every shape in a single colour (the two Ls then read as one framed shape)
    for name, build in (
        ("symbol", lambda c: symbol_svg(c, c, f"{t} symbol")),
        ("horizontal", lambda c: lockup_horizontal(c, c, f"{t} logo")),
        ("stacked", lambda c: lockup_stacked(c, c, f"{t} logo")),
        ("wordmark", lambda c: wordmark_only(c, c, f"{t} wordmark")),
    ):
        for tone, colour in (("black", "#000000"), ("white", "#FFFFFF")):
            with open(os.path.join(EXPORTS, f"vettr-{name}-{tone}.svg"), "w", encoding="utf-8") as fh:
                fh.write(build(colour))
    write("vettr-symbol.svg", symbol_svg(INK, ACCENT, f"{t} symbol"))
    write("vettr-symbol-on-dark.svg", symbol_svg(PAPER, ACCENT_DARK, f"{t} symbol", offset=29))
    # small-size cut: thicker bands, no tip gap, tight crop
    write(
        "vettr-symbol-small.svg",
        symbol_svg(INK, ACCENT, f"{t} symbol", offset=36, size=140, tip=False, view="28 28 200 200"),
    )
    # app icon: rounded tile (Linux/desktop); full-bleed square for stores that apply their own mask
    write("vettr-app-icon.svg", tile_svg(f"{t} app icon", 0.82, 29, 144, True, 56))
    write("vettr-app-icon-square.svg", tile_svg(f"{t} app icon", 0.74, 29, 144, True, 0))
    write("vettr-favicon.svg", tile_svg(f"{t} favicon", 1.0, 36, 140, False, 48))
    write("vettr-horizontal.svg", lockup_horizontal(INK, ACCENT, f"{t} logo"))
    write("vettr-horizontal-on-dark.svg", lockup_horizontal(PAPER, ACCENT_DARK, f"{t} logo"))
    write("vettr-stacked.svg", lockup_stacked(INK, ACCENT, f"{t} logo"))
    write("vettr-stacked-on-dark.svg", lockup_stacked(PAPER, ACCENT_DARK, f"{t} logo"))
    write("vettr-wordmark.svg", wordmark_only(INK, ACCENT, f"{t} wordmark"))
    write("vettr-wordmark-on-dark.svg", wordmark_only(PAPER, ACCENT_DARK, f"{t} wordmark"))


if __name__ == "__main__":
    main()
