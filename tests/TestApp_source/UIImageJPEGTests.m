/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/. */
#import "system_headers.h"
@interface NSData (JPEGTest)
- (NSUInteger)length;
- (const void *)bytes;
@end
@interface UIImage (JPEGTest)
+ (instancetype)imageWithData:(NSData *)data;
- (CGImageRef)CGImage;
@end
NSData *UIImageJPEGRepresentation(UIImage *image, CGFloat quality);
size_t CGImageGetWidth(CGImageRef image);
size_t CGImageGetHeight(CGImageRef image);
void CGImageRelease(CGImageRef image);

// UIImageJPEGRepresentation must generate a real JPEG that decodes with its
// original dimensions; lower quality must change quantization/compression.
// https://developer.apple.com/documentation/uikit/uiimage/jpegdata(compressionquality:)
int test_UIImage_JPEG_representation(void) {
  if (UIImageJPEGRepresentation(nil, 1) != nil) return -1;
  unsigned char pixels[32 * 32 * 4];
  for (int y = 0; y < 32; y++) for (int x = 0; x < 32; x++) {
    unsigned char *p = pixels + (y * 32 + x) * 4;
    p[0] = x * 8; p[1] = y * 8; p[2] = (x * 37 + y * 71) % 256; p[3] = 255;
  }
  CGContextRef ctx = CGBitmapContextCreate(pixels, 32, 32, 8, 128, NULL, 1 | 0x4000);
  CGImageRef cg = CGBitmapContextCreateImage(ctx);
  UIImage *image = [UIImage imageWithCGImage:cg];
  NSData *high = UIImageJPEGRepresentation(image, 1);
  NSData *low = UIImageJPEGRepresentation(image, 0);
  int result = 0;
  if (!high || !low || [high length] < 4 || [low length] < 4) result = -2;
  else {
    const unsigned char *b = [high bytes];
    if (b[0] != 0xff || b[1] != 0xd8 || b[[high length]-2] != 0xff || b[[high length]-1] != 0xd9) result = -3;
    else if ([high length] <= [low length]) result = -4;
    else {
      UIImage *decoded = [UIImage imageWithData:high];
      if (!decoded || CGImageGetWidth([decoded CGImage]) != 32 || CGImageGetHeight([decoded CGImage]) != 32) result = -5;
    }
  }
  CGImageRelease(cg);
  CGContextRelease(ctx);
  return result;
}
