# Fonts

## Emulation font substitutes

`Arial.ttf` and `helvetica-bold.ttf` are legacy path aliases for modified
TeX Gyre Heros fonts, not the proprietary fonts their filenames resemble.
See `README-Alpine-Emulation-Sans.txt`, `LICENSE.heros`, and
`LICENSE.lppl-1.3c` for provenance, metric adjustments, and full licence terms.
Unmodified source fonts are in `sources/`; regeneration is handled by
`dev-scripts/generate-emulation-fonts.py`.

## Liberation fonts

These are taken from the Liberation fonts release 2.1.5: https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5

See `LICENSE.liberation` for the license details.

## Noto fonts

The Noto Sans CJK fonts are taken from the Noto Sans CJK release that was current as of 2023-01-28: https://github.com/googlefonts/noto-cjk/tree/main/Sans

Specifically these are "Region-specific Subset OTFs" for Japan.

The Noto Sans Arabic fonts (`NotoSansArabic-Regular.ttf`, `NotoSansArabic-Bold.ttf`) are taken from the Noto fonts project: https://github.com/googlefonts/noto-fonts/tree/main/hinted/ttf/NotoSansArabic

See `LICENSE.noto` for the license details (all of these are licensed under the SIL Open Font License).
