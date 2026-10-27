Alpine Emulation Sans - modified TeX Gyre Heros
Modified on 26 October 2026 by Markus Maribu / Krystall Media
Contact for this modified version: markus@krystall.media
Maintenance status: maintained by Markus Maribu

The files Arial.ttf and helvetica-bold.ttf are compatibility path aliases
for Alpine Emulation Sans Regular and Bold. They contain no Arial or
Helvetica font software. The legacy filenames allow existing emulator
builds and guest applications to find replacement fonts without changing
their font lookup paths. They do not identify the font's author or licence.

Original work: TeX Gyre Heros, by the Polish TeX Users Group GUST;
TeX Gyre extensions by Boguslaw Jackowski and Janusz M. Nowacki on behalf
of TeX users groups. Original copyright notices are retained in the fonts.
The original authors do not provide support for this modified version.

Licence: GUST Font License, under LPPL 1.3c or later.
Full licence texts: LICENSE.heros and LICENSE.lppl-1.3c.
The upstream fonts and the complete original TeX Gyre work are available at:
https://www.gust.org.pl/projects/e-foundry/tex-gyre
https://ctan.org/pkg/tex-gyre
https://mirrors.ctan.org/fonts/tex-gyre.zip

Unmodified inputs in VetusCore: sources/texgyreheros_regular.otf and
sources/texgyreheros_bold.otf. These match Alpine Launcher's font resources
app/src/main/res/font/texgyreheros_regular.otf and texgyreheros_bold.otf.

Changes from the original work:
- Renamed the font family/PostScript identities to Alpine Emulation Sans.
- Converted CFF cubic outlines to TrueType quadratic outlines, maximum
  conversion error 0.5 design units at 2048 units per em.
- Normalized both faces to 2048 units per em without resizing their
  horizontal advance widths. Rounded ASCII advances match the legacy fonts.
- Adjusted vertical outlines to preserve the previous capital H heights:
  regular 1466/2048 em; bold 1474/2048 em.
- Preserved the previous hhea, typographic, and Windows line metrics and
  regular/bold selection flags, preventing a different ascent/descent
  normalization from changing the guest's font scale or line spacing.
- Retained upstream character maps, glyph names, substitutions and
  positioning tables. The upstream outlines and character coverage differ
  from the removed fonts; pixel-identical rendering is not promised.
- Added this modification/provenance notice to the font's own metadata.

Regeneration (FontTools 4.61.1):
python dev-scripts/generate-emulation-fonts.py
To copy generated fonts and notices into an Android host, also pass:
--copy-to PATH/TO/app/src/main/assets/touchHLE_fonts

No proprietary font is needed to regenerate the replacements. The
emulator's font-size calculations and UIKit fallback fonts are unchanged.
