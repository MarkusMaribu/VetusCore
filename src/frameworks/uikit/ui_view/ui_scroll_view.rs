/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#![allow(dead_code)]
//! `UIScrollView`.

pub mod ui_text_view;

use crate::abi::{GuestArg, GuestRet};
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::NSInteger;
use crate::mem::SafeRead;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, nil, objc_classes, release, retain,
    ClassExports, NSZonePtr,
};
use crate::Environment;
use std::time::Instant;

type UIScrollViewIndicatorStyle = NSInteger;
type UIScrollViewKeyboardDismissMode = NSInteger;

// UIScrollViewKeyboardDismissMode values
const UIScrollViewKeyboardDismissModeNone: NSInteger = 0;
const UIScrollViewKeyboardDismissModeOnDrag: NSInteger = 1;
const UIScrollViewKeyboardDismissModeInteractive: NSInteger = 2;

pub struct UIScrollViewHostObject {
    superclass: super::UIViewHostObject,
    /// UIScrollViewDelegate — weak reference
    delegate: id,
    scroll_enabled: bool,
    content_offset: CGPoint,
    content_size: CGSize,
    content_inset: UIEdgeInsets,
    shows_horizontal_scroll_indicator: bool,
    shows_vertical_scroll_indicator: bool,
    always_bounce_vertical: bool,
    always_bounce_horizontal: bool,
    bounces: bool,
    paging_enabled: bool,
    directional_lock_enabled: bool,
    minimum_zoom_scale: CGFloat,
    maximum_zoom_scale: CGFloat,
    zoom_scale: CGFloat,
    keyboard_dismiss_mode: UIScrollViewKeyboardDismissMode,
    decelerates: bool,
    scrolls_to_top: bool,             // (с прошлого фикса)
    can_cancel_content_touches: bool, // <-- ДОБАВЛЕНО
    delays_content_touches: bool,
    tracking: bool,
    dragging: bool,
    tracked_touch: id, // weak; the touch dispatcher owns it
    drag_axis: Option<bool>, // true: horizontal
    velocity: CGPoint,
    last_touch_time: f64,
    settling: Option<Settling>,
    settling_timer: id,
    /// `UIScrollViewIndicatorStyle` — specifies the look of the scroll
    /// indicators. Per Apple's UIScrollView reference:
    /// https://developer.apple.com/documentation/uikit/uiscrollview/1619615-indicatorstyle
    indicator_style: UIScrollViewIndicatorStyle,
}
impl_HostObject_with_superclass!(UIScrollViewHostObject);

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
struct UIEdgeInsets {
    top: CGFloat,
    left: CGFloat,
    bottom: CGFloat,
    right: CGFloat,
}
unsafe impl SafeRead for UIEdgeInsets {}
impl GuestRet for UIEdgeInsets {}
impl GuestArg for UIEdgeInsets {
    const REG_COUNT: usize = 4;
    fn from_regs(regs: &[u32]) -> Self {
        UIEdgeInsets {
            top: GuestArg::from_regs(&regs[0..1]),
            left: GuestArg::from_regs(&regs[1..2]),
            bottom: GuestArg::from_regs(&regs[2..3]),
            right: GuestArg::from_regs(&regs[3..4]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        GuestArg::to_regs(self.top, &mut regs[0..1]);
        GuestArg::to_regs(self.left, &mut regs[1..2]);
        GuestArg::to_regs(self.bottom, &mut regs[2..3]);
        GuestArg::to_regs(self.right, &mut regs[3..4]);
    }
}

impl Default for UIScrollViewHostObject {
    fn default() -> Self {
        UIScrollViewHostObject {
            superclass: Default::default(),
            delegate: nil,
            scroll_enabled: true,
            content_offset: CGPoint { x: 0.0, y: 0.0 },
            content_size: CGSize {
                width: 0.0,
                height: 0.0,
            },
            content_inset: UIEdgeInsets::default(),
            shows_horizontal_scroll_indicator: true,
            shows_vertical_scroll_indicator: true,
            always_bounce_vertical: false,
            always_bounce_horizontal: false,
            bounces: true,
            paging_enabled: false,
            directional_lock_enabled: false,
            minimum_zoom_scale: 1.0,
            maximum_zoom_scale: 1.0,
            zoom_scale: 1.0,
            keyboard_dismiss_mode: UIScrollViewKeyboardDismissModeNone,
            decelerates: true,
            scrolls_to_top: true,
            can_cancel_content_touches: true, // <-- ДОБАВЛЕНО
            delays_content_touches: true,
            tracking: false,
            dragging: false,
            tracked_touch: nil,
            drag_axis: None,
            velocity: CGPoint::default(),
            last_touch_time: 0.0,
            settling: None,
            settling_timer: nil,
            indicator_style: 0,               // UIScrollViewIndicatorStyleDefault
        }
    }
}

struct Settling {
    started: Instant,
    from: CGPoint,
    to: CGPoint,
}

fn notify(env: &mut Environment, scroll: id, name: &str) {
    let delegate: id = msg![env; scroll delegate];
    let selector = env.objc.register_host_selector(name.into(), &mut env.mem);
    if delegate != nil && msg![env; delegate respondsToSelector:selector] {
        let _: () = crate::objc::msg_send_no_type_checking(env, (delegate, selector, scroll));
    }
}

fn stop_settling(env: &mut Environment, scroll: id) {
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(scroll);
    host.settling = None;
    let timer = std::mem::replace(&mut host.settling_timer, nil);
    if timer != nil {
        () = msg![env; timer invalidate];
        release(env, timer);
    }
}

pub(crate) fn begin_tracking(env: &mut Environment, scroll: id, touch: id) -> bool {
    let host = env.objc.borrow::<UIScrollViewHostObject>(scroll);
    if !host.scroll_enabled || (host.tracking && host.tracked_touch != touch) { return false; }
    stop_settling(env, scroll);
    let time: f64 = msg![env; touch timestamp];
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(scroll);
    host.tracking = true;
    host.tracked_touch = touch;
    host.velocity = CGPoint::default();
    host.last_touch_time = time;
    host.drag_axis = None;
    true
}

pub(crate) fn end_tracking(env: &mut Environment, scroll: id, touch: id) {
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(scroll);
    if host.tracked_touch == touch {
        host.tracking = false;
        host.dragging = false;
        host.tracked_touch = nil;
    }
}

pub(crate) fn begin_dragging(env: &mut Environment, scroll: id, touch: id) {
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(scroll);
    if host.tracked_touch == touch && !host.dragging {
        host.dragging = true;
        notify(env, scroll, "scrollViewWillBeginDragging:");
    }
}

pub(crate) fn can_scroll_in_direction(env: &mut Environment, scroll: id, delta: CGPoint) -> bool {
    let host = env.objc.borrow::<UIScrollViewHostObject>(scroll);
    if !host.scroll_enabled { return false; }
    let bounds: CGRect = msg![env; scroll bounds];
    let host = env.objc.borrow::<UIScrollViewHostObject>(scroll);
    if delta.x.abs() >= delta.y.abs() {
        host.content_size.width + host.content_inset.left + host.content_inset.right > bounds.size.width
            || (host.bounces && host.always_bounce_horizontal)
    } else {
        host.content_size.height + host.content_inset.top + host.content_inset.bottom > bounds.size.height
            || (host.bounces && host.always_bounce_vertical)
    }
}

fn clamp_offset(env: &mut Environment, scroll: id, offset: CGPoint) -> CGPoint {
    let bounds: CGRect = msg![env; scroll bounds];
    let h = env.objc.borrow::<UIScrollViewHostObject>(scroll);
    let min_x = -h.content_inset.left;
    let min_y = -h.content_inset.top;
    CGPoint {
        x: offset.x.clamp(min_x, (h.content_size.width - bounds.size.width + h.content_inset.right).max(min_x)),
        y: offset.y.clamp(min_y, (h.content_size.height - bounds.size.height + h.content_inset.bottom).max(min_y)),
    }
}

fn move_content(env: &mut Environment, scroll: id, offset: CGPoint) {
    let old: CGPoint = msg![env; scroll contentOffset];
    if offset != old {
        () = msg![env; scroll setContentOffset:offset];
        notify(env, scroll, "scrollViewDidScroll:");
    }
}

pub(crate) fn settle_step(env: &mut Environment, scroll: id, elapsed: f32) {
    let Some(s) = &env.objc.borrow::<UIScrollViewHostObject>(scroll).settling else { return; };
    let fraction = (elapsed / 0.25).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - fraction).powi(3);
    let offset = CGPoint { x: s.from.x + (s.to.x - s.from.x) * eased,
        y: s.from.y + (s.to.y - s.from.y) * eased };
    move_content(env, scroll, offset);
    if fraction >= 1.0 {
        stop_settling(env, scroll);
        notify(env, scroll, "scrollViewDidEndDecelerating:");
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIScrollView: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIScrollViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Delegate

- (id)delegate {
    env.objc.borrow::<UIScrollViewHostObject>(this).delegate
}
- (())setDelegate:(id)delegate {
    // Weak reference — no retain.
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).delegate = delegate;
}

// MARK: - Content offset

- (CGPoint)contentOffset {
    env.objc.borrow::<UIScrollViewHostObject>(this).content_offset
}
- (())setContentOffset:(CGPoint)offset {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).content_offset = offset;

    let mut bounds: CGRect = msg![env; this bounds];
    bounds.origin = offset;
    () = msg![env; this setBounds:bounds];
    () = msg![env; this setNeedsDisplay];
}
- (())setContentOffset:(CGPoint)offset animated:(bool)_animated {
    msg![env; this setContentOffset:offset]
}

// MARK: - Content size & inset

- (CGSize)contentSize {
    env.objc.borrow::<UIScrollViewHostObject>(this).content_size
}
- (())setContentSize:(CGSize)size {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).content_size = size;
}

// UIEdgeInsets passed as four floats (top, left, bottom, right) on the stack.
// We store them individually to avoid needing a SafeRead impl here.

- (UIEdgeInsets)contentInset {
    env.objc.borrow::<UIScrollViewHostObject>(this).content_inset
}
- (())setContentInset:(UIEdgeInsets)inset {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).content_inset = inset;
}
- (CGFloat)contentInsetTop {
    env.objc.borrow::<UIScrollViewHostObject>(this).content_inset.top
}
- (())setContentInset:(CGFloat)top left:(CGFloat)left bottom:(CGFloat)bottom right:(CGFloat)right {
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(this);
    host.content_inset = UIEdgeInsets { top, left, bottom, right };
}

// MARK: - Scroll enable / bounces

- (bool)scrollEnabled {
    env.objc.borrow::<UIScrollViewHostObject>(this).scroll_enabled
}
- (bool)isScrollEnabled {
    msg![env; this scrollEnabled]
}
- (())setScrollEnabled:(bool)enabled {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).scroll_enabled = enabled;
}

// MARK: - Touch handling properties

- (bool)canCancelContentTouches {
    env.objc.borrow::<UIScrollViewHostObject>(this).can_cancel_content_touches
}

- (())setCanCancelContentTouches:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).can_cancel_content_touches = value;
}

// MARK: - Scroll to top

- (bool)scrollsToTop {
    env.objc.borrow::<UIScrollViewHostObject>(this).scrolls_to_top
}

- (())setScrollsToTop:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).scrolls_to_top = value;
}

- (bool)bounces {
    env.objc.borrow::<UIScrollViewHostObject>(this).bounces
}
- (())setBounces:(bool)bounces {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).bounces = bounces;
}

- (bool)alwaysBounceVertical {
    env.objc.borrow::<UIScrollViewHostObject>(this).always_bounce_vertical
}
- (())setAlwaysBounceVertical:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).always_bounce_vertical = value;
}

- (bool)alwaysBounceHorizontal {
    env.objc.borrow::<UIScrollViewHostObject>(this).always_bounce_horizontal
}
- (())setAlwaysBounceHorizontal:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).always_bounce_horizontal = value;
}

- (bool)delaysContentTouches {
    env.objc.borrow::<UIScrollViewHostObject>(this).delays_content_touches
}
- (())setDelaysContentTouches:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).delays_content_touches = value;
}
- (bool)touchesShouldBegin:(id)_touches withEvent:(id)_event inContentView:(id)_view {
    true
}
- (bool)touchesShouldCancelInContentView:(id)view {
    // UIKit's default protects controls after their tracking has begun.
    // https://developer.apple.com/documentation/uikit/uiscrollview/touchesshouldcancel(in:)
    let control = env.objc.get_known_class("UIControl", &mut env.mem);
    !msg![env; view isKindOfClass:control]
}

// MARK: - Scroll indicators

- (bool)showsHorizontalScrollIndicator {
    env.objc.borrow::<UIScrollViewHostObject>(this).shows_horizontal_scroll_indicator
}
- (())setShowsHorizontalScrollIndicator:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).shows_horizontal_scroll_indicator = value;
}

- (bool)showsVerticalScrollIndicator {
    env.objc.borrow::<UIScrollViewHostObject>(this).shows_vertical_scroll_indicator
}
- (())setShowsVerticalScrollIndicator:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).shows_vertical_scroll_indicator = value;
}

- (UIScrollViewIndicatorStyle)indicatorStyle {
    env.objc.borrow::<UIScrollViewHostObject>(this).indicator_style
}
- (())setIndicatorStyle:(UIScrollViewIndicatorStyle)style {
    // Apple defines:
    // - UIScrollViewIndicatorStyleDefault (0)
    // - UIScrollViewIndicatorStyleBlack (1)
    // - UIScrollViewIndicatorStyleWhite (2)
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).indicator_style = style;
}

- (())flashScrollIndicators {
    // No-op — we don't render scroll indicators.
}

// MARK: - Paging & locking

- (bool)isPagingEnabled {
    env.objc.borrow::<UIScrollViewHostObject>(this).paging_enabled
}
- (())setPagingEnabled:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).paging_enabled = value;
}

- (bool)isDirectionalLockEnabled {
    env.objc.borrow::<UIScrollViewHostObject>(this).directional_lock_enabled
}
- (())setDirectionalLockEnabled:(bool)value {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).directional_lock_enabled = value;
}

// MARK: - Zooming

- (CGFloat)minimumZoomScale {
    env.objc.borrow::<UIScrollViewHostObject>(this).minimum_zoom_scale
}
- (())setMinimumZoomScale:(CGFloat)scale {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).minimum_zoom_scale = scale;
}

- (CGFloat)maximumZoomScale {
    env.objc.borrow::<UIScrollViewHostObject>(this).maximum_zoom_scale
}
- (())setMaximumZoomScale:(CGFloat)scale {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).maximum_zoom_scale = scale;
}

- (CGFloat)zoomScale {
    env.objc.borrow::<UIScrollViewHostObject>(this).zoom_scale
}
- (())setZoomScale:(CGFloat)scale {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).zoom_scale = scale;
}
- (())setZoomScale:(CGFloat)scale animated:(bool)_animated {
    msg![env; this setZoomScale:scale]
}
- (())zoomToRect:(CGRect)_rect animated:(bool)_animated {
    log!("TODO: UIScrollView zoomToRect:animated:");
}

// MARK: - Keyboard dismiss

- (UIScrollViewKeyboardDismissMode)keyboardDismissMode {
    env.objc.borrow::<UIScrollViewHostObject>(this).keyboard_dismiss_mode
}
- (())setKeyboardDismissMode:(UIScrollViewKeyboardDismissMode)mode {
    env.objc.borrow_mut::<UIScrollViewHostObject>(this).keyboard_dismiss_mode = mode;
}

// MARK: - Scrolling to visible area

- (())scrollRectToVisible:(CGRect)rect animated:(bool)_animated {
    let offset: CGPoint = msg![env; this contentOffset];
    let bounds: CGRect  = msg![env; this bounds];

    let mut new_x = offset.x;
    let mut new_y = offset.y;

    // Bring rect into view horizontally.
    if rect.origin.x < offset.x {
        new_x = rect.origin.x;
    } else if rect.origin.x + rect.size.width > offset.x + bounds.size.width {
        new_x = rect.origin.x + rect.size.width - bounds.size.width;
    }

    // Bring rect into view vertically.
    if rect.origin.y < offset.y {
        new_y = rect.origin.y;
    } else if rect.origin.y + rect.size.height > offset.y + bounds.size.height {
        new_y = rect.origin.y + rect.size.height - bounds.size.height;
    }

    let new_offset = CGPoint { x: new_x, y: new_y };
    if new_offset.x != offset.x || new_offset.y != offset.y {
        msg![env; this setContentOffset:new_offset]
    }
}

// MARK: - State queries

- (bool)isDecelerating {
    env.objc.borrow::<UIScrollViewHostObject>(this).settling.is_some()
}
- (bool)isDragging {
    env.objc.borrow::<UIScrollViewHostObject>(this).dragging
}
- (bool)isTracking {
    env.objc.borrow::<UIScrollViewHostObject>(this).tracking
}
- (bool)isZooming {
    false
}
- (bool)isZoomBouncing {
    false
}

// MARK: - Touch handling

- (())touchesBegan:(id)touches withEvent:(id)_event {
    let touch: id = msg![env; touches anyObject];
    begin_tracking(env, this, touch);
}

- (())touchesMoved:(id)touches withEvent:(id)_event {
    let touch: id = msg![env; touches anyObject];
    let host = env.objc.borrow::<UIScrollViewHostObject>(this);
    if !host.scroll_enabled || host.tracked_touch != touch || !host.dragging { return; }
    let prev_location: CGPoint = msg![env; touch previousLocationInView:this];
    let new_location:  CGPoint = msg![env; touch locationInView:this];
    let mut delta = CGPoint { x: new_location.x - prev_location.x, y: new_location.y - prev_location.y };
    let time: f64 = msg![env; touch timestamp];
    let host = env.objc.borrow_mut::<UIScrollViewHostObject>(this);
    if host.directional_lock_enabled {
        let horizontal = *host.drag_axis.get_or_insert(delta.x.abs() >= delta.y.abs());
        if horizontal { delta.y = 0.0; } else { delta.x = 0.0; }
    }
    let dt = time - host.last_touch_time;
    if dt > 0.0 {
        host.velocity = CGPoint { x: -delta.x / dt as f32, y: -delta.y / dt as f32 };
    }
    host.last_touch_time = time;
    let offset = host.content_offset;
    let new_offset = clamp_offset(env, this, CGPoint { x: offset.x - delta.x, y: offset.y - delta.y });
    move_content(env, this, new_offset);
}

- (())touchesEnded:(id)touches withEvent:(id)_event {
    let touch: id = msg![env; touches anyObject];
    let time: f64 = msg![env; touch timestamp];
    let host = env.objc.borrow::<UIScrollViewHostObject>(this);
    if host.tracked_touch != touch { return; }
    let was_dragging = host.dragging;
    let offset = host.content_offset;
    let velocity = if time - host.last_touch_time < 0.1 { host.velocity } else { CGPoint::default() };
    let paging = host.paging_enabled;
    end_tracking(env, this, touch);
    if !was_dragging { return; }
    let mut target = CGPoint { x: offset.x + velocity.x * 0.15, y: offset.y + velocity.y * 0.15 };
    if paging {
        let bounds: CGRect = msg![env; this bounds];
        let page = |position: f32, speed: f32, size: f32| {
            if size <= 0.0 { return position; }
            let current = position / size;
            let next = if speed.abs() > 300.0 {
                if speed > 0.0 { current.floor() + 1.0 } else { current.ceil() - 1.0 }
            } else { current.round() };
            next * size
        };
        target = CGPoint { x: page(offset.x, velocity.x, bounds.size.width),
            y: page(offset.y, velocity.y, bounds.size.height) };
    }
    target = clamp_offset(env, this, target);
    let decelerate = target != offset;
    let delegate: id = msg![env; this delegate];
    let selector = env.objc.register_host_selector("scrollViewDidEndDragging:willDecelerate:".into(), &mut env.mem);
    if delegate != nil && msg![env; delegate respondsToSelector:selector] {
        () = msg![env; delegate scrollViewDidEndDragging:this willDecelerate:decelerate];
    }
    if decelerate {
        env.objc.borrow_mut::<UIScrollViewHostObject>(this).settling = Some(Settling {
            started: Instant::now(), from: offset, to: target,
        });
        notify(env, this, "scrollViewWillBeginDecelerating:");
        let selector = env.objc.lookup_selector("_touchHLE_scrollStep:").unwrap();
        let timer: id = msg_class![env; NSTimer scheduledTimerWithTimeInterval:(1.0 / 60.0f64)
            target:this selector:selector userInfo:nil repeats:true];
        retain(env, timer);
        env.objc.borrow_mut::<UIScrollViewHostObject>(this).settling_timer = timer;
    }
}

- (())touchesCancelled:(id)touches withEvent:(id)_event {
    let touch: id = msg![env; touches anyObject];
    let dragging = env.objc.borrow::<UIScrollViewHostObject>(this).dragging;
    end_tracking(env, this, touch);
    if dragging {
        let delegate: id = msg![env; this delegate];
        let selector = env.objc.register_host_selector("scrollViewDidEndDragging:willDecelerate:".into(), &mut env.mem);
        if delegate != nil && msg![env; delegate respondsToSelector:selector] {
            () = msg![env; delegate scrollViewDidEndDragging:this willDecelerate:false];
        }
    }
}

- (())_touchHLE_scrollStep:(id)_timer {
    let elapsed = env.objc.borrow::<UIScrollViewHostObject>(this).settling.as_ref()
        .map(|s| s.started.elapsed().as_secs_f32());
    if let Some(elapsed) = elapsed { settle_step(env, this, elapsed); }
}

@end

};
