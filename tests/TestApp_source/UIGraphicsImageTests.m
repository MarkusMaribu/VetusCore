/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"

void UIGraphicsBeginImageContext(CGSize size);
void UIGraphicsEndImageContext(void);
CGContextRef UIGraphicsGetCurrentContext(void);
void *CGBitmapContextGetData(CGContextRef context);
void CGContextDrawImage(CGContextRef context, CGRect rect, CGImageRef image);
void CGImageRelease(CGImageRef image);

// Transparent source pixels must preserve existing image-context contents.
// https://developer.apple.com/documentation/uikit/uigraphicsbeginimagecontextwithoptions(_:_:_:)
int test_UIGraphics_transparent_images_preserve_background(void) {
  unsigned char source[] = {255, 0, 0, 255, 0, 0, 0, 0};
  CGContextRef src = CGBitmapContextCreate(source, 2, 1, 8, 8, NULL, 1 | 0x4000);
  CGImageRef image = CGBitmapContextCreateImage(src);
  UIGraphicsBeginImageContext(CGSizeMake(2, 1));
  CGContextRef context = UIGraphicsGetCurrentContext();
  unsigned char *pixels = CGBitmapContextGetData(context);
  int result = 0;
  if (!context || !pixels) result = -1;
  else {
    for (int i = 0; i < 8; i++) if (pixels[i] != 0) result = -2;
    CGContextSetRGBFillColor(context, 0, 0, 1, 1);
    CGContextFillRect(context, CGRectMake(0, 0, 2, 1));
    CGContextDrawImage(context, CGRectMake(0, 0, 2, 1), image);
    if (pixels[0] != 255 || pixels[1] != 0 || pixels[2] != 0 || pixels[3] != 255) result = -3;
    if (pixels[4] != 0 || pixels[5] != 0 || pixels[6] != 255 || pixels[7] != 255) result = -4;
  }
  UIGraphicsEndImageContext();
  CGImageRelease(image);
  CGContextRelease(src);
  return result;
}
