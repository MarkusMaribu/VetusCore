/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"

@interface UITextField : UIControl
- (void)setText:(NSString *)text;
- (NSString *)text;
- (BOOL)isEditing;
- (BOOL)isFirstResponder;
- (BOOL)becomeFirstResponder;
- (BOOL)resignFirstResponder;
@end

static unsigned textChangeCount;
@interface TextEditingObserver : NSObject
- (void)changed:(NSNotification *)notification;
@end
@implementation TextEditingObserver
- (void)changed:(NSNotification *)notification {
  textChangeCount++;
  // A normalizer must not recursively receive its own programmatic update.
  if (textChangeCount < 4) [[notification object] setText:@"30"];
}
@end

// Editing notifications report user edits, not assignments to the text property.
// https://developer.apple.com/documentation/uikit/uitextfielddelegate
// https://developer.apple.com/documentation/uikit/uitextfield/textdidchangenotification
int test_UITextField_programmatic_text_does_not_notify(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UITextField *field = [[UITextField alloc] initWithFrame:CGRectMake(0, 0, 100, 40)];
  TextEditingObserver *observer = [[TextEditingObserver alloc] init];
  NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
  [center addObserver:observer selector:@selector(changed:)
                 name:@"UITextFieldTextDidChangeNotification" object:field];
  textChangeCount = 0;
  [field setText:@"20"];
  int result = textChangeCount == 0 && [[field text] isEqualToString:@"20"] ? 0 : -1;
  // Simulate a user-edit notification, then normalize from inside its observer.
  textChangeCount = 0;
  [center postNotificationName:@"UITextFieldTextDidChangeNotification" object:field];
  if (!result && (textChangeCount != 1 || ![[field text] isEqualToString:@"30"])) result = -2;
  [center removeObserver:observer];
  [observer release];
  [field release];
  [pool drain];
  return result;
}

static unsigned keyboardHideCount;
static BOOL wasEditingDuringHide;
@interface KeyboardHideObserver : NSObject
- (void)willHide:(NSNotification *)notification;
@end
@implementation KeyboardHideObserver
- (void)willHide:(NSNotification *)notification {
  UITextField *field = [notification object];
  keyboardHideCount++;
  wasEditingDuringHide |= [field isEditing] || [field isFirstResponder];
  if (keyboardHideCount < 4) [field resignFirstResponder];
}
@end

// Resignation is complete before the keyboard-hide callbacks can reenter it.
// https://developer.apple.com/documentation/uikit/uitextfielddelegate
int test_UITextField_keyboard_hide_can_end_editing_again(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UITextField *field = [[UITextField alloc] initWithFrame:CGRectMake(0, 0, 100, 40)];
  KeyboardHideObserver *observer = [[KeyboardHideObserver alloc] init];
  NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
  [center addObserver:observer selector:@selector(willHide:)
                 name:@"UIKeyboardWillHideNotification" object:field];
  keyboardHideCount = 0;
  wasEditingDuringHide = NO;
  int result = 0;
  if (![field becomeFirstResponder] || ![field resignFirstResponder]) result = -1;
  else if (keyboardHideCount != 1 || wasEditingDuringHide) result = -2;
  else if (![field resignFirstResponder] || keyboardHideCount != 1) result = -3;
  [center removeObserver:observer];
  [observer release];
  [field release];
  [pool drain];
  return result;
}
