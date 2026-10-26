/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"

void UIImageWriteToSavedPhotosAlbum(id image, id target, SEL selector, void *context);
@interface NSRunLoop : NSObject
+ (id)mainRunLoop;
- (void)runUntilDate:(id)date;
@end
@interface NSDate : NSObject
+ (id)dateWithTimeIntervalSinceNow:(double)seconds;
@end

static int save_calls, save_errors, save_bad_context, save_deallocs;
@interface ImageSaveProbe : NSObject @end
@implementation ImageSaveProbe
- (void)image:(id)image didFinishSavingWithError:(id)error contextInfo:(void *)context {
  save_calls++;
  if (error != nil) save_errors++;
  if (image != nil || context != (void *)0x1234) save_bad_context++;
}
- (void)dealloc { save_deallocs++; [super dealloc]; }
@end

// Completion must be asynchronous, retain its target, preserve contextInfo,
// and report errors instead of falsely claiming that invalid images saved.
// https://developer.apple.com/documentation/uikit/uiimagewritetosavedphotosalbum(_:_:_:_:)
int test_UIImage_save_completion_is_deferred_and_reports_failure(void) {
  save_calls = save_errors = save_bad_context = save_deallocs = 0;
  ImageSaveProbe *probe = [[ImageSaveProbe alloc] init];
  SEL callback = @selector(image:didFinishSavingWithError:contextInfo:);
  UIImageWriteToSavedPhotosAlbum(nil, probe, callback, (void *)0x1234);
  UIImageWriteToSavedPhotosAlbum(nil, probe, callback, (void *)0x1234);
  [probe release];
  if (save_calls != 0 || save_deallocs != 0) return -1;
  [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
  if (save_calls != 2 || save_errors != 2 || save_bad_context != 0) return -2;
  if (save_deallocs != 1) return -3;
  return 0;
}
