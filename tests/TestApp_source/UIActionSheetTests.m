/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
@interface UIActionSheet : UIView
- (id)initWithTitle:(id)title delegate:(id)delegate cancelButtonTitle:(id)cancel
 destructiveButtonTitle:(id)destructive otherButtonTitles:(id)first, ...;
- (void)showInView:(id)view;
- (BOOL)isVisible;
- (NSInteger)cancelButtonIndex;
- (NSInteger)destructiveButtonIndex;
- (NSInteger)firstOtherButtonIndex;
- (NSUInteger)numberOfButtons;
- (id)buttonTitleAtIndex:(NSInteger)index;
- (void)dismissWithClickedButtonIndex:(NSInteger)index animated:(BOOL)animated;
@end
@interface NSObject (SheetTest)
- (NSUInteger)retainCount;
@end
static int sheet_calls, sheet_will, sheet_did, sheet_index;
@interface SheetProbe : NSObject @end
@implementation SheetProbe
- (void)actionSheet:(id)sheet clickedButtonAtIndex:(NSInteger)index {
  sheet_calls++; sheet_index = index;
}
- (void)actionSheet:(id)sheet willDismissWithButtonIndex:(NSInteger)index { sheet_will++; }
- (void)actionSheet:(id)sheet didDismissWithButtonIndex:(NSInteger)index { sheet_did++; }
@end

// All nil-terminated titles must be preserved. Showing is asynchronous and
// actual selection dismisses once, with the original button index.
// https://developer.apple.com/documentation/uikit/uiactionsheetdelegate
int test_UIActionSheet_selection_and_variadic_buttons(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  sheet_calls = sheet_will = sheet_did = 0; sheet_index = -1;
  SheetProbe *probe = [[SheetProbe alloc] init];
  NSUInteger refs = [probe retainCount];
  UIActionSheet *sheet = [[UIActionSheet alloc] initWithTitle:@"Export" delegate:probe
    cancelButtonTitle:@"Cancel" destructiveButtonTitle:@"Delete"
    otherButtonTitles:@"Save original", @"Save result", nil];
  int result = 0;
  if ([probe retainCount] != refs) result = -1;
  if ([sheet numberOfButtons] != 4 || [sheet destructiveButtonIndex] != 0 ||
      [sheet firstOtherButtonIndex] != 1 || [sheet cancelButtonIndex] != 3 ||
      ![[sheet buttonTitleAtIndex:2] isEqualToString:@"Save result"]) result = -2;
  UIView *view = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 320, 480)];
  [sheet showInView:view];
  if (![sheet isVisible] || sheet_calls != 0) result = -3;
  [sheet dismissWithClickedButtonIndex:2 animated:NO];
  if ([sheet isVisible] || sheet_calls != 1 || sheet_index != 2 || sheet_will != 1 || sheet_did != 1) result = -4;
  [sheet dismissWithClickedButtonIndex:2 animated:NO];
  if (sheet_calls != 1) result = -5;
  [sheet showInView:view];
  [sheet dismissWithClickedButtonIndex:3 animated:NO];
  if (sheet_calls != 2 || sheet_index != 3) result = -6;
  [sheet release]; [view release]; [probe release]; [pool release];
  return result;
}
