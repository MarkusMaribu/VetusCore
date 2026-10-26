/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"

@interface UIViewController : NSObject
- (UIView *)view;
- (void)setView:(UIView *)view;
@end
@interface UIWindow (ControllerViewTests)
- (void)setRootViewController:(UIViewController *)controller;
@end
@interface UIView (ControllerViewTests)
- (UIView *)superview;
@end

// A window owns its root controller's view and sizes it to fill the window.
// Replacing that view during launch must not leave an orphan while the
// discarded placeholder remains on screen.
// https://developer.apple.com/documentation/uikit/uiwindow/rootviewcontroller
// https://developer.apple.com/documentation/uikit/uiviewcontroller/view
int test_UIViewController_replacing_window_root_view(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  CGRect full = CGRectMake(0, 0, 320, 480);
  UIWindow *window = [[UIWindow alloc] initWithFrame:full];
  UIViewController *controller = [[UIViewController alloc] init];
  UIView *placeholder = [[UIView alloc] initWithFrame:full];
  [controller setView:placeholder];
  [placeholder release];
  [window setRootViewController:controller];
  UIView *old = [[controller view] retain];
  UIView *overlay = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 10, 10)];
  [window addSubview:overlay];
  UIView *next = [[UIView alloc] initWithFrame:CGRectMake(3, 4, 70, 80)];
  [controller setView:next];
  int result = 0;
  if ([controller view] != next) result = -1;
  else if ([next superview] != window || [old superview] != nil) result = -2;
  else {
    CGRect frame = [next frame];
    if (frame.origin.x != 0 || frame.origin.y != 0 ||
        frame.size.width != 320 || frame.size.height != 480) result = -3;
    NSArray *views = [window subviews];
    if ([views count] != 2 || [views objectAtIndex:0] != next ||
        [views objectAtIndex:1] != overlay) result = -4;
    NSUInteger references = [next retainCount];
    [controller setView:next];
    if ([next retainCount] != references || [next superview] != window) result = -5;
  }
  [window setRootViewController:nil];
  [old release];
  [next release];
  [overlay release];
  [controller release];
  [window release];
  [pool drain];
  return result;
}

int test_UIViewController_unmounted_view_stays_unmounted(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UIViewController *controller = [[UIViewController alloc] init];
  UIView *first = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 20, 30)];
  UIView *next = [[UIView alloc] initWithFrame:CGRectMake(2, 3, 40, 50)];
  [controller setView:first];
  [controller setView:next];
  CGRect frame = [next frame];
  int result = [controller view] == next && [next superview] == nil &&
      frame.origin.x == 2 && frame.origin.y == 3 &&
      frame.size.width == 40 && frame.size.height == 50 ? 0 : -1;
  [first release];
  [next release];
  [controller release];
  [pool drain];
  return result;
}
