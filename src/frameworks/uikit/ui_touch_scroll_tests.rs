/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use super::*;
use crate::frameworks::core_graphics::CGSize;
use crate::objc::SEL;

// Exercise real hit testing and event dispatch: calling UIScrollView's own
// touchesMoved directly would miss the regression (a child UIButton owns the
// initial hit). Apple specifies temporary interception, cancellation policy,
// and distinct end-drag/end-deceleration callbacks:
// https://developer.apple.com/documentation/uikit/uiscrollview
// https://developer.apple.com/documentation/uikit/uiscrollview/touchesshouldcancel(in:)
// Upstream comparison:
// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view/ui_scroll_view.rs

fn mark(env: &mut Environment, receiver: id, bit: NSInteger) {
    let old: NSInteger = msg![env; receiver tag];
    () = msg![env; receiver setTag:(old | bit)];
}
fn did_scroll(env: &mut Environment, receiver: id, _: SEL, _: id) { mark(env, receiver, 2); }
fn will_drag(env: &mut Environment, receiver: id, _: SEL, _: id) { mark(env, receiver, 1); }
fn end_drag(env: &mut Environment, receiver: id, _: SEL, _: id, decelerate: bool) {
    mark(env, receiver, if decelerate { 4 } else { 8 });
}
fn will_decelerate(env: &mut Environment, receiver: id, _: SEL, _: id) { mark(env, receiver, 16); }
fn end_decelerate(env: &mut Environment, receiver: id, _: SEL, _: id) { mark(env, receiver, 32); }
fn allow_control_cancel(env: &mut Environment, scroll: id, _: SEL, _: id) -> bool {
    mark(env, scroll, 1);
    true
}
fn touch_cancelled(env: &mut Environment, receiver: id, _: SEL, _: id) {
    mark(env, receiver, 1);
}

struct Views { pool: id, window: id, scroll: id, content: id, receiver: id, delegate: id }
fn views(env: &mut Environment, button: bool) -> Views {
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let _application: id = msg_class![env; UIApplication new];
    let frame = CGRect { origin: CGPoint::default(), size: CGSize { width: 100.0, height: 100.0 } };
    let window: id = msg_class![env; UIWindow alloc];
    let window: id = msg![env; window initWithFrame:frame];
    let scroll: id = msg_class![env; UIScrollView alloc];
    let scroll: id = msg![env; scroll initWithFrame:frame];
    let size = CGSize { width: 300.0, height: 100.0 };
    () = msg![env; scroll setContentSize:size];
    let content: id = if button { msg_class![env; UIButton alloc] } else { msg_class![env; UIImageView alloc] };
    let content: id = msg![env; content initWithFrame:(CGRect { size, ..frame })];
    () = msg![env; content setUserInteractionEnabled:true];
    () = msg![env; scroll addSubview:content];
    () = msg![env; window addSubview:scroll];
    () = msg![env; window setHidden:false];
    let receiver: id = msg_class![env; UIView new];
    if button {
        let action = env.objc.lookup_selector("setTag:").unwrap();
        () = msg![env; content addTarget:receiver action:action forControlEvents:64u32];
    }
    let class = env.objc.get_known_class("UIView", &mut env.mem);
    env.objc.add_host_method_if_absent(class, "scrollViewWillBeginDragging:",
        &(will_drag as fn(&mut Environment, id, SEL, id)), &mut env.mem);
    env.objc.add_host_method_if_absent(class, "scrollViewDidScroll:",
        &(did_scroll as fn(&mut Environment, id, SEL, id)), &mut env.mem);
    env.objc.add_host_method_if_absent(class, "scrollViewDidEndDragging:willDecelerate:",
        &(end_drag as fn(&mut Environment, id, SEL, id, bool)), &mut env.mem);
    env.objc.add_host_method_if_absent(class, "scrollViewWillBeginDecelerating:",
        &(will_decelerate as fn(&mut Environment, id, SEL, id)), &mut env.mem);
    env.objc.add_host_method_if_absent(class, "scrollViewDidEndDecelerating:",
        &(end_decelerate as fn(&mut Environment, id, SEL, id)), &mut env.mem);
    let delegate: id = msg_class![env; UIView new];
    () = msg![env; scroll setDelegate:delegate];
    Views { pool, window, scroll, content, receiver, delegate }
}
fn cleanup(env: &mut Environment, v: Views) {
    ui_scroll_view::settle_step(env, v.scroll, 1.0);
    () = msg![env; (v.scroll) setDelegate:nil];
    for object in [v.window, v.scroll, v.content, v.receiver, v.delegate, v.pool] { release(env, object); }
}
fn event(env: &mut Environment, phase: u8, x: f32) {
    let points = HashMap::from([(FingerId::Mouse, (x, 30.0))]);
    handle_event(env, match phase {
        0 => Event::TouchesDown(points), 1 => Event::TouchesMove(points), _ => Event::TouchesUp(points),
    });
}
fn offset(env: &mut Environment, scroll: id) -> f32 {
    let point: CGPoint = msg![env; scroll contentOffset]; point.x
}

#[test]
fn early_drag_over_button_scrolls_without_touch_up_inside_and_pages_after_release() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let v = views(env, true);
    () = msg![env; (v.scroll) setPagingEnabled:true];
    event(env, 0, 80.0);
    let touch = env.framework_state.uikit.ui_touch.current_touches[&FingerId::Mouse];
    let tracking: bool = msg![env; (v.content) tracking];
    assert!(!tracking, "delaysContentTouches must hold the button's Down");
    event(env, 1, 20.0);
    assert_eq!(offset(env, v.scroll), 60.0, "the ancestor never received the drag");
    let hit: id = msg![env; touch view];
    assert_eq!(hit, v.content, "scroll interception must preserve UITouch.view");
    let dragging: bool = msg![env; (v.scroll) isDragging];
    assert!(dragging);
    event(env, 2, 20.0);
    let action: NSInteger = msg![env; (v.receiver) tag];
    assert_eq!(action, 0, "a drag incorrectly sent TouchUpInside to the button");
    let decelerating: bool = msg![env; (v.scroll) isDecelerating];
    assert!(decelerating);
    let callbacks: NSInteger = msg![env; (v.delegate) tag];
    assert_eq!(callbacks, 1 | 2 | 4 | 16);
    ui_scroll_view::settle_step(env, v.scroll, 1.0);
    assert_eq!(offset(env, v.scroll), 100.0);
    let callbacks: NSInteger = msg![env; (v.delegate) tag];
    assert_eq!(callbacks, 1 | 2 | 4 | 16 | 32);
    cleanup(env, v);
}

#[test]
fn quick_button_tap_delivers_down_then_up_and_releases_scroll_tracking() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let v = views(env, true);
    event(env, 0, 50.0);
    event(env, 2, 50.0);
    let action: NSInteger = msg![env; (v.receiver) tag];
    assert_eq!(action, v.content.to_bits() as NSInteger);
    let tracking: bool = msg![env; (v.scroll) isTracking];
    assert!(!tracking);
    assert_eq!(env.objc.get_refcount(v.scroll).get(), 2);
    cleanup(env, v);
}

#[test]
fn a_button_already_tracking_keeps_touches_and_disabled_scrolling_does_not_delay_it() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let v = views(env, true);
    () = msg![env; (v.scroll) setDelaysContentTouches:false];
    event(env, 0, 80.0);
    let tracking: bool = msg![env; (v.content) tracking];
    assert!(tracking);
    event(env, 1, 20.0);
    assert_eq!(offset(env, v.scroll), 0.0, "default cancellation must protect UIControl");
    event(env, 2, 20.0);
    let action: NSInteger = msg![env; (v.receiver) tag];
    assert_ne!(action, 0);
    () = msg![env; (v.scroll) setScrollEnabled:false];
    () = msg![env; (v.scroll) setDelaysContentTouches:true];
    event(env, 0, 80.0);
    let tracking: bool = msg![env; (v.content) tracking];
    assert!(tracking, "disabled scrolling must not intercept content touches");
    event(env, 2, 80.0);
    cleanup(env, v);
}

#[test]
fn can_cancel_policy_applies_after_content_tracking_and_lost_up_cleans_pending_timer() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let v = views(env, false);
    () = msg![env; (v.scroll) setDelaysContentTouches:false];
    () = msg![env; (v.scroll) setCanCancelContentTouches:false];
    event(env, 0, 80.0);
    event(env, 1, 20.0);
    assert_eq!(offset(env, v.scroll), 0.0);
    event(env, 2, 20.0);
    () = msg![env; (v.scroll) setCanCancelContentTouches:true];
    event(env, 0, 80.0);
    event(env, 1, 20.0);
    assert_eq!(offset(env, v.scroll), 60.0, "ordinary content views should be cancellable");
    event(env, 2, 20.0);
    ui_scroll_view::settle_step(env, v.scroll, 1.0);
    () = msg![env; (v.scroll) setDelaysContentTouches:true];
    event(env, 0, 80.0);
    let touch = env.framework_state.uikit.ui_touch.current_touches[&FingerId::Mouse];
    stale_cancel_and_release(env, FingerId::Mouse, touch, "test lost Up");
    let tracking: bool = msg![env; (v.scroll) isTracking];
    assert!(!tracking);
    assert_eq!(env.objc.get_refcount(v.scroll).get(), 2);
    cleanup(env, v);
}

#[test]
fn delay_timer_forwards_a_stationary_button_down_before_its_up() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let v = views(env, true);
    event(env, 0, 50.0);
    let touch = env.framework_state.uikit.ui_touch.current_touches[&FingerId::Mouse];
    let timer = env.objc.borrow::<UITouchHostObject>(touch).scroll.unwrap().timer;
    () = msg![env; timer fire];
    let tracking: bool = msg![env; (v.content) tracking];
    assert!(tracking, "the delay timer never forwarded Down to the child");
    let action: NSInteger = msg![env; (v.receiver) tag];
    assert_eq!(action, 0);
    event(env, 2, 50.0);
    let action: NSInteger = msg![env; (v.receiver) tag];
    assert_eq!(action, v.content.to_bits() as NSInteger);
    cleanup(env, v);
}

#[test]
fn custom_cancellation_policy_cancels_a_tracking_control_unless_cancellation_is_disabled() {
    for can_cancel in [true, false] {
        let mut env = crate::environment::touch_test_environment();
        let env = &mut env;
        let v = views(env, true);
        // Install a custom implementation using the supported ObjC runtime.
        // This exercises dynamic dispatch rather than duplicating the default
        // cancellation predicate in the test or implementation.
        let class = env.objc.get_known_class("UIScrollView", &mut env.mem);
        env.objc.add_host_method_if_absent(class, "_testCancellationPolicy:",
            &(allow_control_cancel as fn(&mut Environment, id, SEL, id) -> bool), &mut env.mem);
        let original = env.objc.lookup_selector("touchesShouldCancelInContentView:").unwrap();
        let custom = env.objc.lookup_selector("_testCancellationPolicy:").unwrap();
        let original = crate::objc::class_getInstanceMethod(env, class, original);
        let custom = crate::objc::class_getInstanceMethod(env, class, custom);
        crate::objc::method_exchangeImplementations(env, original, custom);
        let receiver_class = env.objc.get_known_class("UIView", &mut env.mem);
        env.objc.add_host_method_if_absent(receiver_class, "_testTouchCancelled:",
            &(touch_cancelled as fn(&mut Environment, id, SEL, id)), &mut env.mem);
        let cancel = env.objc.lookup_selector("_testTouchCancelled:").unwrap();
        let receiver = v.receiver;
        () = msg![env; (v.content) addTarget:receiver action:cancel forControlEvents:256u32];
        () = msg![env; (v.scroll) setDelaysContentTouches:false];
        () = msg![env; (v.scroll) setCanCancelContentTouches:can_cancel];
        event(env, 0, 80.0);
        event(env, 1, 20.0);
        let policy_calls: NSInteger = msg![env; (v.scroll) tag];
        assert_eq!(policy_calls, if can_cancel { 1 } else { 0 });
        assert_eq!(offset(env, v.scroll), if can_cancel { 60.0 } else { 0.0 });
        event(env, 2, 20.0);
        let action: NSInteger = msg![env; (v.receiver) tag];
        assert_eq!(action, if can_cancel { 1 } else { v.content.to_bits() as NSInteger },
            "a cancelled control must receive TouchCancel and never TouchUpInside");
        cleanup(env, v);
    }
}
