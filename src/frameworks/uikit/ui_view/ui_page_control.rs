/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIPageControl`.

use super::ui_control::UIControlHostObject;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::NSInteger;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, release,
    retain, ClassExports, NSZonePtr,
};

#[derive(Default)]
struct UIPageControlHostObject {
    superclass: UIControlHostObject,
    number_of_pages: NSInteger,
    current_page: NSInteger,
    displayed_page: NSInteger,
    dots: Vec<id>,
    hides_for_single_page: bool,
    defers_current_page_display: bool,
    /// UIColor* for the dots that are not current.
    page_indicator_tint_color: id,
    /// UIColor* for the current-page dot.
    current_page_indicator_tint_color: id,
}
impl_HostObject_with_superclass!(UIPageControlHostObject);

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIPageControl: UIControl

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIPageControlHostObject {
        superclass: UIControlHostObject::default(),
        number_of_pages: 0,
        current_page: 0,
        displayed_page: 0,
        dots: Vec::new(),
        hides_for_single_page: false,
        defers_current_page_display: false,
        page_indicator_tint_color: nil,
        current_page_indicator_tint_color: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Size hint

// Returns the minimum size needed to display the dots for a given page count.
// Space the legacy indicators with 7pt dots and 9pt gaps.
- (CGSize)sizeForNumberOfPages:(NSInteger)page_count {
    let width: f32 = (page_count.max(0) as f32) * 16.0 - 9.0;
    CGSize {
        width: width.max(0.0),
        height: 36.0,
    }
}

- (id)init {
    msg![env; this initWithFrame:(CGRect::default())]
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    () = msg![env; this setOpaque:false];
    this
}

- (())dealloc {
    let host = env.objc.borrow::<UIPageControlHostObject>(this);
    let (tint, current_tint) = (
        host.page_indicator_tint_color,
        host.current_page_indicator_tint_color,
    );
    release(env, tint);
    release(env, current_tint);
    let dots = std::mem::take(&mut env.objc.borrow_mut::<UIPageControlHostObject>(this).dots);
    for dot in dots { release(env, dot); }
    msg_super![env; this dealloc]
}

- (())layoutSubviews {
    let bounds: CGRect = msg![env; this bounds];
    let host = env.objc.borrow::<UIPageControlHostObject>(this);
    let count = host.number_of_pages.max(0) as usize;
    let displayed = host.displayed_page;
    let hide = host.hides_for_single_page && count <= 1;
    let tint = host.page_indicator_tint_color;
    let selected_tint = host.current_page_indicator_tint_color;
    if host.dots.len() != count {
        let old = std::mem::take(&mut env.objc.borrow_mut::<UIPageControlHostObject>(this).dots);
        for dot in old {
            () = msg![env; dot removeFromSuperview];
            release(env, dot);
        }
        for _ in 0..count {
            let dot: id = msg_class![env; UIView new];
            () = msg![env; dot setUserInteractionEnabled:false];
            let layer: id = msg![env; dot layer];
            () = msg![env; layer setCornerRadius:(3.5f32)];
            () = msg![env; this addSubview:dot];
            env.objc.borrow_mut::<UIPageControlHostObject>(this).dots.push(dot);
        }
    }
    let width = (count as f32 * 16.0 - 9.0).max(0.0);
    let left = bounds.origin.x + (bounds.size.width - width) / 2.0;
    let top = bounds.origin.y + (bounds.size.height - 7.0) / 2.0;
    let dots = env.objc.borrow::<UIPageControlHostObject>(this).dots.clone();
    for (index, dot) in dots.into_iter().enumerate() {
        let selected = index as NSInteger == displayed;
        let color = if selected { selected_tint } else { tint };
        let alpha: f32 = if !selected && color == nil { 0.3 } else { 1.0 };
        let color = if color == nil { msg_class![env; UIColor whiteColor] } else { color };
        () = msg![env; dot setBackgroundColor:color];
        () = msg![env; dot setAlpha:alpha];
        () = msg![env; dot setHidden:hide];
        let frame = CGRect { origin: CGPoint { x: left + index as f32 * 16.0, y: top }, size: CGSize { width: 7.0, height: 7.0 } };
        () = msg![env; dot setFrame:frame];
    }
}

// MARK: - Page count

- (NSInteger)numberOfPages {
    env.objc.borrow::<UIPageControlHostObject>(this).number_of_pages
}

- (())setNumberOfPages:(NSInteger)number_of_pages {
    log_dbg!("UIPageControl setNumberOfPages:{}", number_of_pages);
    let host = env.objc.borrow_mut::<UIPageControlHostObject>(this);
    let number_of_pages = number_of_pages.max(0);
    host.number_of_pages = number_of_pages;
    // Clamp current page to valid range.
    if number_of_pages == 0 {
        host.current_page = 0;
    } else if host.current_page >= number_of_pages {
        host.current_page = number_of_pages - 1;
    }
    host.displayed_page = host.current_page;
    () = msg![env; this setNeedsLayout];
}

// MARK: - Current page

- (NSInteger)currentPage {
    env.objc.borrow::<UIPageControlHostObject>(this).current_page
}

- (())setCurrentPage:(NSInteger)current_page {
    log_dbg!("UIPageControl setCurrentPage:{}", current_page);
    let n = env.objc.borrow::<UIPageControlHostObject>(this).number_of_pages;
    let clamped = if n == 0 {
        0
    } else {
        current_page.max(0).min(n - 1)
    };
    let host = env.objc.borrow_mut::<UIPageControlHostObject>(this);
    host.current_page = clamped;
    if !host.defers_current_page_display {
        host.displayed_page = clamped;
        () = msg![env; this setNeedsLayout];
    }
}

// MARK: - Display options

- (bool)hidesForSinglePage {
    env.objc.borrow::<UIPageControlHostObject>(this).hides_for_single_page
}

- (())setHidesForSinglePage:(bool)hides {
    env.objc.borrow_mut::<UIPageControlHostObject>(this).hides_for_single_page = hides;
    () = msg![env; this setNeedsLayout];
}

- (bool)defersCurrentPageDisplay {
    env.objc.borrow::<UIPageControlHostObject>(this).defers_current_page_display
}

- (())setDefersCurrentPageDisplay:(bool)defers {
    env.objc.borrow_mut::<UIPageControlHostObject>(this).defers_current_page_display = defers;
}

// Publish a deferred currentPage change to the displayed indicators.
- (())updateCurrentPageDisplay {
    let host = env.objc.borrow_mut::<UIPageControlHostObject>(this);
    host.displayed_page = host.current_page;
    () = msg![env; this setNeedsLayout];
}

// MARK: - Tint colors

- (id)pageIndicatorTintColor { // UIColor*
    env.objc.borrow::<UIPageControlHostObject>(this).page_indicator_tint_color
}

- (())setPageIndicatorTintColor:(id)color { // UIColor*
    let old = env.objc.borrow::<UIPageControlHostObject>(this).page_indicator_tint_color;
    retain(env, color);
    release(env, old);
    env.objc.borrow_mut::<UIPageControlHostObject>(this).page_indicator_tint_color = color;
    () = msg![env; this setNeedsLayout];
}

- (id)currentPageIndicatorTintColor { // UIColor*
    env.objc.borrow::<UIPageControlHostObject>(this).current_page_indicator_tint_color
}

- (())setCurrentPageIndicatorTintColor:(id)color { // UIColor*
    let old = env.objc.borrow::<UIPageControlHostObject>(this)
        .current_page_indicator_tint_color;
    retain(env, color);
    release(env, old);
    env.objc.borrow_mut::<UIPageControlHostObject>(this)
        .current_page_indicator_tint_color = color;
    () = msg![env; this setNeedsLayout];
}

// MARK: - Description

- (id)description {
    let (n, cur) = {
        let h = env.objc.borrow::<UIPageControlHostObject>(this);
        (h.number_of_pages, h.current_page)
    };
    let s = format!(
        "<UIPageControl: {:?}; numberOfPages={}; currentPage={}>",
        this, n, cur
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_size_and_page_updates_layout_visible_indicators() {
        // https://developer.apple.com/documentation/uikit/uipagecontrol/size(fornumberofpages:)
        // https://developer.apple.com/documentation/uikit/uipagecontrol/deferscurrentpagedisplay
        // Upstream touchHLE models UIView layout, but has no UIPageControl.
        // https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view.rs
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let control: id = msg_class![env; UIPageControl new];
        let size: CGSize = msg![env; control sizeForNumberOfPages:(3i32)];
        assert!(size.width >= 3.0 * 7.0 && size.height >= 7.0);
        let frame = CGRect {
            origin: CGPoint::default(),
            size,
        };
        () = msg![env; control setFrame:frame];
        () = msg![env; control setNumberOfPages:(3i32)];
        () = msg![env; control layoutIfNeeded];
        let dots = env
            .objc
            .borrow::<UIPageControlHostObject>(control)
            .dots
            .clone();
        assert_eq!(dots.len(), 3);
        let first: CGRect = msg![env; (dots[0]) frame];
        let last: CGRect = msg![env; (dots[2]) frame];
        let first_x = first.origin.x;
        assert_eq!(first_x, size.width - last.origin.x - last.size.width);
        let alpha: f32 = msg![env; (dots[0]) alpha];
        assert_eq!(alpha, 1.0);
        () = msg![env; control setDefersCurrentPageDisplay:true];
        () = msg![env; control setCurrentPage:(2i32)];
        () = msg![env; control layoutIfNeeded];
        let alpha: f32 = msg![env; (dots[2]) alpha];
        assert_eq!(alpha, 0.3);
        () = msg![env; control updateCurrentPageDisplay];
        () = msg![env; control layoutIfNeeded];
        let alpha: f32 = msg![env; (dots[2]) alpha];
        assert_eq!(alpha, 1.0);
        () = msg![env; control setNumberOfPages:(1i32)];
        () = msg![env; control setHidesForSinglePage:true];
        () = msg![env; control layoutIfNeeded];
        let dot = env.objc.borrow::<UIPageControlHostObject>(control).dots[0];
        let hidden: bool = msg![env; dot isHidden];
        assert!(hidden);
        release(env, control);
        release(env, pool);
    }
}
