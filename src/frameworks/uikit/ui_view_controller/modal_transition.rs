/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Legacy UIKit cover-vertical modal presentation.
//! https://developer.apple.com/documentation/uikit/uimodaltransitionstyle/coververtical
//! https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view_controller.rs
use super::UIViewControllerHostObject;
use crate::frameworks::core_animation::ca_transform3d::CATransform3D;
use crate::frameworks::core_graphics::{CGPoint, CGRect};
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};
use crate::Environment;

#[derive(Default)]
struct ModalAnimation {
    presenter: id,
    modal: id,
    dismissing: bool,
}
impl HostObject for ModalAnimation {}

pub fn complete(env: &mut Environment, presenter: id, modal: id, dismissing: bool, animated: bool) {
    if !dismissing {
        env.objc
            .borrow_mut::<UIViewControllerHostObject>(modal)
            .is_being_presented = false;
        () = msg![env; modal viewDidAppear:animated];
        return;
    }
    let view: id = msg![env; modal view];
    () = msg![env; view removeFromSuperview];
    env.objc
        .borrow_mut::<UIViewControllerHostObject>(modal)
        .is_being_dismissed = false;
    env.objc
        .borrow_mut::<UIViewControllerHostObject>(modal)
        .is_being_presented = false;
    () = msg![env; modal viewDidDisappear:animated];
    env.objc
        .borrow_mut::<UIViewControllerHostObject>(modal)
        .presenting_view_controller = nil;
    if env
        .objc
        .borrow::<UIViewControllerHostObject>(presenter)
        .presented_view_controller
        == modal
    {
        env.objc
            .borrow_mut::<UIViewControllerHostObject>(presenter)
            .presented_view_controller = nil;
        release(env, modal);
    }
}

pub fn animate(env: &mut Environment, presenter: id, modal: id, dismissing: bool) {
    let view: id = msg![env; modal view];
    let layer: id = msg![env; view layer];
    let bounds: CGRect = msg![env; view bounds];
    let position: CGPoint = msg![env; layer position];
    let transform: CATransform3D = msg![env; layer transform];
    // Translate in the presented view's local vertical direction, so rotation
    // of a landscape controller never turns the slide into a sideways push.
    let below = CGPoint {
        x: position.x + transform.m21 * bounds.size.height,
        y: position.y + transform.m22 * bounds.size.height,
    };
    let key = get_static_str(env, "position");
    let animation: id = msg_class![env; CABasicAnimation animationWithKeyPath:key];
    let from: id =
        msg_class![env; NSValue valueWithCGPoint:(if dismissing { position } else { below })];
    let to: id =
        msg_class![env; NSValue valueWithCGPoint:(if dismissing { below } else { position })];
    () = msg![env; animation setFromValue:from];
    () = msg![env; animation setToValue:to];
    () = msg![env; animation setDuration:(0.4f64)];
    let curve = get_static_str(env, "easeInEaseOut");
    let timing: id = msg_class![env; CAMediaTimingFunction functionWithName:curve];
    () = msg![env; animation setTimingFunction:timing];
    let fill = get_static_str(env, "forwards");
    () = msg![env; animation setFillMode:fill];
    let delegate: id = msg_class![env; _VetusModalAnimation new];
    *env.objc.borrow_mut::<ModalAnimation>(delegate) = ModalAnimation {
        presenter: retain(env, presenter),
        modal: retain(env, modal),
        dismissing,
    };
    () = msg![env; animation setDelegate:delegate];
    let key = get_static_str(env, "VetusCoverVertical");
    () = msg![env; layer addAnimation:animation forKey:key];
    release(env, delegate);
}

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);
@implementation _VetusModalAnimation: NSObject
+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this, Box::<ModalAnimation>::default(), &mut env.mem)
}
- (())animationDidStop:(id)_animation finished:(bool)finished {
    let h = env.objc.borrow::<ModalAnimation>(this);
    let (presenter, modal, dismissing) = (h.presenter, h.modal, h.dismissing);
    if finished { complete(env, presenter, modal, dismissing, true); }
}
- (())dealloc {
    let h = env.objc.borrow::<ModalAnimation>(this);
    let (presenter, modal) = (h.presenter, h.modal);
    release(env, presenter);
    release(env, modal);
    env.objc.dealloc_object(this, &mut env.mem);
}
@end
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frameworks::core_graphics::CGSize;

    #[test]
    fn cover_vertical_keeps_presenting_content_visible_until_slide_finishes() {
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let _app: id = msg_class![env; UIApplication new];
        let frame = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 320.0,
                height: 480.0,
            },
        };
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:frame];
        let presenter: id = msg_class![env; UIViewController new];
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:frame];
        () = msg![env; presenter setView:root];
        () = msg![env; window addSubview:root];
        let modal: id = msg_class![env; UIViewController new];
        let modal_view: id = msg_class![env; UIView alloc];
        let modal_view: id = msg![env; modal_view initWithFrame:frame];
        () = msg![env; modal setView:modal_view];
        release(env, modal_view);
        () = msg![env; presenter presentModalViewController:modal animated:true];
        assert!(msg![env; modal isBeingPresented]);
        let view: id = msg![env; modal view];
        let layer: id = msg![env; view layer];
        let key = get_static_str(env, "VetusCoverVertical");
        let animation: id = msg![env; layer animationForKey:key];
        assert_ne!(animation, nil);
        let from: id = msg![env; animation fromValue];
        let to: id = msg![env; animation toValue];
        let from: CGPoint = msg![env; from CGPointValue];
        let to: CGPoint = msg![env; to CGPointValue];
        assert!(from.y > to.y);
        let duration: f64 = msg![env; animation duration];
        assert_eq!(duration, 0.4);
        let parent: id = msg![env; root superview];
        assert_eq!(parent, window);
        let views: id = msg![env; window subviews];
        let count: u32 = msg![env; views count];
        assert_eq!(
            count, 2,
            "a modal inserted an opaque backdrop over the presenting app"
        );
        let delegate: id = msg![env; animation delegate];
        () = msg![env; delegate animationDidStop:animation finished:true];
        () = msg![env; layer removeAnimationForKey:key];
        assert!(!msg![env; modal isBeingPresented]);
        () = msg![env; modal dismissModalViewControllerAnimated:true];
        assert!(msg![env; modal isBeingDismissed]);
        let parent: id = msg![env; view superview];
        assert_eq!(
            parent, window,
            "dismissal removed the page before its slide"
        );
        let animation: id = msg![env; layer animationForKey:key];
        let from: id = msg![env; animation fromValue];
        let to: id = msg![env; animation toValue];
        let from: CGPoint = msg![env; from CGPointValue];
        let to: CGPoint = msg![env; to CGPointValue];
        assert!(to.y > from.y);
        let delegate: id = msg![env; animation delegate];
        () = msg![env; delegate animationDidStop:animation finished:true];
        () = msg![env; layer removeAnimationForKey:key];
        let parent: id = msg![env; view superview];
        assert_eq!(parent, nil);
        let parent: id = msg![env; root superview];
        assert_eq!(parent, window);
        let presented: id = msg![env; presenter presentedViewController];
        assert_eq!(presented, nil);
        () = msg![env; presenter presentModalViewController:modal animated:false];
        assert!(!msg![env; modal isBeingPresented]);
        let animation: id = msg![env; layer animationForKey:key];
        assert_eq!(animation, nil);
        () = msg![env; presenter dismissModalViewControllerAnimated:false];
        release(env, modal);
        release(env, root);
        release(env, presenter);
        release(env, window);
        release(env, pool);
    }
}
