/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
@interface CALayer (MaskTest)
- (BOOL)masksToBounds;
- (void)setMasksToBounds:(BOOL)clip;
- (void)setMask:(CALayer *)mask;
- (CALayer *)mask;
- (CALayer *)superlayer;
- (NSArray *)sublayers;
- (void)addSublayer:(CALayer *)layer;
@end
@interface UIView (MaskTest)
- (BOOL)clipsToBounds;
- (void)setClipsToBounds:(BOOL)clip;
@end
// UIView clipping is the backing layer's clipping setting. The mask itself
// remains independently retained when rectangular clipping is changed.
// https://developer.apple.com/documentation/quartzcore/calayer/maskstobounds
int test_CALayer_mask_and_view_clipping(void) {
  UIView *view = [[UIView alloc] initWithFrame:CGRectMake(0,0,100,100)];
  CALayer *layer = [view layer];
  int result = 0;
  if ([layer masksToBounds] || [view clipsToBounds]) result = -1;
  [view setClipsToBounds:YES];
  if (![layer masksToBounds] || ![view clipsToBounds]) result = -2;
  [layer setMasksToBounds:NO];
  if ([view clipsToBounds]) result = -3;
  CALayer *mask = [[CALayer alloc] init];
  CALayer *parent = [[CALayer alloc] init];
  [parent addSublayer:mask];
  [layer setMask:mask]; [mask release];
  if ([mask superlayer] != nil || [[parent sublayers] count] != 0) result = -6;
  [parent release];
  [layer setMasksToBounds:YES];
  if ([layer mask] != mask || ![view clipsToBounds]) result = -4;
  [layer setMask:nil];
  if (![layer masksToBounds]) result = -5;
  [view release];
  return result;
}
