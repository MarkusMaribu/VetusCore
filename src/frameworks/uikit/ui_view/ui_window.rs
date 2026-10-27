/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIWindow`.
//!
//! Useful resources:
//! - [Technical Q&A QA1588: Automatic orientation support for iPhone and iPad apps](https://developer.apple.com/library/archive/qa/qa1588/_index.html)
//! - [Technical Q&A QA1688: Why won't my UIViewController rotate with the device?](https://developer.apple.com/library/archive/qa/qa1688/_index.html)

use super::UIViewHostObject;
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_graphics::cg_affine_transform::{
    CGAffineTransform, CGAffineTransformIdentity,
};
use crate::frameworks::core_graphics::{CGPoint, CGRect};
use crate::frameworks::foundation::ns_string;
use crate::frameworks::uikit::ui_application::{
    UIInterfaceOrientationLandscapeLeft, UIInterfaceOrientationLandscapeRight,
    UIInterfaceOrientationPortraitUpsideDown,
};
use crate::frameworks::uikit::ui_device::{
    UIDeviceOrientationLandscapeLeft, UIDeviceOrientationLandscapeRight,
    UIDeviceOrientationPortraitUpsideDown,
};
use crate::objc::{
    id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    /// List of visible windows for internal purposes. Non-retaining!
    ///
    /// This is public because Core Animation also uses it.
    pub windows: Vec<id>,
    /// Initial rotation is applied after the application finishes constructing
    /// its interface in the launch delegate callback.
    pub defer_autorotation: bool,
    pending_autorotation: Vec<(id, id)>,
    /// The most recent window which received `makeKeyAndVisible` message.
    /// Non-retaining!
    pub key_window: Option<id>,
    /// Per-window `rootViewController` association (iOS 4+).
    ///
    /// `UIWindow` itself derives from `UIView` whose host object is shared
    /// with every plain view; we therefore keep the root-VC association out
    /// of the per-view struct and indexed by window pointer instead. The VC
    /// is retained while the window holds it, mirroring Apple's documented
    /// strong-property semantics.
    pub root_view_controllers: HashMap<id, id>,
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIWindow: UIView

// TODO: more?

// UIWindow levels are only relevant when multiple windows overlap. The
// compositor already draws windows in insertion order, so retain the value
// for API compatibility without changing the single-screen ordering.
- (())setWindowLevel:(f32)_level { }
- (f32)windowLevel { 0.0 }

- (id)initWithFrame:(CGRect)frame {
    let this = msg_super![env; this initWithFrame:frame];
    // Undocumented: windows seem to be hidden by default on iOS, unlike views.
    // Super call to bypass the overriden setter on this class, which would post
    // a notification.
    () = msg_super![env; this setHidden:true];

    let list = &mut env.framework_state.uikit.ui_view.ui_window.windows;
    list.push(this);
    log_dbg!(
        "New window: {:?}. New list of all windows: {:?}",
        this,
        list,
    );

    this
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let this = msg_super![env; this initWithCoder:coder];
    // Undocumented: windows seem to be hidden by default on iOS, unlike views.
    // Super call to bypass the overriden setter on this class, which would post
    // a notification.
    () = msg_super![env; this setHidden:true];

    // Real iOS forces UIWindow.frame to match UIScreen.bounds regardless of
    // whatever frame was encoded in the NIB. Interface Builder by default
    // uses a 320x460 canvas (status bar visible) for iPhone XIBs, so a
    // window loaded from a NIB will normally come out at 320x460 and miss
    // the bottom 20px of the screen. Apps that hide the status bar via
    // Info.plist (e.g. Minecraft PE 0.6.x) then end up with their EAGLView
    // sized to the truncated window, producing a visible offset/shift.
    //
    // Mirror Apple's behaviour by re-setting the frame to the full screen
    // bounds after the super decoder runs.
    let screen: id = msg_class![env; UIScreen mainScreen];
    let screen_bounds: CGRect = msg![env; screen bounds];
    let current_bounds: CGRect = msg![env; this bounds];
    if current_bounds.size != screen_bounds.size {
        log_dbg!(
            "UIWindow {:?}: overriding NIB-encoded size {:?} with UIScreen.bounds size {:?}",
            this,
            current_bounds.size,
            screen_bounds.size,
        );
        () = msg![env; this setFrame:screen_bounds];
    }

    let list = &mut env.framework_state.uikit.ui_view.ui_window.windows;
    list.push(this);
    log_dbg!(
        "New window: {:?}. New list of all windows: {:?}",
        this,
        list,
    );

    this
}

- (())dealloc {
    if let Some(key_window) = env.framework_state.uikit.ui_view.ui_window.key_window {
        if key_window == this {
            env.framework_state.uikit.ui_view.ui_window.key_window = None;
        }
    }
    // Release the strong `rootViewController` reference, if any.
    if let Some(root_vc) = env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .root_view_controllers
        .remove(&this)
    {
        release(env, root_vc);
    }
    let list = &mut env.framework_state.uikit.ui_view.ui_window.windows;
    // A window may be deallocated without ever being registered (e.g.
    // `[[UIWindow alloc] init]`, which skips both designated initializers).
    // Never panic on guest-driven paths.
    if let Some(idx) = list.iter().position(|&w| w == this) {
        list.remove(idx);
    }
    log_dbg!(
        "Deallocating window {:?}. New list of all windows: {:?}",
        this,
        list,
    );
    msg_super![env; this dealloc]
}

// Permissive hit-testing for the application window.
//
// On real iOS, UIWindow's frame matches UIScreen.bounds and touches always
// land inside it, so the standard UIView hitTest implementation is fine.
// In touchHLE, however, the host window can be larger than the iPhone's
// virtual screen (Android phones, large desktop windows). Even with the
// touch-coordinate clamp in `transform_input_coords`, edge cases such as
// the rootViewController's view being rotated for landscape orientation
// can leave individual modal/overlay subviews positioned in such a way
// that a touch landing on them produces a window-coordinate point just
// outside of UIWindow's own bounds (off-by-one at the bottom row, status
// bar overlap, etc.). The default UIView.hitTest then early-exits via
// pointInside, returning nil for the entire touch — which is what
// produced the `SUPER HACK: Forcing rejected touch ...` log spam and
// stopped the in-game chat / Create World text fields from ever becoming
// first responder.
//
// Override hitTest to ALWAYS recurse into subviews. If any subview claims
// the touch we return it; otherwise we fall back to the window itself so
// touch dispatch is never lost. This matches Apple's documented intent:
// "Windows don't actively participate in event handling. They merely
// pass events on to their subviews."
- (id)hitTest:(CGPoint)point withEvent:(id)event {
    let subviews = env.objc.borrow::<super::UIViewHostObject>(this).subviews.clone();
    for subview in subviews.into_iter().rev() {
        let hidden: bool = msg![env; subview isHidden];
        let alpha: crate::frameworks::core_graphics::CGFloat = msg![env; subview alpha];
        let interactible: bool = msg![env; subview isUserInteractionEnabled];
        if hidden || alpha < 0.01 || !interactible { continue; }
        let sub_point: CGPoint = msg![env; subview convertPoint:point fromView:this];
        let hit: id = msg![env; subview hitTest:sub_point withEvent:event];
        if hit != nil { return hit; }
    }
    this
}

- (())setHidden:(bool)is_hidden {
    () = msg_super![env; this setHidden:is_hidden];

    let notif_name = ns_string::get_static_str(
        env,
        if is_hidden {
            UIWindowDidBecomeHiddenNotification
        } else {
            UIWindowDidBecomeVisibleNotification
        },
    );
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    () = msg![env; center postNotificationName:notif_name object:this userInfo:nil];

    log_dbg!("[(UIWindow*){:?} setHidden:{:?}]", this, is_hidden);
}

- (())makeKeyWindow {
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];

    if let Some(previous) = env.framework_state.uikit.ui_view.ui_window.key_window {
        if previous != this {
            let notif_name =
                ns_string::get_static_str(env, UIWindowDidResignKeyNotification);
            () = msg![env; center postNotificationName:notif_name object:previous userInfo:nil];
        }
    }

    env.framework_state.uikit.ui_view.ui_window.key_window = Some(this);

    let notif_name = ns_string::get_static_str(env, UIWindowDidBecomeKeyNotification);
    () = msg![env; center postNotificationName:notif_name object:this userInfo:nil];
}

- (bool)isKeyWindow {
    env.framework_state.uikit.ui_view.ui_window.key_window == Some(this)
}

- (())makeKeyAndVisible {
    // MakeKeyVisibleFix (from XaViewDnK fork): some engines (e.g. Asphalt 8's
    // Jet) create the window with a zero frame and only size it via
    // `makeKeyAndVisible`. Without this, the window stays 0x0 and every
    // layer composite is empty => black screen.
    let screen: id = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];
    let frame: CGRect = msg![env; this frame];
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
        log!("Fixing empty window frame to {:?}", bounds);
        () = msg![env; this setFrame:bounds];
    }

    // Bump the window to the top of the touch list (was a FIXME upstream).
    let list = &mut env.framework_state.uikit.ui_view.ui_window.windows;
    if let Some(idx) = list.iter().position(|&w| w == this) {
        let w = list.remove(idx);
        list.push(w);
    }

    () = msg![env; this makeKeyWindow];
    () = msg![env; this setHidden:false];
}

// Legacy iOS 2/3 pattern: [window setContentView:someView]
// Semantically equivalent to addSubview: for UIWindow.
- (())setContentView:(id)view {
    log_dbg!("[(UIWindow*){:?} setContentView:{:?}]", this, view);
    () = msg![env; this addSubview:view];
}

// Returns the first subview as the content view (legacy behaviour).
- (id)contentView {
    let subviews = &env.objc.borrow::<UIViewHostObject>(this).subviews;
    subviews.first().copied().unwrap_or(nil)
}

// Support for rootViewController (iOS 4+)
// Per Apple's [UIWindow Reference](https://developer.apple.com/documentation/uikit/uiwindow/1621581-rootviewcontroller):
// `rootViewController` is a strong reference. Setting a new root view
// controller installs its `-view` as a full-bounds subview of the window,
// optionally replacing the previously installed root view. We mirror that
// contract exactly: retain the new VC, release the old one, swap the
// associated view, and notify the existing appearance lifecycle through
// `-addSubview:`.
- (())setRootViewController:(id)view_controller {
    log_dbg!("[(UIWindow*){:?} setRootViewController:{:?}]", this, view_controller);

    let previous = env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .root_view_controllers
        .get(&this)
        .copied()
        .unwrap_or(nil);

    if previous == view_controller {
        return;
    }

    // Retain BEFORE releasing the old one, so that if the new VC was the
    // last strong holder of the old VC we don't temporarily drop it.
    if view_controller != nil {
        retain(env, view_controller);
    }

    if previous != nil {
        let previous_view: id = msg![env; previous view];
        if previous_view != nil {
            () = msg![env; previous_view removeFromSuperview];
        }
        release(env, previous);
    }

    if view_controller != nil {
        let view: id = msg![env; view_controller view];
        // Apple's docs: the root view controller's view is resized to fill
        // the window's bounds. Match the window bounds before adding so
        // -layoutSubviews observes the right geometry.
        let bounds: CGRect = msg![env; this bounds];
        () = msg![env; view setFrame:bounds];
        () = msg![env; this addSubview:view];
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .root_view_controllers
            .insert(this, view_controller);
    } else {
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .root_view_controllers
            .remove(&this);
    }
}

- (id)rootViewController {
    env.framework_state
        .uikit
        .ui_view
        .ui_window
        .root_view_controllers
        .get(&this)
        .copied()
        .unwrap_or(nil)
}

// UIResponder implementation
// From the Apple UIView docs regarding [UIResponder nextResponder]:
// "UIWindow returns the application object."
- (id)nextResponder {
    msg_class![env; UIApplication sharedApplication]
}

- (())addSubview:(id)view {
    log_dbg!("[(UIWindow*){:?} addSubview:{:?}] => ()", this, view);

    if view == nil || env.objc.borrow::<UIViewHostObject>(view).view_controller == nil {
        () = msg_super![env; this addSubview:view];
        return;
    }

    // Below we treat a special case of adding a view controller's view
    // to a window, in order to generate the appropriate display-related
    // notifications. Apple's UIViewController contract:
    //   - viewWillAppear: is sent before the view is added to the
    //     view hierarchy AND will become visible.
    //   - viewDidAppear:  is sent after that.
    // When the view is already a subview, [UIView addSubview:] still
    // moves it to the top of the subviews array (Apple docs:
    // "Calls to this method with a view that is already a subview
    //  result in the view being moved to the end of the subview list.")
    // After moving to the top there are no obstructing siblings, so the
    // appearance lifecycle is fired iff the view is not explicitly
    // hidden via -setHidden:YES. The previous TODO existed because
    // touchHLE had no way to detect this case; we now defer to
    // -isHidden, which mirrors UIKit's actual visibility test.
    let was_subview = env
        .objc
        .borrow::<UIViewHostObject>(this)
        .subviews
        .contains(&view);
    let view_hidden: bool = msg![env; view isHidden];
    let should_fire_appearance = !view_hidden;
    if was_subview {
        log_dbg!(
            "[(UIWindow*){:?} addSubview:{:?}]: re-adding existing subview; \
             moving to top, appearance lifecycle will{} fire (hidden={}).",
            this,
            view,
            if should_fire_appearance { "" } else { " not" },
            view_hidden
        );
    }

    let vc = env.objc.borrow::<UIViewHostObject>(view).view_controller;
    if should_fire_appearance {
        () = msg![env; vc viewWillAppear:false];
    }
    () = msg_super![env; this addSubview:view];
    if should_fire_appearance {
        () = msg![env; vc viewDidAppear:false];
    }

    let rotation = env.window.as_ref().map(|window| window.current_rotation());
    queue_or_autorotate_view(env, this, view, rotation);
}

- (CGPoint)convertPoint:(CGPoint)point
             fromWindow:(id)other { // UIWindow*
    let this_layer: id = msg![env; this layer];
    // Resolves to nil if other is nil.
    let other_layer: id = msg![env; other layer];
    msg![env; this_layer convertPoint:point fromLayer:other_layer]
}
- (CGPoint)convertPoint:(CGPoint)point
               toWindow:(id)other { // UIWindow*
    let this_layer: id = msg![env; this layer];
    // Resolves to nil if other is nil.
    let other_layer: id = msg![env; other layer];
    msg![env; this_layer convertPoint:point toLayer:other_layer]
}

// Apple's
// <https://developer.apple.com/documentation/uikit/uiwindow/1621597-screen>:
// `screen` returns the `UIScreen` the window is currently displayed on,
// and the documented setter (`setScreen:`) is used by apps that wish to
// move a window to an external display. We emulate a single main screen
// only, so the getter always returns `+[UIScreen mainScreen]` and the
// setter is a no-op that just acknowledges the call to keep the
// "doesNotRecognizeSelector" warning from firing on apps that
// unconditionally call `window.screen = UIScreen.mainScreen` during
// `application:didFinishLaunchingWithOptions:`.
- (id)screen {
    msg_class![env; UIScreen mainScreen]
}
- (())setScreen:(id)_screen {
    log_dbg!(
        "[UIWindow setScreen:] ignored; touchHLE only emulates the main screen."
    );
}

@end

};

/// Window life-cycle notifications.
///
/// Apple `UIWindow.h` declares each name as
/// `UIKIT_EXTERN NSNotificationName const ...`. The literal value
/// posted to `NSNotificationCenter` is the symbol's own name, which is
/// what `-[NSNotificationCenter addObserver:selector:name:object:]`
/// compares against with `-[NSString isEqualToString:]`.
///
/// References:
/// * Apple [UIWindow notifications](https://developer.apple.com/documentation/uikit/uiwindow)
const UIWindowDidBecomeKeyNotification: &str = "UIWindowDidBecomeKeyNotification";
const UIWindowDidResignKeyNotification: &str = "UIWindowDidResignKeyNotification";
const UIWindowDidBecomeHiddenNotification: &str = "UIWindowDidBecomeHiddenNotification";
const UIWindowDidBecomeVisibleNotification: &str = "UIWindowDidBecomeVisibleNotification";
/// Keyboard notifications
/// TODO: more keyboard notifications
pub const UIKeyboardWillShowNotification: &str = "UIKeyboardWillShowNotification";
pub const UIKeyboardDidShowNotification: &str = "UIKeyboardDidShowNotification";
pub const UIKeyboardWillHideNotification: &str = "UIKeyboardWillHideNotification";
pub const UIKeyboardDidHideNotification: &str = "UIKeyboardDidHideNotification";
pub const UIKeyboardBoundsUserInfoKey: &str = "UIKeyboardBoundsUserInfoKey";

pub const CONSTANTS: ConstantExports = &[
    (
        "_UIWindowDidBecomeKeyNotification",
        HostConstant::NSString(UIWindowDidBecomeKeyNotification),
    ),
    (
        "_UIWindowDidResignKeyNotification",
        HostConstant::NSString(UIWindowDidResignKeyNotification),
    ),
    (
        "_UIWindowDidBecomeHiddenNotification",
        HostConstant::NSString(UIWindowDidBecomeHiddenNotification),
    ),
    (
        "_UIWindowDidBecomeVisibleNotification",
        HostConstant::NSString(UIWindowDidBecomeVisibleNotification),
    ),
    // _UIKeyboardWillShowNotification, _UIKeyboardDidShowNotification,
    // _UIKeyboardWillHideNotification, _UIKeyboardDidHideNotification and
    // _UIKeyboardBoundsUserInfoKey are exported from
    // uikit::ui_keyboard::CONSTANTS; not duplicated here.
];

/// Keep a window's root hierarchy synchronized when its controller replaces
/// its view, including replacing a placeholder during launch. The window
/// continues to own the same controller; it owns and sizes the current view.
/// https://developer.apple.com/documentation/uikit/uiwindow/rootviewcontroller
pub(crate) fn replace_controller_root_view(
    env: &mut crate::Environment,
    controller: id,
    old_view: id,
    new_view: id,
) {
    let windows: Vec<id> = env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .root_view_controllers
        .iter()
        .filter_map(|(&window, &root)| (root == controller).then_some(window))
        .collect();
    for window in windows {
        retain(env, window);
        let index = env
            .objc
            .borrow::<UIViewHostObject>(window)
            .subviews
            .iter()
            .position(|&view| view == old_view)
            .unwrap_or(0) as i32;
        let transform: CGAffineTransform = if old_view == nil {
            CGAffineTransformIdentity
        } else {
            msg![env; old_view transform]
        };
        if old_view != nil {
            () = msg![env; old_view removeFromSuperview];
        }
        if new_view != nil {
            let bounds: CGRect = msg![env; window bounds];
            () = msg![env; new_view setTransform:transform];
            () = msg![env; new_view setFrame:bounds];
            // Preserve overlays above the root. Insertion also avoids
            // repeating controller appearance events for a view replacement.
            () = msg![env; window insertSubview:new_view atIndex:index];
            let rotation = env.window.as_ref().map(|window| window.current_rotation());
            queue_or_autorotate_view(env, window, new_view, rotation);
        }
        release(env, window);
    }
}

fn queue_or_autorotate_view(
    env: &mut crate::Environment,
    window: id,
    view: id,
    rotation: Option<crate::window::DeviceOrientation>,
) {
    if env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .defer_autorotation
    {
        retain(env, window);
        retain(env, view);
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .pending_autorotation
            .push((window, view));
    } else if let Some(rotation) = rotation {
        autorotate_view(env, window, view, rotation, None);
    }
}

/// UIKit rotates the initial interface after the launch delegate returns, not
/// while the delegate is still constructing and sizing its child views.
/// https://developer.apple.com/documentation/uikit/uiviewcontroller
pub(crate) fn finish_launch_autorotation(env: &mut crate::Environment) {
    let rotation = env.window.as_ref().map(|window| window.current_rotation());
    let screen_bounds = env.window.as_ref().map(|window| {
        let (width, height) = window.screen_coordinate_size();
        CGRect {
            origin: CGPoint::default(),
            size: crate::frameworks::core_graphics::CGSize {
                width: width as f32,
                height: height as f32,
            },
        }
    });
    finish_launch_autorotation_with_rotation(env, rotation, screen_bounds);
}

fn finish_launch_autorotation_with_rotation(
    env: &mut crate::Environment,
    rotation: Option<crate::window::DeviceOrientation>,
    screen_bounds: Option<CGRect>,
) {
    env.framework_state
        .uikit
        .ui_view
        .ui_window
        .defer_autorotation = false;
    let pending = std::mem::take(
        &mut env
            .framework_state
            .uikit
            .ui_view
            .ui_window
            .pending_autorotation,
    );
    for (window, view) in pending {
        let parent: id = msg![env; view superview];
        if parent == window {
            if let Some(rotation) = rotation {
                autorotate_view(env, window, view, rotation, screen_bounds);
            }
        }
        release(env, view);
        release(env, window);
    }
}

fn autorotate_view(
    env: &mut crate::Environment,
    window: id,
    view: id,
    rotation: crate::window::DeviceOrientation,
    screen_bounds: Option<CGRect>,
) {
    let vc = env.objc.borrow::<UIViewHostObject>(view).view_controller;
    if vc == nil {
        return;
    }
    use crate::frameworks::uikit::ui_view_controller::UIViewControllerHostObject;
    if env
        .objc
        .borrow::<UIViewControllerHostObject>(vc)
        .autorotation_in_progress
    {
        return;
    }
    // Apply the legacy controller orientation policy once its view is mounted
    // and launch construction is complete. Runtime sensor-driven autorotation
    // and the iOS 6 supportedInterfaceOrientations policy are not handled here.
    // The caller supplies the orientation so headless tests can exercise the
    // same geometry and launch-order behavior without a host GL window.
    if let Some(orientation) = Some(rotation).and_then(|rotation| match rotation {
        crate::window::DeviceOrientation::LandscapeLeft => Some(UIDeviceOrientationLandscapeLeft),
        crate::window::DeviceOrientation::LandscapeRight => Some(UIDeviceOrientationLandscapeRight),
        crate::window::DeviceOrientation::PortraitUpsideDown => {
            Some(UIDeviceOrientationPortraitUpsideDown)
        }
        // Portrait is the default so we don't do anything here.
        crate::window::DeviceOrientation::Portrait => None,
    }) {
        // (UIInterfaceOrientation and UIDeviceOrientation are compatible enums,
        //  here we use whichever is clearer contextually.)
        let should = msg![env; vc shouldAutorotateToInterfaceOrientation:orientation];
        log_dbg!(
            "[{:?} shouldAutorotateToInterfaceOrientation:{:?}] => {:?}",
            vc,
            orientation,
            should
        );
        if should {
            let interface_oriented = env.options.interface_oriented_screen_coordinates();
            // Launch rotation is synchronous. UIKit brackets the bounds
            // change with legacy rotation callbacks for visible controllers.
            // Keep the old orientation visible until willRotate returns.
            // https://developer.apple.com/documentation/uikit/uiviewcontroller
            let old_orientation = env
                .objc
                .borrow::<UIViewControllerHostObject>(vc)
                .interface_orientation
                .unwrap_or(1);
            let visible: bool = !msg![env; window isHidden] && !msg![env; view isHidden];
            let notify = visible && old_orientation != orientation;
            retain(env, vc);
            retain(env, window);
            retain(env, view);
            {
                let controller = env.objc.borrow_mut::<UIViewControllerHostObject>(vc);
                controller.interface_orientation = Some(old_orientation);
                controller.autorotation_in_progress = true;
            }
            if notify {
                () = msg![env; vc willRotateToInterfaceOrientation:orientation duration:0.0f64];
            }
            // willRotate may replace the controller's view. Rotate that view
            // and suppress a nested rotation initiated by its mounting.
            let current_view: id = msg![env; vc view];
            retain(env, current_view);
            release(env, view);
            let view = current_view;
            env.objc
                .borrow_mut::<UIViewControllerHostObject>(vc)
                .interface_orientation = Some(orientation);
            if matches!(
                rotation,
                crate::window::DeviceOrientation::LandscapeLeft
                    | crate::window::DeviceOrientation::LandscapeRight
            ) {
                if let Some(screen) = screen_bounds {
                    let frame: CGRect = msg![env; window frame];
                    // Window geometry remains in the screen's base coordinate
                    // space. A full-screen landscape rectangle supplied during
                    // launch must fit that screen before the controller's
                    // orientation transform is applied. Leave partial windows
                    // and windows positioned away from the screen origin alone.
                    // https://developer.apple.com/library/archive/documentation/WindowsViews/Conceptual/ViewPG_iPhoneOS/CreatingWindows/CreatingWindows.html
                    if frame.origin == screen.origin
                        && frame.size.width == screen.size.height
                        && frame.size.height == screen.size.width
                    {
                        () = msg![env; window setFrame:screen];
                    }
                }
            }

            log_dbg!(
                "App requested autorotation; applying orientation transform to view {:?}.",
                view
            );
            // In iOS 8+ the screen/window already use interface-oriented
            // coordinates; rotating the root again swaps its bounds twice.
            // https://developer.apple.com/documentation/uikit/uicoordinatespace
            let transform = if interface_oriented {
                CGAffineTransformIdentity
            } else {
                match orientation {
                    UIInterfaceOrientationLandscapeLeft => {
                        CGAffineTransform::make_rotation(-std::f32::consts::FRAC_PI_2)
                    }
                    UIInterfaceOrientationLandscapeRight => {
                        CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_2)
                    }
                    UIInterfaceOrientationPortraitUpsideDown => {
                        CGAffineTransform::make_rotation(std::f32::consts::PI)
                    }
                    other => {
                        // Fall back to identity for an unexpected orientation.
                        log!(
                        "Warning: UIWindow autorotation: unsupported interface orientation {}; using identity transform.",
                        other
                    );
                        CGAffineTransformIdentity
                    }
                }
            };

            let window_frame: CGRect = msg![env; window frame];
            log_dbg!("Window frame: {window_frame:?}");
            let view_frame: CGRect = msg![env; view frame];
            log_dbg!("Old view frame: {view_frame:?}");

            () = msg![env; view setTransform:transform];
            env.framework_state.uikit.autorotation_transform_applied = true;

            // Keep the transformed root view filling its window; setFrame:
            // derives the corresponding bounds in the rotated local space.

            let view_frame: CGRect = msg![env; view frame];
            log_dbg!("Old view frame after transform: {view_frame:?}");

            () = msg![env; view setFrame:window_frame];

            let view_frame: CGRect = msg![env; view frame];
            log_dbg!("New view frame after re-applying old view frame: {view_frame:?}");

            if notify {
                () = msg![env; vc willAnimateRotationToInterfaceOrientation:orientation duration:0.0f64];
            }

            // The rotation swapped the view's bounds to landscape. UIKit lays
            // the root view controller's view out again when it rotates, and
            // apps depend on it: SDL 2's GL view recreates its renderbuffer
            // from the new layer bounds in -layoutSubviews, and its view
            // controller reports the new size to the game in
            // -viewDidLayoutSubviews. Without this, Costume Quest kept a
            // portrait renderbuffer and squeezed its landscape scene into it.
            () = msg![env; vc viewWillLayoutSubviews];
            () = msg![env; view layoutSubviews];
            () = msg![env; vc viewDidLayoutSubviews];
            if notify {
                () = msg![env; vc didRotateFromInterfaceOrientation:old_orientation];
            }
            env.objc
                .borrow_mut::<UIViewControllerHostObject>(vc)
                .autorotation_in_progress = false;
            release(env, view);
            release(env, window);
            release(env, vc);
        }
    }
}

#[cfg(test)]
mod launch_rotation_tests {
    use super::*;
    use crate::frameworks::core_graphics::CGSize;
    use crate::window::DeviceOrientation;

    #[test]
    fn modern_screen_coordinates_keep_root_unrotated_and_landscape() {
        // Since iOS 8, window/screen coordinates follow the interface. The
        // root must not receive the old portrait-screen rotation as well.
        // https://developer.apple.com/documentation/uikit/uicoordinatespace
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        env.options.ios_version = Some((8, 0, 0));
        env.options.linked_sdk_version = Some((8, 0, 0));
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let landscape = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 480.0,
                height: 320.0,
            },
        };
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:landscape];
        let controller: id = msg_class![env; UIViewController new];
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:landscape];
        () = msg![env; controller setView:root];
        () = msg![env; window addSubview:root];
        autorotate_view(
            env,
            window,
            root,
            DeviceOrientation::LandscapeLeft,
            Some(landscape),
        );
        let bounds: CGRect = msg![env; root bounds];
        let transform: CGAffineTransform = msg![env; root transform];
        assert_eq!(bounds, landscape);
        assert_eq!(transform, CGAffineTransformIdentity);
        release(env, controller);
        release(env, root);
        release(env, window);
        release(env, pool);
    }

    thread_local! {
        static ROTATION_EVENTS: std::cell::RefCell<Vec<(u8, i32, i32, CGSize, f64)>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static REPLACE_VIEW_ON_ROTATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn record_rotation(
        env: &mut crate::Environment,
        controller: id,
        stage: u8,
        orientation: i32,
        duration: f64,
    ) {
        let current: i32 = msg![env; controller interfaceOrientation];
        let view: id = msg![env; controller view];
        let bounds: CGRect = msg![env; view bounds];
        ROTATION_EVENTS.with(|events| {
            events
                .borrow_mut()
                .push((stage, orientation, current, bounds.size, duration));
        });
    }

    fn will_rotate(
        env: &mut crate::Environment,
        controller: id,
        _: crate::objc::SEL,
        orientation: i32,
        duration: f64,
    ) {
        record_rotation(env, controller, 0, orientation, duration);
        if REPLACE_VIEW_ON_ROTATION.with(|replace| replace.replace(false)) {
            let old_view: id = msg![env; controller view];
            let bounds: CGRect = msg![env; old_view bounds];
            let view: id = msg_class![env; UIView alloc];
            let view: id = msg![env; view initWithFrame:bounds];
            () = msg![env; controller setView:view];
            release(env, view);
        }
    }

    fn animate_rotation(
        env: &mut crate::Environment,
        controller: id,
        _: crate::objc::SEL,
        orientation: i32,
        duration: f64,
    ) {
        record_rotation(env, controller, 1, orientation, duration);
    }

    fn did_rotate(
        env: &mut crate::Environment,
        controller: id,
        _: crate::objc::SEL,
        orientation: i32,
    ) {
        record_rotation(env, controller, 2, orientation, 0.0);
    }

    #[test]
    fn rotation_callbacks_bracket_geometry_and_report_old_and_new_orientation() {
        // Apple specifies old orientation in willRotate, new orientation and
        // bounds in willAnimateRotation, then didRotate with the old argument.
        // Invisible controllers receive no rotation callbacks.
        // https://developer.apple.com/documentation/uikit/uiviewcontroller/willrotate(to:duration:)
        // https://developer.apple.com/documentation/uikit/uiviewcontroller/willanimaterotation(to:duration:)
        // https://developer.apple.com/documentation/uikit/uiviewcontroller/didrotate(from:)
        // https://developer.apple.com/documentation/uikit/uiviewcontroller
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        // The default compatibility policy must retain portrait-fixed screen
        // coordinates and rotate the root, even when UIDevice reports Latest.
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let class: crate::objc::Class = msg_class![env; UIViewController class];
        type RotationCallback = fn(&mut crate::Environment, id, crate::objc::SEL, i32, f64);
        type DidRotateCallback = fn(&mut crate::Environment, id, crate::objc::SEL, i32);
        let callbacks: [(&str, crate::dyld::HostFunction, &[u8]); 3] = [
            (
                "willRotateToInterfaceOrientation:duration:",
                &(will_rotate as RotationCallback),
                b"v@:id",
            ),
            (
                "willAnimateRotationToInterfaceOrientation:duration:",
                &(animate_rotation as RotationCallback),
                b"v@:id",
            ),
            (
                "didRotateFromInterfaceOrientation:",
                &(did_rotate as DidRotateCallback),
                b"v@:i",
            ),
        ];
        for (name, callback, signature) in callbacks {
            let selector = env
                .objc
                .register_host_selector(name.to_owned(), &mut env.mem);
            let imp =
                env.dyld
                    .create_guest_function(&mut env.mem, "[rotation test callback]", callback);
            let types = env.mem.alloc_and_write_cstr(signature);
            crate::objc::class_replaceMethod(
                env,
                class,
                selector,
                imp.to_ptr(),
                types.cast_const(),
            );
        }
        ROTATION_EVENTS.with(|events| events.borrow_mut().clear());
        let portrait = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 320.0,
                height: 480.0,
            },
        };
        let landscape = CGSize {
            width: 480.0,
            height: 320.0,
        };
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:portrait];
        let controller: id = msg_class![env; UIViewController new];
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:portrait];
        () = msg![env; controller setView:root];
        () = msg![env; window setRootViewController:controller];
        () = msg![env; window setHidden:false];
        autorotate_view(
            env,
            window,
            root,
            DeviceOrientation::LandscapeLeft,
            Some(portrait),
        );
        ROTATION_EVENTS.with(|events| {
            assert_eq!(
                *events.borrow(),
                vec![
                    (0, 3, 1, portrait.size, 0.0),
                    (1, 3, 3, landscape, 0.0),
                    (2, 1, 3, landscape, 0.0),
                ]
            )
        });
        // Reapplying the same orientation sizes a replacement view without
        // pretending that another interface rotation took place.
        ROTATION_EVENTS.with(|events| events.borrow_mut().clear());
        autorotate_view(
            env,
            window,
            root,
            DeviceOrientation::LandscapeLeft,
            Some(portrait),
        );
        ROTATION_EVENTS.with(|events| assert!(events.borrow().is_empty()));
        REPLACE_VIEW_ON_ROTATION.with(|replace| replace.set(true));
        autorotate_view(
            env,
            window,
            root,
            DeviceOrientation::LandscapeRight,
            Some(portrait),
        );
        ROTATION_EVENTS.with(|events| {
            assert_eq!(
                *events.borrow(),
                vec![
                    (0, 4, 3, landscape, 0.0),
                    (1, 4, 4, landscape, 0.0),
                    (2, 3, 4, landscape, 0.0),
                ]
            )
        });
        let replacement: id = msg![env; controller view];
        assert_ne!(replacement, root);
        let parent: id = msg![env; replacement superview];
        let old_parent: id = msg![env; root superview];
        assert_eq!(parent, window);
        assert_eq!(old_parent, nil);
        ROTATION_EVENTS.with(|events| events.borrow_mut().clear());
        () = msg![env; window setHidden:true];
        autorotate_view(
            env,
            window,
            replacement,
            DeviceOrientation::LandscapeLeft,
            Some(portrait),
        );
        let current: i32 = msg![env; controller interfaceOrientation];
        assert_eq!(current, 3);
        ROTATION_EVENTS.with(|events| assert!(events.borrow().is_empty()));
        () = msg![env; window setRootViewController:nil];
        release(env, controller);
        release(env, root);
        release(env, window);
        release(env, pool);
    }

    #[test]
    fn fullscreen_landscape_window_keeps_landscape_child_dimensions() {
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        // Exercise the portrait-fixed screen model used before iOS 8.
        env.options.ios_version = Some((7, 1, 2));
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let portrait = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 320.0,
                height: 480.0,
            },
        };
        let landscape = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 480.0,
                height: 320.0,
            },
        };
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:landscape];
        let controller: id = msg_class![env; UIViewController new];
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:landscape];
        () = msg![env; controller setView:root];
        () = msg![env; window addSubview:root];
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .defer_autorotation = true;
        queue_or_autorotate_view(env, window, root, Some(DeviceOrientation::LandscapeLeft));
        let child: id = msg_class![env; UIView alloc];
        let child: id = msg![env; child initWithFrame:landscape];
        () = msg![env; root addSubview:child];
        finish_launch_autorotation_with_rotation(
            env,
            Some(DeviceOrientation::LandscapeLeft),
            Some(portrait),
        );
        let frame: CGRect = msg![env; window frame];
        let bounds: CGRect = msg![env; root bounds];
        let child_frame: CGRect = msg![env; child frame];
        assert_eq!(frame, portrait);
        assert_eq!(bounds, landscape);
        assert_eq!(child_frame, landscape);
        release(env, child);
        release(env, controller);
        release(env, root);
        release(env, window);
        release(env, pool);
    }

    #[test]
    fn launch_rotation_waits_for_child_construction_and_releases_pending_views() {
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        // Exercise the portrait-fixed screen model used before iOS 8.
        env.options.ios_version = Some((7, 1, 2));
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let portrait = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 320.0,
                height: 480.0,
            },
        };
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:portrait];
        let controller: id = msg_class![env; UIViewController new];
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:portrait];
        () = msg![env; controller setView:root];
        // Exercise normal mounting/appearance first. There is no host window
        // in this environment, so mounting alone cannot initiate rotation.
        () = msg![env; window addSubview:root];
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .defer_autorotation = true;
        queue_or_autorotate_view(env, window, root, Some(DeviceOrientation::LandscapeLeft));
        let initial: CGRect = msg![env; root bounds];
        assert_eq!(initial, portrait, "rotation ran inside launch construction");
        let window_refs = env.objc.get_refcount(window).get();
        let root_refs = env.objc.get_refcount(root).get();
        let child: id = msg_class![env; UIView alloc];
        let child: id = msg![env; child initWithFrame:initial];
        () = msg![env; child setAutoresizingMask:(2u32 | 16u32)];
        () = msg![env; root addSubview:child];
        assert_eq!(
            env.framework_state
                .uikit
                .ui_view
                .ui_window
                .pending_autorotation
                .len(),
            1
        );

        finish_launch_autorotation_with_rotation(
            env,
            Some(DeviceOrientation::LandscapeLeft),
            Some(portrait),
        );
        let bounds: CGRect = msg![env; root bounds];
        let child_bounds: CGRect = msg![env; child bounds];
        assert_eq!(
            bounds.size,
            CGSize {
                width: 480.0,
                height: 320.0
            }
        );
        assert_eq!(child_bounds.size, bounds.size);
        assert_eq!(env.objc.get_refcount(window).get(), window_refs - 1);
        assert_eq!(env.objc.get_refcount(root).get(), root_refs - 1);
        assert!(env
            .framework_state
            .uikit
            .ui_view
            .ui_window
            .pending_autorotation
            .is_empty());

        // A view removed before the launch callback returns must not rotate.
        env.framework_state
            .uikit
            .ui_view
            .ui_window
            .defer_autorotation = true;
        () = msg![env; window addSubview:root];
        () = msg![env; root removeFromSuperview];
        finish_launch_autorotation_with_rotation(
            env,
            Some(DeviceOrientation::PortraitUpsideDown),
            Some(portrait),
        );
        let after_removal: CGRect = msg![env; root bounds];
        assert_eq!(after_removal, bounds);
        assert!(env
            .framework_state
            .uikit
            .ui_view
            .ui_window
            .pending_autorotation
            .is_empty());
        release(env, child);
        release(env, controller);
        release(env, root);
        release(env, window);
        release(env, pool);
    }
}
