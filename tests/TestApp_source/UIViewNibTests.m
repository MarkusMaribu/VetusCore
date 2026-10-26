/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/. */
#import "system_headers.h"
@interface UIView (NibTest)
- (id)initWithCoder:(id)coder;
- (CGFloat)alpha;
- (BOOL)clipsToBounds;
@end
@interface UIViewDefaultCoder : NSCoder @end
@implementation UIViewDefaultCoder
- (BOOL)containsValueForKey:(NSString *)key { return [key isEqualToString:@"UIBounds"]; }
- (CGRect)decodeCGRectForKey:(NSString *)key { return CGRectMake(0, 0, 8, 8); }
@end
@interface UIViewOpacityCoder : UIViewDefaultCoder @end
@implementation UIViewOpacityCoder
- (BOOL)containsValueForKey:(NSString *)key {
  return [super containsValueForKey:key] || [key isEqualToString:@"UIAlpha"] || [key isEqualToString:@"UIClipsToBounds"];
}
- (float)decodeFloatForKey:(NSString *)key { return 0; }
- (BOOL)decodeBoolForKey:(NSString *)key { return YES; }
@end
// Explicit zero opacity differs from an omitted default (one).
// https://developer.apple.com/documentation/uikit/uiview/alpha
int test_UIView_nib_opacity_and_clipping(void) {
  UIViewOpacityCoder *coder = [[UIViewOpacityCoder alloc] init];
  UIView *view = [[UIView alloc] initWithCoder:coder];
  int result = 0;
  if ([view alpha] != 0) result = -1;
  else if (![view clipsToBounds]) result = -2;
  [view release]; [coder release];
  UIViewDefaultCoder *defaults = [[UIViewDefaultCoder alloc] init];
  view = [[UIView alloc] initWithCoder:defaults];
  if ([view alpha] != 1 || [view clipsToBounds]) result = -3;
  [view release]; [defaults release];
  return result;
}

@interface UINavigationItem : NSObject
- (id)initWithCoder:(id)coder;
- (NSString *)title;
- (id)leftBarButtonItem;
- (id)rightBarButtonItem;
@end
@interface UINavigationBar : UIView
- (id)initWithCoder:(id)coder;
- (id)topItem;
@end
@interface UIBarButtonItem : UIControl
- (id)initWithTitle:(NSString *)title style:(NSInteger)style target:(id)target action:(SEL)action;
@end
@interface UIView (NavigationNibTest)
- (id)superview;
@end
static id navigation_left, navigation_right, navigation_items;
@interface NavigationMetadataCoder : UIViewDefaultCoder @end
@implementation NavigationMetadataCoder
- (CGRect)decodeCGRectForKey:(NSString *)key { return CGRectMake(0, 0, 480, 44); }
- (id)decodeObjectForKey:(NSString *)key {
  if ([key isEqualToString:@"UITitle"]) return @"Archived title";
  if ([key isEqualToString:@"UILeftBarButtonItem"]) return navigation_left;
  if ([key isEqualToString:@"UIRightBarButtonItem"]) return navigation_right;
  if ([key isEqualToString:@"UIItems"]) return navigation_items;
  return nil;
}
@end
// NIB navigation metadata must produce visible controls just like setters.
// https://developer.apple.com/documentation/uikit/uinavigationbar
int test_UINavigation_nib_items_and_controls(void) {
  navigation_left = [[UIBarButtonItem alloc] initWithTitle:@"Camera" style:0 target:nil action:(SEL)0];
  navigation_right = [[UIBarButtonItem alloc] initWithTitle:@"Clear" style:0 target:nil action:(SEL)0];
  NavigationMetadataCoder *coder = [[NavigationMetadataCoder alloc] init];
  UINavigationItem *item = [[UINavigationItem alloc] initWithCoder:coder];
  navigation_items = [NSArray arrayWithObjects:item, nil];
  UINavigationBar *bar = [[UINavigationBar alloc] initWithCoder:coder];
  [bar layoutSubviews];
  int result = 0;
  if (![[item title] isEqualToString:@"Archived title"]) result = -1;
  else if ([bar topItem] != item) result = -2;
  else if ([item leftBarButtonItem] != navigation_left || [item rightBarButtonItem] != navigation_right) result = -3;
  else if ([navigation_left superview] != bar || [navigation_right superview] != bar) result = -4;
  [bar release]; [item release]; [coder release];
  [navigation_left release]; [navigation_right release];
  navigation_left = navigation_right = navigation_items = nil;
  return result;
}
