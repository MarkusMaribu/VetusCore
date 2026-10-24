/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
#include <math.h>

@interface UIView (LayoutTests)
- (void)setNeedsLayout;
- (void)layoutIfNeeded;
- (void)setAutoresizingMask:(NSUInteger)mask;
@end

@interface UIView (FlipTests)
- (CALayer *)layer;
+ (void)beginAnimations:(id)identifier context:(void *)context;
+ (void)setAnimationTransition:(NSInteger)transition forView:(UIView *)view cache:(BOOL)cache;
+ (void)setAnimationDuration:(double)duration;
+ (void)setAnimationDelegate:(id)delegate;
+ (void)setAnimationDidStopSelector:(SEL)selector;
+ (void)commitAnimations;
@end
@interface CALayer (FlipTests)
- (void)addSublayer:(CALayer *)layer;
- (void)removeFromSuperlayer;
@end
@interface NSRunLoop : NSObject
+ (id)mainRunLoop;
- (BOOL)runMode:(id)mode beforeDate:(id)date;
@end
@interface NSDate : NSObject
+ (id)dateWithTimeIntervalSinceNow:(double)seconds;
- (double)timeIntervalSinceNow;
@end
extern NSString *const NSDefaultRunLoopMode;

static int flip_layer_deallocs;
static int flip_completions;
@interface FlipLifetimeLayer : CALayer @end
@implementation FlipLifetimeLayer
- (void)dealloc {
  flip_layer_deallocs++;
  [super dealloc];
}
@end
@interface FlipLifetimeView : UIView @end
@implementation FlipLifetimeView
+ (Class)layerClass { return [FlipLifetimeLayer class]; }
@end
@interface FlipCompletionProbe : NSObject @end
@implementation FlipCompletionProbe
- (void)finished:(id)identifier finished:(BOOL)finished context:(void *)context {
  if (finished && context == (void *)0x1234) flip_completions++;
}
@end

// A transition must retain its removed outgoing hierarchy until completion.
// https://developer.apple.com/documentation/uikit/uiview/setanimationtransition(_:for:cache:)
int test_UIView_flip_keeps_removed_layers_alive(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  flip_layer_deallocs = 0;
  flip_completions = 0;
  UIView *container = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  UIView *old = [[FlipLifetimeView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  UIView *next = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  FlipCompletionProbe *probe = [[FlipCompletionProbe alloc] init];
  [container addSubview:old];
  [UIView beginAnimations:nil context:(void *)0x1234];
  [UIView setAnimationDuration:0.02];
  [UIView setAnimationDelegate:probe];
  [UIView setAnimationDidStopSelector:@selector(finished:finished:context:)];
  [UIView setAnimationTransition:1 forView:container cache:NO];
  [old removeFromSuperview];
  [container addSubview:next];
  [old release];
  int result = flip_layer_deallocs == 0 ? 0 : -1;
  [UIView commitAnimations];
  if (flip_layer_deallocs != 0 || flip_completions != 0) result = -2;
  NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:1.0];
  while (!flip_completions && [deadline timeIntervalSinceNow] > 0) {
    [[NSRunLoop mainRunLoop] runMode:NSDefaultRunLoopMode beforeDate:deadline];
  }
  if (flip_completions != 1 || flip_layer_deallocs != 1) result = -3;
  [probe release];
  [next release];
  [container release];
  [pool drain];
  return result;
}

int test_UIView_flip_preserves_nested_removed_layers(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  flip_layer_deallocs = 0;
  flip_completions = 0;
  UIView *container = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  CALayer *wrapper = [[CALayer alloc] init];
  CALayer *leaf = [[FlipLifetimeLayer alloc] init];
  [[container layer] addSublayer:wrapper];
  [wrapper addSublayer:leaf];
  FlipCompletionProbe *probe = [[FlipCompletionProbe alloc] init];
  [UIView beginAnimations:nil context:(void *)0x1234];
  [UIView setAnimationDuration:0.02];
  [UIView setAnimationDelegate:probe];
  [UIView setAnimationDidStopSelector:@selector(finished:finished:context:)];
  [UIView setAnimationTransition:2 forView:container cache:NO];
  // The removed leaf has no UIView delegate retaining it. Preserving only
  // the outer container leaves this nested outgoing content unprotected.
  [leaf removeFromSuperlayer];
  [leaf release];
  int result = flip_layer_deallocs == 0 ? 0 : -1;
  [UIView commitAnimations];
  if (flip_layer_deallocs != 0) result = -2;
  NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:1.0];
  while (!flip_completions && [deadline timeIntervalSinceNow] > 0) {
    [[NSRunLoop mainRunLoop] runMode:NSDefaultRunLoopMode beforeDate:deadline];
  }
  if (flip_completions != 1 || flip_layer_deallocs != 1) result = -3;
  [probe release];
  [wrapper release];
  [container release];
  [pool drain];
  return result;
}

static int layout_sequence;
@interface LayoutLifecycleProbe : UIView {
@public
  int calls;
  int order;
  BOOL reenter;
  BOOL invalidate_during_layout;
}
@end
@implementation LayoutLifecycleProbe
- (void)layoutSubviews {
  calls++;
  order = ++layout_sequence;
  if (reenter) {
    reenter = NO;
    [self layoutIfNeeded];
  }
  if (invalidate_during_layout) {
    invalidate_during_layout = NO;
    [self setNeedsLayout];
  }
}
@end

// https://developer.apple.com/documentation/uikit/uiview/setneedslayout()
// https://developer.apple.com/documentation/uikit/uiview/layoutifneeded()
int test_UIView_layout_deferred_and_coalesced(void) {
  LayoutLifecycleProbe *view = [[LayoutLifecycleProbe alloc]
      initWithFrame:CGRectMake(0, 0, 100, 100)];
  [view layoutIfNeeded];
  int result = view->calls == 1 ? 0 : -1;
  [view layoutIfNeeded];
  if (view->calls != 1) result = -2;
  [view setNeedsLayout];
  [view setNeedsLayout];
  if (view->calls != 1) result = -3;
  [view layoutIfNeeded];
  if (view->calls != 2) result = -4;
  [view layoutIfNeeded];
  if (view->calls != 2) result = -5;
  [view setFrame:CGRectMake(0, 0, 120, 140)];
  if (view->calls != 2) result = -6;
  [view layoutIfNeeded];
  if (view->calls != 3) result = -7;
  [view release];
  return result;
}

int test_UIView_layout_dirty_descendants(void) {
  UIView *root = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  LayoutLifecycleProbe *child = [[LayoutLifecycleProbe alloc]
      initWithFrame:CGRectMake(10, 10, 20, 20)];
  LayoutLifecycleProbe *grandchild = [[LayoutLifecycleProbe alloc]
      initWithFrame:CGRectMake(0, 0, 5, 5)];
  [child addSubview:grandchild];
  [root addSubview:child];
  [root layoutIfNeeded];
  int result = child->calls == 1 && grandchild->calls == 1 &&
      child->order < grandchild->order ? 0 : -1;
  [grandchild setNeedsLayout];
  [root layoutIfNeeded];
  if (child->calls != 1 || grandchild->calls != 2) result = -2;
  [root layoutIfNeeded];
  if (child->calls != 1 || grandchild->calls != 2) result = -3;
  [grandchild release];
  [child release];
  [root release];
  return result;
}

int test_UIView_layout_reentry_and_reinvalidation(void) {
  LayoutLifecycleProbe *view = [[LayoutLifecycleProbe alloc]
      initWithFrame:CGRectMake(0, 0, 100, 100)];
  view->reenter = YES;
  view->invalidate_during_layout = YES;
  [view layoutIfNeeded];
  int result = view->calls == 1 ? 0 : -1;
  [view layoutIfNeeded];
  if (view->calls != 2) result = -2;
  [view layoutIfNeeded];
  if (view->calls != 2) result = -3;
  [view release];
  return result;
}

int test_UIView_layout_preserves_autoresizing_geometry(void) {
  UIView *root = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 100, 100)];
  UIView *child = [[UIView alloc] initWithFrame:CGRectMake(10, 20, 30, 40)];
  [child setAutoresizingMask:(1 << 0) | (1 << 2)];
  [root addSubview:child];
  [root layoutIfNeeded];
  CGRect frame = [child frame];
  int result = frame.origin.x == 10 && frame.size.width == 30 ? 0 : -1;
  [root setBounds:CGRectMake(0, 0, 200, 100)];
  [root layoutIfNeeded];
  frame = [child frame];
  // The extra width is shared in proportion to the old 10/60 margins.
  if (fabs(frame.origin.x - (10 + 100.0 / 7.0)) > 0.001 ||
      frame.size.width != 30) result = -2;
  [child release];
  [root release];
  return result;
}

// UIView blocks animate layer transforms while model values change immediately.
// https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CoreAnimation_guide/AnimatableProperties/AnimatableProperties.html
// https://developer.apple.com/library/archive/documentation/WindowsViews/Conceptual/ViewPG_iPhoneOS/AnimatingViews/AnimatingViews.html
typedef struct {
  CGFloat m11,m12,m13,m14,m21,m22,m23,m24,m31,m32,m33,m34,m41,m42,m43,m44;
} LayoutTestTransform3D;
@interface CALayer (TransformAnimationTests)
- (void)setTransform:(LayoutTestTransform3D)transform;
- (LayoutTestTransform3D)transform;
- (CABasicAnimation *)animationForKey:(NSString *)key;
@end
@interface NSValue (TransformAnimationTests)
- (LayoutTestTransform3D)CATransform3DValue;
@end
@interface CABasicAnimation (TransformAnimationTests)
- (NSValue *)fromValue;
- (NSValue *)toValue;
@end
int test_UIView_block_interpolates_layer_transform(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UIView *view = [[UIView alloc] initWithFrame:CGRectMake(0,0,100,100)];
  CALayer *layer = [view layer];
  LayoutTestTransform3D small = {0.01,0,0,0, 0,0.01,0,0, 0,0,0,0, 0,0,0,1};
  LayoutTestTransform3D middle = small; middle.m11 = middle.m22 = 0.5;
  LayoutTestTransform3D full = small; full.m11 = full.m22 = 1;
  [layer setTransform:small];
  [UIView beginAnimations:nil context:NULL];
  [UIView setAnimationDuration:1.0];
  [layer setTransform:middle];
  [layer setTransform:full];
  [UIView commitAnimations];
  CABasicAnimation *animation = [layer animationForKey:@"transform"];
  int result = 0;
  if (!animation) result = -1;
  else {
    LayoutTestTransform3D from = [[animation fromValue] CATransform3DValue];
    LayoutTestTransform3D to = [[animation toValue] CATransform3DValue];
    if (fabs(from.m11-0.01)>0.00001 || fabs(to.m11-1)>0.00001) result = -2;
    if ([animation duration] != 1.0 || [layer transform].m22 != 1.0) result = -3;
  }
  [layer removeAnimationForKey:@"transform"];
  [view release];
  [pool release];
  return result;
}

@interface CAEAGLLayer : CALayer @end
@interface CALayer (RetinaScaleTests)
- (CGFloat)contentsScale;
@end
@interface UIView (RetinaScaleTests)
- (CGFloat)contentScaleFactor;
- (void)setContentScaleFactor:(CGFloat)scale;
@end
@interface RetinaGLScaleView : UIView @end
@implementation RetinaGLScaleView
+ (Class)layerClass { return [CAEAGLLayer class]; }
@end
@interface RetinaScreenScaleProbe : NSObject
- (CGFloat)scale;
@end
@implementation RetinaScreenScaleProbe
- (CGFloat)scale { return 2.0; }
@end
extern void *class_getInstanceMethod(Class cls, SEL selector);
extern void method_exchangeImplementations(void *first, void *second);

// EAGL views opt in to Retina; normal UIKit views inherit the screen scale.
// https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/OpenGLES_ProgrammingGuide/ImplementingaMultitasking-awareOpenGLESApplication/ImplementingaMultitasking-awareOpenGLESApplication.html
int test_UIView_EAGL_retina_is_opt_in(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  void *screenScale = class_getInstanceMethod([UIScreen class], @selector(scale));
  void *retinaScale = class_getInstanceMethod([RetinaScreenScaleProbe class], @selector(scale));
  method_exchangeImplementations(screenScale, retinaScale);
  UIView *plain = [[UIView alloc] initWithFrame:CGRectMake(0,0,100,100)];
  UIView *gl = [[RetinaGLScaleView alloc] initWithFrame:CGRectMake(0,0,100,100)];
  int result = 0;
  if ([plain contentScaleFactor] != 2 || [[plain layer] contentsScale] != 2) result = -1;
  if ([gl contentScaleFactor] != 1 || [[gl layer] contentsScale] != 1) result = -2;
  [gl setContentScaleFactor:2];
  if ([gl contentScaleFactor] != 2 || [[gl layer] contentsScale] != 2) result = -3;
  [plain release];
  [gl release];
  method_exchangeImplementations(screenScale, retinaScale);
  [pool release];
  return result;
}


@interface NSURLRequest : NSObject
+ (id)requestWithURL:(NSURL *)url;
@end
@interface UIWebView : UIView
- (void)setDelegate:(id)delegate;
- (void)loadRequest:(NSURLRequest *)request;
- (BOOL)isLoading;
@end
static int web_navigation_attempts;
static int web_load_starts;
@interface WebNavigationProbe : NSObject @end
@implementation WebNavigationProbe
- (BOOL)webView:(id)view shouldStartLoadWithRequest:(id)request navigationType:(NSInteger)kind {
  web_navigation_attempts++;
  return NO;
}
- (void)webViewDidStartLoad:(id)view { web_load_starts++; }
@end

// UIWebView inherits UIView geometry and asks its delegate before starting a load.
// https://developer.apple.com/documentation/uikit/uiwebviewdelegate
int test_UIWebView_frame_and_navigation_veto(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UIWebView *view = [[UIWebView alloc] initWithFrame:CGRectMake(11, 23, 201, 307)];
  CGRect frame = [view frame];
  CGRect bounds = [view bounds];
  int result = 0;
  if (frame.origin.x != 11 || frame.origin.y != 23 || frame.size.width != 201 || frame.size.height != 307 ||
      bounds.size.width != 201 || bounds.size.height != 307) result = -1;
  WebNavigationProbe *probe = [[WebNavigationProbe alloc] init];
  [view setDelegate:probe];
  web_navigation_attempts = 0;
  web_load_starts = 0;
  [view loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:@"https://example.invalid/"]]];
  if (web_navigation_attempts != 1 || web_load_starts != 0 || [view isLoading]) result = -2;
  [view setDelegate:nil];
  [probe release];
  [view release];
  [pool release];
  return result;
}


@interface NSError : NSObject
- (NSInteger)code;
- (NSString *)domain;
@end
@interface UIApplication (BlockedLinkTests)
- (BOOL)openURL:(NSURL *)url;
@end
static int blocked_link_failures;
static int blocked_link_finishes;
static NSInteger blocked_link_error;
@interface BlockedLinkProbe : NSObject @end
@implementation BlockedLinkProbe
// Host policy must reject and notify even if an SDK would veto the navigation.
- (BOOL)webView:(id)view shouldStartLoadWithRequest:(id)request navigationType:(NSInteger)kind { return NO; }
- (void)webView:(id)view didFailLoadWithError:(NSError *)error {
  blocked_link_failures++;
  blocked_link_error = [error code];
}
- (void)webViewDidFinishLoad:(id)view { blocked_link_finishes++; }
@end

// Invoked separately with --disable-links: denied requests must fail rather than
// reporting a successful load or leaving an SDK's login dialog waiting forever.
int TestApp_blocked_links_test_main(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  UIApplication *application = [UIApplication sharedApplication];
  if (application == nil) application = [[UIApplication alloc] init];
  NSURL *url = [NSURL URLWithString:@"vetus-link-policy-test://blocked"];
  if ([application canOpenURL:[NSURL URLWithString:@"https://example.invalid/"]] ||
      [application canOpenURL:url] || [application openURL:url]) return 1;
  UIWebView *view = [[UIWebView alloc] initWithFrame:CGRectMake(0, 0, 320, 480)];
  BlockedLinkProbe *probe = [[BlockedLinkProbe alloc] init];
  blocked_link_failures = blocked_link_finishes = 0;
  blocked_link_error = 0;
  [view setDelegate:probe];
  [view loadRequest:[NSURLRequest requestWithURL:url]];
  if (blocked_link_failures != 0) return 2; // The failure must be asynchronous.
  for (int i = 0; i < 20 && blocked_link_failures == 0; i++)
    [[NSRunLoop mainRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
  int result = blocked_link_failures == 1 && blocked_link_finishes == 0 &&
      blocked_link_error == -1 && ![view isLoading] ? 0 : 3;
  [view setDelegate:nil];
  [view release];
  [probe release];
  [pool release];
  return result;
}
