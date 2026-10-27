#!/usr/bin/env python3
"""Build the GFL-licensed Heros substitutes used at legacy guest font paths.

Requires fonttools==4.61.1. No proprietary font is an input to this process.
See touchHLE_fonts/README-Alpine-Emulation-Sans.txt for provenance and changes.
"""

import argparse
import copy
from pathlib import Path
import shutil

from fontTools.fontBuilder import FontBuilder
from fontTools.misc.roundTools import otRound
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.scaleUpem import scale_upem


ROOT = Path(__file__).resolve().parent.parent
FONT_DIR = ROOT / "touchHLE_fonts"
UPM = 2048
# Keep the guest's existing em, line spacing and capital height. Horizontal
# Heros advances, rounded to 2048 UPM, already match the legacy ASCII advances.
FACES = (
    dict(source="texgyreheros_regular.otf", output="Arial.ttf", style="Regular",
         ascent=1854, descent=-434, gap=67, typo=(1491, -431, 307),
         win=(1854, 434), cap=1466, selection=64),
    dict(source="texgyreheros_bold.otf", output="helvetica-bold.ttf", style="Bold",
         ascent=1577, descent=-471, gap=0, typo=(1536, -348, 61),
         win=(1966, 475), cap=1474, selection=32),
)
DESCRIPTION = (
    "Alpine Emulation Sans, a modified TeX Gyre Heros font. Converted from "
    "CFF to TrueType outlines, normalized to 2048 units per em, with adjusted "
    "vertical outlines and line metrics for emulator compatibility. Legacy "
    "filenames are guest path aliases, not the identity of this font. "
    "See README-Alpine-Emulation-Sans.txt for the change log, source and license. "
    "Modified version maintained by Markus Maribu / Krystall Media; "
    "markus@krystall.media. Original authors do not provide support for it."
)


def generate(face):
    source = TTFont(FONT_DIR / "sources" / face["source"], recalcTimestamp=False)
    scale_upem(source, UPM)
    glyph_set = source.getGlyphSet()
    # Scaling UPM must not accidentally scale the glyphs twice. Apply only
    # the small vertical correction needed to match the legacy capital height.
    y_scale = face["cap"] / source["OS/2"].sCapHeight
    glyphs = {}
    for name in source.getGlyphOrder():
        pen = TTGlyphPen(None)
        curves = Cu2QuPen(pen, max_err=0.5, reverse_direction=True)
        glyph_set[name].draw(TransformPen(curves, (1, 0, 0, y_scale, 0, 0)))
        glyphs[name] = pen.glyph()

    builder = FontBuilder(UPM, isTTF=True)
    builder.setupGlyphOrder(source.getGlyphOrder())
    builder.setupCharacterMap(source.getBestCmap())
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({
        name: (advance, glyphs[name].xMin if glyphs[name].numberOfContours else 0)
        for name, (advance, _) in source["hmtx"].metrics.items()
    })
    builder.setupHorizontalHeader(
        ascent=face["ascent"], descent=face["descent"], lineGap=face["gap"],
    )
    family = "Alpine Emulation Sans"
    builder.setupNameTable({
        "familyName": family,
        "styleName": face["style"],
        "uniqueFontIdentifier": f"AlpineEmulationSans-{face['style']}-1.000",
        "fullName": f"{family} {face['style']}",
        "psName": f"AlpineEmulationSans-{face['style']}",
        "version": "Version 1.000; modified TeX Gyre Heros",
        "copyright": source["name"].getDebugName(0),
        "description": DESCRIPTION,
        "licenseDescription": "GUST Font License (GFL), LPPL 1.3c or later. "
            "See LICENSE.heros and LICENSE.lppl-1.3c.",
        "licenseInfoURL": "https://www.tug.org/fonts/licenses/gfl.html",
    })
    builder.setupOS2()
    builder.font["OS/2"] = copy.deepcopy(source["OS/2"])
    os2 = builder.font["OS/2"]
    os2.sTypoAscender, os2.sTypoDescender, os2.sTypoLineGap = face["typo"]
    os2.usWinAscent, os2.usWinDescent = face["win"]
    os2.fsSelection = face["selection"]
    os2.sCapHeight = face["cap"]
    os2.sxHeight = otRound(os2.sxHeight * y_scale)
    os2.fsType = 0  # No embedding restriction: the GFL permits redistribution.
    builder.setupPost(keepGlyphNames=True,
                      underlinePosition=source["post"].underlinePosition,
                      underlineThickness=source["post"].underlineThickness)
    # Retain OpenType substitutions and positioning (already rescaled to UPM).
    for tag in ("GDEF", "GPOS", "GSUB"):
        if tag in source:
            builder.font[tag] = copy.deepcopy(source[tag])
    builder.font["head"].created = source["head"].created
    builder.font["head"].modified = 3875817600  # 2026-10-26, fixed for reproducibility.
    builder.font["head"].macStyle = 1 if face["style"] == "Bold" else 0
    target = FONT_DIR / face["output"]
    builder.save(target)

    # Reopen the actual serialized result, including its cubic-to-quadratic
    # conversion, rather than only checking in-memory builder values.
    result = TTFont(target)
    cmap = result.getBestCmap()
    assert result["head"].unitsPerEm == UPM
    assert "glyf" in result and "CFF " not in result
    assert result["glyf"][cmap[ord("H")]].yMax == face["cap"]
    for code in range(32, 127):
        name = cmap[code]
        assert result["hmtx"][name][0] == source["hmtx"][name][0]
    print(f"{target.name}: {len(cmap)} Unicode characters, TrueType, "
          f"{UPM} UPM, cap height {face['cap']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--copy-to", type=Path,
                        help="Also copy outputs and notices to an app asset directory")
    args = parser.parse_args()
    for face in FACES:
        generate(face)
    if args.copy_to:
        args.copy_to.mkdir(parents=True, exist_ok=True)
        for name in [face["output"] for face in FACES] + [
            "LICENSE.heros", "LICENSE.lppl-1.3c", "README-Alpine-Emulation-Sans.txt",
        ]:
            shutil.copyfile(FONT_DIR / name, args.copy_to / name)


if __name__ == "__main__":
    main()
