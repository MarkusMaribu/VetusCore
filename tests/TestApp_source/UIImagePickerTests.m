/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"

@interface UIViewController : NSObject
- (UIView *)view;
- (void)setView:(UIView *)view;
- (void)presentModalViewController:(UIViewController *)controller animated:(BOOL)animated;
- (void)dismissModalViewControllerAnimated:(BOOL)animated;
- (UIViewController *)presentedViewController;
- (UIViewController *)presentingViewController;
@end
@interface UINavigationController : UIViewController
- (id)navigationBar;
@end
@interface UIImagePickerController : UINavigationController
+ (BOOL)isSourceTypeAvailable:(NSInteger)source;
+ (NSArray *)availableMediaTypesForSourceType:(NSInteger)source;
- (BOOL)shouldAutorotateToInterfaceOrientation:(NSInteger)orientation;
- (NSInteger)sourceType;
- (void)setSourceType:(NSInteger)source;
- (NSArray *)mediaTypes;
- (void)setDelegate:(id)delegate;
- (id)delegate;
@end
@interface NSObject (PickerRetainTest)
- (NSUInteger)retainCount;
@end

// The picker must keep UIViewController storage, default to public.image,
// and leave delegate ownership with the caller. Its presenting controller
// must be able to retain it until dismissal without a host-object mismatch.
// https://developer.apple.com/documentation/uikit/uiimagepickercontroller
// https://developer.apple.com/documentation/uikit/uiimagepickercontroller/delegate
int test_UIImagePicker_controller_state_and_sources(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UIImagePickerController *picker = [[UIImagePickerController alloc] init];
  int result = 0;
  if (![picker shouldAutorotateToInterfaceOrientation:1] || [picker shouldAutorotateToInterfaceOrientation:3] || [picker shouldAutorotateToInterfaceOrientation:4]) result = -9;
  if (![picker isKindOfClass:[UINavigationController class]] || [picker navigationBar] == nil) result = -8;
  if ([picker sourceType] != 0 || [[picker mediaTypes] count] != 1 ||
      ![[[picker mediaTypes] objectAtIndex:0] isEqualToString:@"public.image"]) result = -1;
  NSObject *delegate = [[NSObject alloc] init];
  NSUInteger refs = [delegate retainCount];
  [picker setDelegate:delegate];
  if ([picker delegate] != delegate || [delegate retainCount] != refs) result = -2;
  [picker setDelegate:nil];
  [delegate release];
  UIView *view = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 320, 480)];
  [picker setView:view];
  if ([picker view] != view) result = -3;
  [view release];
  // Avoid opening a host library while testing controller ownership.
  [picker setSourceType:1];
  UIViewController *parent = [[UIViewController alloc] init];
  UIView *parentView = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 320, 480)];
  [parent setView:parentView];
  [parentView release];
  [parent presentModalViewController:picker animated:NO];
  if ([parent presentedViewController] != picker || [picker presentingViewController] != parent) result = -4;
  [picker dismissModalViewControllerAnimated:NO];
  if ([parent presentedViewController] != nil || [picker presentingViewController] != nil) result = -5;
  for (NSInteger source = -1; source <= 3; source++) {
    BOOL available = [UIImagePickerController isSourceTypeAvailable:source];
    NSArray *types = [UIImagePickerController availableMediaTypesForSourceType:source];
    if (available && ([types count] != 1 || ![[types objectAtIndex:0] isEqualToString:@"public.image"])) result = -6;
    if (!available && types != nil) result = -7;
  }
  [parent release];
  [picker release];
  [pool release];
  return result;
}
