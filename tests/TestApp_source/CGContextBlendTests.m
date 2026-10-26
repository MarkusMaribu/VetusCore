/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
#include <stdlib.h>
#include <string.h>

void CGContextSetAlpha(CGContextRef, CGFloat);
void CGContextSaveGState(CGContextRef);
void CGContextRestoreGState(CGContextRef);
void CGContextClearRect(CGContextRef, CGRect);
void CGContextDrawImage(CGContextRef, CGRect, CGImageRef);
void CGImageRelease(CGImageRef);

// CGContext alpha applies to image and fill drawing; it is part of saved
// graphics state. Blend equations: Quartz 2D Programming Guide, Bitmap Images
// and Image Masks, "Using Blend Modes with Images".
// https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/drawingwithquartz2d/dq_images/dq_images.html
int test_CGContext_image_fill_alpha_and_blending(void) {
    unsigned char pixels[16] = {0};
    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(pixels, 1, 1, 8, 16, space,
                                            1); // kCGImageAlphaPremultipliedLast
    CGColorSpaceRelease(space);
    CGRect rect = CGRectMake(0, 0, 1, 1);
    CGContextSetRGBFillColor(ctx, 1, 0, 0, 1);
    CGContextSaveGState(ctx);
    CGContextSetAlpha(ctx, .25);
    CGContextFillRect(ctx, rect);
    if (abs(pixels[0] - 64) > 1 || abs(pixels[3] - 64) > 1) return -1;
    CGContextRestoreGState(ctx);
    CGContextClearRect(ctx, rect);
    CGContextFillRect(ctx, rect);
    CGImageRef red = CGBitmapContextCreateImage(ctx);
    if (pixels[0] != 255 || pixels[3] != 255) return -2;
    CGContextClearRect(ctx, rect);
    CGContextSetAlpha(ctx, .5);
    CGContextDrawImage(ctx, rect, red);
    if (abs(pixels[0] - 128) > 1 || abs(pixels[3] - 128) > 1) return -3;
    const int modes[] = {1, 2, 3, 10, 11};
    const unsigned char expected[][3] = {
        {0, 0, 0}, {255, 0, 255}, {0, 0, 255},
        {255, 0, 255}, {255, 0, 255}
    };
    CGContextSetAlpha(ctx, 1);
    for (unsigned i = 0; i < sizeof(modes)/sizeof(modes[0]); ++i) {
        CGContextSetBlendMode(ctx, 17); // Copy the blue backdrop.
        CGContextSetRGBFillColor(ctx, 0, 0, 1, 1);
        CGContextFillRect(ctx, rect);
        CGContextSetBlendMode(ctx, modes[i]);
        CGContextDrawImage(ctx, rect, red);
        for (unsigned c = 0; c < 3; ++c)
            if (abs(pixels[c] - expected[i][c]) > 1) return -4 - i;
        if (pixels[3] != 255) return -10;
    }
    CGContextSetBlendMode(ctx, 17);
    CGContextSetAlpha(ctx, .25);
    CGContextDrawImage(ctx, rect, red);
    if (abs(pixels[0] - 64) > 1 || abs(pixels[3] - 64) > 1) return -11;
    CGContextSetBlendMode(ctx, 16); // Clear ignores the paint color.
    CGContextDrawImage(ctx, rect, red);
    if (pixels[0] || pixels[1] || pixels[2] || pixels[3]) return -12;
    for (unsigned i = 4; i < sizeof(pixels); ++i)
        if (pixels[i] != 0) return -13; // Row padding is untouched.
    CGImageRelease(red);
    CGContextRelease(ctx);
    // A snapshot owns its pixels, and a premultiplied image stays correctly
    // premultiplied when opacity is applied to a little-endian BGRA bitmap.
    unsigned char source[] = {64, 32, 0, 128};
    CGContextRef src = CGBitmapContextCreate(source, 1, 1, 8, 4, NULL, 1);
    CGImageRef translucent = CGBitmapContextCreateImage(src);
    CGContextClearRect(src, rect);
    CGContextRelease(src);
    memset(pixels, 0, sizeof(pixels));
    ctx = CGBitmapContextCreate(pixels, 1, 1, 8, 16, NULL, 2 | 0x2000);
    CGContextSetBlendMode(ctx, 17);
    CGContextSetAlpha(ctx, .5);
    CGContextDrawImage(ctx, rect, translucent);
    if (pixels[0] != 0 || abs(pixels[1] - 16) > 1 ||
        abs(pixels[2] - 32) > 1 || abs(pixels[3] - 64) > 1) return -14;
    CGImageRelease(translucent);
    CGContextRelease(ctx);
    return 0;
}
