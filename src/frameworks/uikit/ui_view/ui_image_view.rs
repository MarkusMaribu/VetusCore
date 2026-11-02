/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIImageView`.

use crate::frameworks::core_graphics::cg_image::CGImageRef;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::foundation::{NSInteger, NSTimeInterval, NSUInteger};
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_super, nil, objc_classes, release, retain,
    ClassExports, NSZonePtr,
};
use crate::Environment;
use std::time::Instant;

#[derive(Default)]
pub struct State {
    /// `UIImageView*` instances that are currently animating.
    /// These are retained while in this list.
    animating_views: Vec<id>,
}

#[derive(Default)]
struct UIImageViewHostObject {
    superclass: super::UIViewHostObject,
    /// `UIImage*`
    image: id,
    highlighted_image: id,
    highlighted: bool,
    /// `NSArray<UIImage *>*`
    animation_images: id,
    /// Total duration of one animation cycle. `0.0` means the default
    /// (number of frames at 30fps).
    animation_duration: NSTimeInterval,
    /// Number of times to repeat the animation. `0` means repeat forever.
    animation_repeat_count: NSInteger,
    /// Set while the view is animating.
    animation_start: Option<Instant>,
    /// Index of the animation frame currently displayed, to avoid redundant
    /// layer updates.
    current_frame: Option<NSUInteger>,
}
impl_HostObject_with_superclass!(UIImageViewHostObject);

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIImageView: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIImageViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    // Not sure if UIImageView does this unconditionally, or only for images
    // with alpha channels.
    () = msg![env; this setOpaque:false];
    // UIImageView disables user interaction by default (unlike plain UIView).
    // Apps routinely stack a decorative image view on top of the real tappable
    // control (e.g. Doodle Jump's menu buttons, or an image inside a UIButton):
    // if the image view swallowed touches, those controls would be dead.
    () = msg![env; this setUserInteractionEnabled:false];
    this
}

- (())dealloc {
    // Should never happen while the view is in the animating list (the list
    // retains it), but be careful anyway.
    let state = &mut env.framework_state.uikit.ui_view.ui_image_view;
    state.animating_views.retain(|&view| view != this);

    let &UIImageViewHostObject {
        superclass: _,
        image,
        highlighted_image,
        animation_images,
        ..
    } = env.objc.borrow(this);
    release(env, image);
    release(env, highlighted_image);
    release(env, animation_images);
    msg_super![env; this dealloc]
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let key_ns_string = get_static_str(env, "UIImage");
    let image: id = msg![env; coder decodeObjectForKey:key_ns_string];

    () = msg![env; this setImage:image];

    // UIView decoded the archived interaction setting. Image views may be
    // interactive containers in a nib; resetting the programmatic default here
    // prevents every control inside them from receiving touches.
    // https://developer.apple.com/documentation/uikit/uiimageview/isuserinteractionenabled
    // https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view/ui_image_view.rs

    this
}

- (id)initWithImage:(id)image { // UIImage*
    let size: CGSize = msg![env; image size];
    let frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size
    };
    let this = msg_super![env; this initWithFrame:frame];
    () = msg![env; this setImage:image];
    // Not sure if UIImageView does this unconditionally, or only for images
    // with alpha channels.
    () = msg![env; this setOpaque:false];
    // UIImageView disables user interaction by default (unlike plain UIView).
    () = msg![env; this setUserInteractionEnabled:false];
    this
}

- (id)image {
    env.objc.borrow::<UIImageViewHostObject>(this).image
}

- (id)initWithImage:(id)image highlightedImage:(id)highlighted_image {
    let this: id = msg![env; this initWithImage:image];
    () = msg![env; this setHighlightedImage:highlighted_image];
    this
}

- (id)highlightedImage {
    env.objc.borrow::<UIImageViewHostObject>(this).highlighted_image
}

- (())setHighlightedImage:(id)image {
    retain(env, image);
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<UIImageViewHostObject>(this).highlighted_image,
        image,
    );
    release(env, old);
    update_static_image(env, this);
}

- (bool)isHighlighted {
    env.objc.borrow::<UIImageViewHostObject>(this).highlighted
}

- (())setHighlighted:(bool)highlighted {
    env.objc.borrow_mut::<UIImageViewHostObject>(this).highlighted = highlighted;
    update_static_image(env, this);
}

- (())setImage:(id)new_image { // UIImage*
    let host_obj = env.objc.borrow_mut::<UIImageViewHostObject>(this);
    let old_image = std::mem::replace(&mut host_obj.image, new_image);
    retain(env, new_image);
    release(env, old_image);

    // While animating, the animation frames take precedence over the image.
    update_static_image(env, this);
}

- (id)animationImages {
    env.objc.borrow::<UIImageViewHostObject>(this).animation_images
}

- (())setAnimationImages:(id)images { // NSArray<UIImage *>*
    let host_obj = env.objc.borrow_mut::<UIImageViewHostObject>(this);
    let old_images = std::mem::replace(&mut host_obj.animation_images, images);
    retain(env, images);
    release(env, old_images);
}

- (NSTimeInterval)animationDuration {
    env.objc.borrow::<UIImageViewHostObject>(this).animation_duration
}

- (())setAnimationDuration:(NSTimeInterval)duration {
    env.objc.borrow_mut::<UIImageViewHostObject>(this).animation_duration = duration;
}

- (NSInteger)animationRepeatCount {
    env.objc.borrow::<UIImageViewHostObject>(this).animation_repeat_count
}

- (())setAnimationRepeatCount:(NSInteger)count {
    env.objc.borrow_mut::<UIImageViewHostObject>(this).animation_repeat_count = count;
}

- (bool)isAnimating {
    env.objc.borrow::<UIImageViewHostObject>(this).animation_start.is_some()
}

- (())startAnimating {
    let images = env.objc.borrow::<UIImageViewHostObject>(this).animation_images;
    if images == nil {
        return;
    }
    let image_count: NSUInteger = msg![env; images count];
    if image_count == 0 {
        return;
    }

    let host_obj = env.objc.borrow_mut::<UIImageViewHostObject>(this);
    if host_obj.animation_start.is_some() {
        return;
    }
    host_obj.animation_start = Some(Instant::now());
    host_obj.current_frame = None;

    retain(env, this);
    let state = &mut env.framework_state.uikit.ui_view.ui_image_view;
    state.animating_views.push(this);

    // Show the first frame immediately.
    update_animation(env, this);
}

- (())stopAnimating {
    let host_obj = env.objc.borrow_mut::<UIImageViewHostObject>(this);
    if host_obj.animation_start.take().is_none() {
        return;
    }
    host_obj.current_frame = None;

    let state = &mut env.framework_state.uikit.ui_view.ui_image_view;
    state.animating_views.retain(|&view| view != this);

    // Restore the appropriate static image.
    update_static_image(env, this);

    release(env, this);
}

// Chrome for iOS calls control-style methods on UIImageView instances that
// come out of its nibs. Tolerate them so the views behave as inert controls.

- (())setImage:(id)new_image forState:(NSUInteger)state {
    () = msg![env; this setImage:new_image];
}

- (())setEnabled:(bool)_enabled {
    // UIImageView is not a control; nothing to enable.
}

- (NSUInteger)state {
    0 // UIControlStateNormal
}

@end

};

fn update_static_image(env: &mut Environment, this: id) {
    let host = env.objc.borrow::<UIImageViewHostObject>(this);
    if host.animation_start.is_some() {
        return;
    }
    let image = if host.highlighted && host.highlighted_image != nil {
        host.highlighted_image
    } else {
        host.image
    };
    set_layer_contents(env, this, image);
}

fn set_layer_contents(env: &mut Environment, this: id, image: id) {
    let layer: id = msg![env; this layer];
    let cg_image: CGImageRef = if image == nil {
        CGImageRef::null()
    } else {
        msg![env; image CGImage]
    };
    () = msg![env; layer setContents:cg_image];
    let mut center = CGRect { origin: CGPoint::default(), size: CGSize { width: 1.0, height: 1.0 } };
    let scale: f32 = if image == nil { 1.0 } else { msg![env; image scale] };
    if image != nil {
        let size: CGSize = msg![env; image size];
        let left: NSInteger = msg![env; image leftCapWidth];
        let top: NSInteger = msg![env; image topCapHeight];
        if left > 0 && size.width > 0.0 {
            center.origin.x = left as f32 / size.width;
            center.size.width = 1.0 / size.width;
        }
        if top > 0 && size.height > 0.0 {
            center.origin.y = top as f32 / size.height;
            center.size.height = 1.0 / size.height;
        }
    }
    () = msg![env; layer setContentsScale:scale];
    () = msg![env; layer setContentsCenter:center];
}

/// Advance a single animating image view; stops the animation if it has
/// played for the requested number of repeats.
fn update_animation(env: &mut Environment, this: id) {
    let images = env
        .objc
        .borrow::<UIImageViewHostObject>(this)
        .animation_images;
    if images == nil {
        () = msg![env; this stopAnimating];
        return;
    }
    let image_count: NSUInteger = msg![env; images count];
    if image_count == 0 {
        () = msg![env; this stopAnimating];
        return;
    }

    let host_obj = env.objc.borrow::<UIImageViewHostObject>(this);
    let Some(start) = host_obj.animation_start else {
        return;
    };
    let duration = if host_obj.animation_duration > 0.0 {
        host_obj.animation_duration
    } else {
        // Default: number of images divided by 30 fps
        image_count as NSTimeInterval / 30.0
    };
    let repeat_count = host_obj.animation_repeat_count;

    let elapsed = start.elapsed().as_secs_f64();
    let cycles = elapsed / duration;
    if repeat_count > 0 && cycles >= repeat_count as f64 {
        () = msg![env; this stopAnimating];
        return;
    }

    let progress = (elapsed / duration).fract();
    let frame_idx = ((progress * image_count as f64) as NSUInteger).min(image_count - 1);

    if env.objc.borrow::<UIImageViewHostObject>(this).current_frame == Some(frame_idx) {
        return;
    }
    env.objc
        .borrow_mut::<UIImageViewHostObject>(this)
        .current_frame = Some(frame_idx);

    let frame: id = msg![env; images objectAtIndex:frame_idx];
    set_layer_contents(env, this, frame);
}

/// For use by the Core Animation compositor: advance all animating image
/// views. Call this before compositing a frame.
pub fn update_animations(env: &mut Environment) {
    let views = env
        .framework_state
        .uikit
        .ui_view
        .ui_image_view
        .animating_views
        .clone();
    for view in views {
        update_animation(env, view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighted_image_initialization_and_fallback_update_layer_contents() {
        // https://developer.apple.com/documentation/uikit/uiimageview/ishighlighted
        // https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view/ui_image_view.rs
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = crate::objc::msg_class![env; NSAutoreleasePool new];
        let normal_cg = crate::frameworks::core_graphics::cg_image::from_image(
            env,
            crate::image::Image::from_pixel_vec(vec![255; 8 * 12 * 4], (8, 12)),
        );
        let highlighted_cg = crate::frameworks::core_graphics::cg_image::from_image(
            env,
            crate::image::Image::from_pixel_vec(vec![128; 8 * 12 * 4], (8, 12)),
        );
        let normal: id = crate::objc::msg_class![env; UIImage imageWithCGImage:normal_cg];
        let highlighted: id = crate::objc::msg_class![env; UIImage imageWithCGImage:highlighted_cg];
        let view: id = crate::objc::msg_class![env; UIImageView alloc];
        let view: id = msg![env; view initWithImage:normal highlightedImage:highlighted];
        let frame: CGRect = msg![env; view frame];
        assert_eq!(
            frame.size,
            CGSize {
                width: 8.0,
                height: 12.0
            }
        );
        let layer: id = msg![env; view layer];
        for (state, expected) in [
            (false, normal_cg),
            (true, highlighted_cg),
            (false, normal_cg),
        ] {
            () = msg![env; view setHighlighted:state];
            let contents: CGImageRef = msg![env; layer contents];
            assert_eq!(contents, expected);
        }
        () = msg![env; view setHighlighted:true];
        () = msg![env; view setHighlightedImage:nil];
        let contents: CGImageRef = msg![env; layer contents];
        assert_eq!(contents, normal_cg);
        release(env, view);
        crate::frameworks::core_graphics::cg_image::CGImageRelease(env, normal_cg);
        crate::frameworks::core_graphics::cg_image::CGImageRelease(env, highlighted_cg);
        release(env, pool);
    }
}
