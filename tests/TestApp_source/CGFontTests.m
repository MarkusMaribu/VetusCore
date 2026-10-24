/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#include "system_headers.h"
#include <stdint.h>

CGFontRef CGFontCreateWithFontName(CFStringRef name);
CGRect CGFontGetFontBBox(CGFontRef font);
int CGFontGetUnitsPerEm(CGFontRef font);
int CGFontGetAscent(CGFontRef font);
CGGlyph CGFontGetGlyphWithGlyphName(CGFontRef font, CFStringRef name);
bool CGFontGetGlyphAdvances(CGFontRef font, const CGGlyph *glyphs,
                           size_t count, int *advances);
CFDataRef CGFontCopyTableForTag(CGFontRef font, uint32_t tag);

static int read_s16be(const UInt8 *p) {
  return (int16_t)(((uint16_t)p[0] << 8) | p[1]);
}

// Apple's API returns CGRect by value, so ARM supplies a hidden result pointer
// before the font argument. Treating that pointer as the font and writing an
// explicit out argument instead corrupts the font object's memory.
// https://developer.apple.com/documentation/coregraphics/cgfont/fontbbox
// Upstream has no CGFontGetFontBBox implementation to reuse:
// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/core_graphics/cg_font.rs
int test_CGFont_bbox_struct_return_preserves_font(void) {
  CGFontRef font = CGFontCreateWithFontName(CFSTR("Helvetica"));
  if (font == NULL) return -1;
  int result = 0;
  CFDataRef head = CGFontCopyTableForTag(font, 0x68656164); // 'head'
  if (head == NULL || CFDataGetLength(head) < 44) {
    result = -2;
    goto done;
  }
  const UInt8 *bytes = CFDataGetBytePtr(head);
  CGRect expected = CGRectMake(read_s16be(bytes + 36), read_s16be(bytes + 38),
      read_s16be(bytes + 40) - read_s16be(bytes + 36),
      read_s16be(bytes + 42) - read_s16be(bytes + 38));
  int upem = CGFontGetUnitsPerEm(font);
  int ascent = CGFontGetAscent(font);
  CGGlyph glyph = CGFontGetGlyphWithGlyphName(font, CFSTR("A"));
  int advance = 0;
  if (upem <= 0 || ascent <= 0 || glyph == 0 ||
      !CGFontGetGlyphAdvances(font, &glyph, 1, &advance) || advance <= 0) {
    result = -3;
    goto done;
  }

  // Repeat to exercise both the returned rectangle and continued font use.
  for (int i = 0; i < 3; i++) {
    CGRect box = CGFontGetFontBBox(font);
    if (box.origin.x != expected.origin.x || box.origin.y != expected.origin.y ||
        box.size.width != expected.size.width || box.size.height != expected.size.height ||
        box.size.width <= 0 || box.size.height <= 0) {
      result = -4;
      goto done;
    }
    int after_advance = 0;
    if (CGFontGetUnitsPerEm(font) != upem || CGFontGetAscent(font) != ascent ||
        CGFontGetGlyphWithGlyphName(font, CFSTR("A")) != glyph ||
        !CGFontGetGlyphAdvances(font, &glyph, 1, &after_advance) ||
        after_advance != advance) {
      result = -5;
      goto done;
    }
  }

done:
  if (head != NULL) CFRelease(head);
  CGFontRelease(font);
  return result;
}
