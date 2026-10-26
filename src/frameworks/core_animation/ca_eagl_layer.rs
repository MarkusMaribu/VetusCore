/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CAEAGLLayer`.

use super::ca_layer::CALayerHostObject;
use crate::frameworks::core_graphics::cg_affine_transform::{
    CGAffineTransform, CGAffineTransformIdentity,
};
use crate::frameworks::core_graphics::{CGPoint, CGRect};
use crate::frameworks::foundation::ns_string;
use crate::matrix::Matrix;
use crate::objc::{id, msg, msg_class, nil, objc_classes, release, Class, ClassExports, ObjC};
use crate::Environment;

// MARK: - EAGLDrawable property key constants
//
// These are the keys apps put in the drawableProperties dictionary.
// We export them as static strings so other modules can reference them.
pub const kEAGLDrawablePropertyRetainedBacking: &str = "RetainedBacking";
pub const kEAGLDrawablePropertyColorFormat: &str = "ColorFormat";

// kEAGLColorFormat values
pub const kEAGLColorFormatRGBA8: &str = "RGBA8";
pub const kEAGLColorFormatRGB565: &str = "RGB565";
pub const kEAGLColorFormatSRGBA8: &str = "SRGBA8";

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CAEAGLLayer: CALayer

// =========================================================================
// MARK: - EAGLDrawable protocol
// =========================================================================

- (id)drawableProperties { // NSDictionary<NSString*, id>*
    env.objc.borrow::<CALayerHostObject>(this).drawable_properties
}

- (())setDrawableProperties:(id)props { // NSDictionary<NSString*, id>*
    // Store a copy — matches Apple's behaviour.
    let old = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    let new_props: id = if props != nil { msg![env; props copy] } else { nil };
    release(env, old);
    env.objc.borrow_mut::<CALayerHostObject>(this).drawable_properties = new_props;

    // Log the properties the app is requesting so rendering issues are
    // easier to diagnose.
    if new_props != nil {
        let retained_key = ns_string::get_static_str(env, kEAGLDrawablePropertyRetainedBacking);
        let format_key    = ns_string::get_static_str(env, kEAGLDrawablePropertyColorFormat);

        let retained_val: id = msg![env; new_props objectForKey:retained_key];
        let format_val:   id = msg![env; new_props objectForKey:format_key];

        let retained_str = if retained_val != nil {
            let b: bool = msg![env; retained_val boolValue];
            if b { "YES" } else { "NO" }
        } else {
            "(not set)"
        };

        let format_str = if format_val != nil {
            ns_string::to_rust_string(env, format_val).into_owned()
        } else {
            "(not set)".to_string()
        };

        log_dbg!(
            "CAEAGLLayer setDrawableProperties: retainedBacking={} colorFormat={}",
            retained_str, format_str
        );
    }
}

// =========================================================================
// MARK: - Convenience helpers (read individual drawable properties)
// =========================================================================

// Returns YES if the backing store should be retained after presentation.
// Corresponds to kEAGLDrawablePropertyRetainedBacking.
- (bool)_touchHLE_retainedBacking {
    let props = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    if props == nil { return false; }
    let key: id = ns_string::get_static_str(env, kEAGLDrawablePropertyRetainedBacking);
    let val: id = msg![env; props objectForKey:key];
    if val == nil { return false; }
    msg![env; val boolValue]
}

// Returns the requested color format string, or "kEAGLColorFormatRGBA8"
// as the default if not specified.
- (id)_touchHLE_colorFormat { // NSString*
    let props = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    if props != nil {
        let key: id = ns_string::get_static_str(env, kEAGLDrawablePropertyColorFormat);
        let val: id = msg![env; props objectForKey:key];
        if val != nil { return val; }
    }
    ns_string::get_static_str(env, kEAGLColorFormatRGBA8)
}

// =========================================================================
// MARK: - CALayer overrides
// =========================================================================

// CAEAGLLayer is always opaque by default (unlike plain CALayer).
- (id)init {
    let _: () = msg![env; this setOpaque:true];
    this
}

// MARK: - Scale / Retina Support

// contentsScale is inherited from CALayer. EAGL-backed views retain the
// documented default of 1.0; apps opt into Retina with setContentScaleFactor:
// or setContentsScale:. Renderbuffer storage honors the selected scale.

- (id)initWithLayer:(id)layer {
    let _: () = msg![env; this setOpaque:true];
    // Copy drawable properties from the source layer if it is also a
    // CAEAGLLayer.
    let ca_eagl_class: Class = msg_class![env; CAEAGLLayer class];
    let is_eagl: bool = msg![env; layer isKindOfClass:ca_eagl_class];
    if is_eagl {
        let src_props = env.objc.borrow::<CALayerHostObject>(layer).drawable_properties;
        if src_props != nil {
            let copy: id = msg![env; src_props copy];
            env.objc.borrow_mut::<CALayerHostObject>(this).drawable_properties = copy;
        }
    }
    this
}

// Prevent the layer from being drawn by Core Animation — its contents are
// managed exclusively by EAGL/OpenGL ES.
- (())display {
    // No-op: the layer is presented via EAGLContext presentRenderBuffer:.
}

- (())drawInContext:(id)_ctx { // CGContextRef
    // No-op: CAEAGLLayer content comes from OpenGL ES, not Core Graphics.
}

// =========================================================================
// MARK: - Description
// =========================================================================

- (id)description {
    let host = env.objc.borrow::<CALayerHostObject>(this);
    let opaque = host.opaque;
    let has_props = host.drawable_properties != nil;
    let s = format!(
        "<CAEAGLLayer: {:?}; opaque={}; drawableProperties={}>",
        this,
        opaque,
        if has_props { "(set)" } else { "(nil)" }
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};

// Zero-area UIKit layers do not cover the fullscreen EAGL surface.
fn layer_has_visible_area(hidden: bool, opacity: f32, bounds: CGRect) -> bool {
    !hidden && opacity > 0.0 && bounds.size.width > 0.0 && bounds.size.height > 0.0
}

fn layer_subtree_may_draw_pixels(env: &Environment, layer: id, depth: usize) -> bool {
    const MAX_DEPTH: usize = 128;
    if depth >= MAX_DEPTH {
        return true;
    }
    let (hidden, opacity, bounds, sublayer_count) = {
        let host = env.objc.borrow::<CALayerHostObject>(layer);
        (host.hidden, host.opacity, host.bounds, host.sublayers.len())
    };
    if hidden || opacity <= 0.0 {
        return false;
    }
    if layer_has_visible_area(hidden, opacity, bounds) {
        return true;
    }
    for index in 0..sublayer_count {
        let child = env.objc.borrow::<CALayerHostObject>(layer).sublayers[index];
        if layer_subtree_may_draw_pixels(env, child, depth + 1) {
            return true;
        }
    }
    false
}

// =========================================================================
// MARK: - find_fullscreen_eagl_layer
// =========================================================================

/// Texture-coordinate transform equivalent to compositing this drawable's
/// current layer hierarchy and then presenting the composited screen.
///
/// Apple's CAEAGLLayer documentation specifies that layer effects also apply
/// to its OpenGL contents:
/// https://developer.apple.com/documentation/quartzcore/caeagllayer
/// Unlike a device-family heuristic or a global autorotation flag, this only
/// sees transforms on the drawable and its ancestors. A previous movie view
/// cannot change the orientation of an unrelated game layer.
pub fn drawable_present_rotation(
    objc: &ObjC,
    mut layer: id,
    device_rotation: Matrix<2>,
) -> Option<Matrix<2>> {
    let size = objc.borrow::<CALayerHostObject>(layer).bounds.size;
    let mut layer_to_screen = CGAffineTransformIdentity;
    while layer != nil {
        let host = objc.borrow::<CALayerHostObject>(layer);
        layer_to_screen = layer_to_screen.concat(host.superlayer_to_layer_transform());
        layer = host.superlayer;
    }
    presentation_texture_rotation(device_rotation, layer_to_screen, size)
}

fn presentation_texture_rotation(
    device_rotation: Matrix<2>,
    layer_to_screen: CGAffineTransform,
    size: crate::frameworks::core_graphics::CGSize,
) -> Option<Matrix<2>> {
    let CGAffineTransform { a, b, c, d, .. } = layer_to_screen;
    // Direct presentation fits the drawable's transformed frame to the host
    // viewport. Normalize in each coordinate space so a rotated rectangular
    // renderbuffer is not stretched by its portrait/landscape aspect ratio.
    let width = a.abs() * size.width + c.abs() * size.height;
    let height = b.abs() * size.width + d.abs() * size.height;
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return None;
    }
    let normalized = Matrix::from_columns([
        [a * size.width / width, b * size.width / height],
        [c * size.height / width, d * size.height / height],
    ]);
    let screen_to_layer = normalized.inverse()?;
    // CALayer geometry is top-down; renderbuffer texture coordinates are
    // bottom-up. Sample through the inverse layer transform between the two
    // Y flips, after the window's screen-to-display texture rotation.
    Some(
        device_rotation
            .multiply(&Matrix::y_flip())
            .multiply(&screen_to_layer)
            .multiply(&Matrix::y_flip()),
    )
}

/// Layer transforms that can still be presented directly to the host window.
/// The only non-identity transform UIKit applies to a fullscreen app view in
/// our compatibility layer is the device-orientation rotation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FullscreenLayerTransform {
    Identity,
    DeviceRotation,
}

fn nearly_equal(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

fn nearly_equal_transform(a: CGAffineTransform, b: CGAffineTransform) -> bool {
    let CGAffineTransform {
        a: aa,
        b: ab,
        c: ac,
        d: ad,
        tx: atx,
        ty: aty,
    } = a;
    let CGAffineTransform {
        a: ba,
        b: bb,
        c: bc,
        d: bd,
        tx: btx,
        ty: bty,
    } = b;
    const TOLERANCE: f32 = 1.0e-5;
    nearly_equal(aa, ba, TOLERANCE)
        && nearly_equal(ab, bb, TOLERANCE)
        && nearly_equal(ac, bc, TOLERANCE)
        && nearly_equal(ad, bd, TOLERANCE)
        && nearly_equal(atx, btx, TOLERANCE)
        && nearly_equal(aty, bty, TOLERANCE)
}

fn classify_fullscreen_layer_transform(
    transform: CGAffineTransform,
    orientation: crate::window::DeviceOrientation,
) -> Option<FullscreenLayerTransform> {
    if nearly_equal_transform(transform, CGAffineTransformIdentity) {
        return Some(FullscreenLayerTransform::Identity);
    }

    let angle = match orientation {
        crate::window::DeviceOrientation::Portrait => return None,
        crate::window::DeviceOrientation::PortraitUpsideDown => std::f32::consts::PI,
        crate::window::DeviceOrientation::LandscapeLeft => -std::f32::consts::FRAC_PI_2,
        crate::window::DeviceOrientation::LandscapeRight => std::f32::consts::FRAC_PI_2,
    };
    nearly_equal_transform(transform, CGAffineTransform::make_rotation(angle))
        .then_some(FullscreenLayerTransform::DeviceRotation)
}

fn fullscreen_frame_matches_screen(frame: CGRect, screen: CGRect) -> bool {
    let CGRect {
        origin: frame_origin,
        size: frame_size,
    } = frame;
    let CGRect {
        origin: screen_origin,
        size: screen_size,
    } = screen;
    // Rotating an integer-sized UIKit view can leave a few floating-point
    // ulps in `frame`; a hundredth of a point is far below a visible pixel.
    const TOLERANCE: f32 = 0.01;
    nearly_equal(frame_origin.x, screen_origin.x, TOLERANCE)
        && nearly_equal(frame_origin.y, screen_origin.y, TOLERANCE)
        && nearly_equal(frame_size.width, screen_size.width, TOLERANCE)
        && nearly_equal(frame_size.height, screen_size.height, TOLERANCE)
}

/// If there is an opaque `CAEAGLLayer` that covers the entire screen, this
/// returns a pointer to it. Otherwise, it returns [nil].
pub fn find_fullscreen_eagl_layer(env: &mut Environment) -> id {
    if env.options.force_composition || super::view_transition::has_active(env) {
        return nil;
    }

    let window_count = env.framework_state.uikit.ui_view.ui_window.windows.len();
    let mut top_window = None;
    for index in (0..window_count).rev() {
        let window = env.framework_state.uikit.ui_view.ui_window.windows[index];
        if msg![env; window isHidden] {
            continue;
        }
        let layer: id = msg![env; window layer];
        let bounds = env.objc.borrow::<CALayerHostObject>(layer).bounds;
        if bounds.size.width > 0.0 && bounds.size.height > 0.0 {
            top_window = Some(window);
            break;
        }
    }
    let Some(top_window) = top_window else {
        return nil;
    };

    let screen_bounds: CGRect = {
        let screen: id = msg_class![env; UIScreen mainScreen];
        msg![env; screen bounds]
    };

    let Some(orientation) = env.window.as_ref().map(|window| window.current_rotation()) else {
        return nil;
    };
    let mut layer: id = msg![env; top_window layer];
    // Accumulate each layer's local-to-superlayer transform so child EAGL
    // layers inside an autorotated root view are checked in screen space too.
    let mut parent_to_screen = CGAffineTransformIdentity;
    let mut saw_device_rotation = false;

    loop {
        // assert!(layer != nil);

        let (
            affine_transform,
            layer_bounds,
            superlayer_to_layer_transform,
            anchor_point,
            hidden,
            opacity,
        ) = {
            let host = env.objc.borrow::<CALayerHostObject>(layer);
            // Direct presentation would bypass the animated presentation transform.
            if !host.transform_3d.is_affine()
                || !host.animations.is_empty()
                || !host.anonymous_animations.is_empty()
            {
                return nil;
            }
            (
                host.affine_transform,
                host.bounds,
                host.superlayer_to_layer_transform(),
                host.anchor_point,
                host.hidden,
                host.opacity,
            )
        };
        let transform_kind = classify_fullscreen_layer_transform(affine_transform, orientation);
        let layer_to_screen = superlayer_to_layer_transform.concat(parent_to_screen);
        let layer_frame = layer_to_screen.apply_to_rect(CGRect {
            origin: layer_bounds.origin,
            size: layer_bounds.size,
        });

        // UIKit's iOS 5-style autorotation sets the root view's transform to
        // the device rotation, then resets its frame to the full-screen bounds.
        // Requiring an identity transform here made landscape EAGL views look
        // non-fullscreen, so `presentRenderbuffer:` fell back to glReadPixels
        // plus software Core Animation composition on every frame. Accept one
        // exact device-rotation transform, but only when the transformed frame
        // still covers the whole screen. The GPU presenter applies the
        // configured orientation compensation itself. Arbitrary transforms,
        // scaled views and nested/double rotations keep the safe compositor
        // path.
        let transform_is_usable = match transform_kind {
            Some(FullscreenLayerTransform::Identity) => true,
            Some(FullscreenLayerTransform::DeviceRotation) if !saw_device_rotation => {
                saw_device_rotation = true;
                true
            }
            _ => false,
        };
        if !fullscreen_frame_matches_screen(layer_frame, screen_bounds)
            || layer_bounds.origin != (CGPoint { x: 0.0, y: 0.0 })
            || anchor_point != (CGPoint { x: 0.5, y: 0.5 })
            || hidden
            || opacity != 1.0
            || !transform_is_usable
        {
            if crate::env_flag_cached!("TOUCHHLE_TRACE_PRESENT") {
                log!(
                    "FULLSCREEN-DIAG layer {:?} rejected: frame={:?} screen={:?} frame_match={} bounds_origin={:?} anchor={:?} hidden={} opacity={} transform_kind={:?} transform_usable={}",
                    layer,
                    layer_frame,
                    screen_bounds,
                    fullscreen_frame_matches_screen(layer_frame, screen_bounds),
                    layer_bounds.origin,
                    anchor_point,
                    hidden,
                    opacity,
                    transform_kind,
                    transform_is_usable
                );
            }
            return nil;
        }

        let sublayers = env
            .objc
            .borrow::<CALayerHostObject>(layer)
            .sublayers
            .clone();
        parent_to_screen = layer_to_screen;
        let eagl_class: Class = msg_class![env; CAEAGLLayer class];
        let mut fallback = None;
        let mut eagl_pick = None;
        for &candidate in sublayers.iter().rev() {
            if !layer_subtree_may_draw_pixels(env, candidate, 0) {
                continue;
            }
            if fallback.is_none() {
                fallback = Some(candidate);
            }
            let bounds = env.objc.borrow::<CALayerHostObject>(candidate).bounds;
            if bounds.size.width > 0.0
                && bounds.size.height > 0.0
                && msg![env; candidate isKindOfClass:eagl_class]
            {
                eagl_pick = Some(candidate);
                break;
            }
        }
        if let Some(next) = eagl_pick.or(fallback) {
            layer = next;
        } else {
            break;
        }
    }

    if !env.objc.borrow::<CALayerHostObject>(layer).opaque {
        if crate::env_flag_cached!("TOUCHHLE_TRACE_PRESENT") {
            log!("FULLSCREEN-DIAG layer {:?} rejected: not opaque", layer);
        }
        return nil;
    }

    let ca_eagl_layer_class: Class = msg_class![env; CAEAGLLayer class];
    if !msg![env; layer isKindOfClass:ca_eagl_layer_class] {
        if crate::env_flag_cached!("TOUCHHLE_TRACE_PRESENT") {
            log!(
                "FULLSCREEN-DIAG layer {:?} rejected: not a CAEAGLLayer",
                layer
            );
        }
        return nil;
    }

    layer
}

#[cfg(test)]
mod presentation_rotation_tests {
    use super::*;
    use crate::frameworks::core_graphics::CGSize;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn assert_maps(rotation: Matrix<2>, point: [f32; 2], expected: [f32; 2]) {
        let actual = rotation.transform(point);
        for i in 0..2 {
            assert!(
                (actual[i] - expected[i]).abs() < 1.0e-5,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn untransformed_drawable_keeps_device_rotation_in_both_landscape_directions() {
        let size = CGSize {
            width: 768.0,
            height: 1024.0,
        };
        for (angle, expected) in [(FRAC_PI_2, [0.0, 1.0]), (-FRAC_PI_2, [0.0, -1.0])] {
            let rotation = presentation_texture_rotation(
                Matrix::z_rotation(angle),
                CGAffineTransformIdentity,
                size,
            )
            .unwrap();
            assert_maps(rotation, [1.0, 0.0], expected);
        }
    }

    #[test]
    fn autorotated_landscape_renderbuffer_matches_composition() {
        // CAEAGLLayer contents participate in the layer hierarchy:
        // https://developer.apple.com/documentation/quartzcore/caeagllayer
        // A landscape layer rotated into a portrait UIKit screen must cancel
        // the window's quarter-turn, not receive a guessed extra half-turn.
        let size = CGSize {
            width: 1024.0,
            height: 768.0,
        };
        for angle in [FRAC_PI_2, -FRAC_PI_2, PI] {
            let rotation = presentation_texture_rotation(
                Matrix::z_rotation(angle),
                CGAffineTransform::make_rotation(-angle),
                size,
            )
            .unwrap();
            for corner in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
                assert_maps(rotation, corner, corner);
            }
        }
    }

    #[test]
    fn drawable_ancestors_determine_rotation_and_movie_siblings_do_not() {
        let mut mem = crate::mem::Mem::new();
        mem.set_null_segment_size(crate::mem::PAGE_SIZE);
        let mut objc = ObjC::new();
        let bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 1024.0,
                height: 768.0,
            },
        };
        let mut alloc_layer = |superlayer, affine_transform| {
            let mut host = CALayerHostObject::default();
            host.bounds = bounds;
            host.superlayer = superlayer;
            host.affine_transform = affine_transform;
            objc.alloc_static_object(nil, Box::new(host), &mut mem)
        };
        let root = alloc_layer(nil, CGAffineTransformIdentity);
        let drawable = alloc_layer(root, CGAffineTransformIdentity);
        let _movie = alloc_layer(root, CGAffineTransform::make_rotation(-FRAC_PI_2));
        let device = Matrix::z_rotation(FRAC_PI_2);
        assert_maps(
            drawable_present_rotation(&objc, drawable, device).unwrap(),
            [1.0, 0.0],
            [0.0, 1.0],
        );
        // The same EAGL child now inherits its parent's live rotation.
        objc.borrow_mut::<CALayerHostObject>(root).affine_transform =
            CGAffineTransform::make_rotation(-FRAC_PI_2);
        assert_maps(
            drawable_present_rotation(&objc, drawable, device).unwrap(),
            [1.0, 0.0],
            [1.0, 0.0],
        );
        // Removing that parent also removes its rotation; no sticky flag.
        objc.borrow_mut::<CALayerHostObject>(drawable).superlayer = nil;
        assert_maps(
            drawable_present_rotation(&objc, drawable, device).unwrap(),
            [1.0, 0.0],
            [0.0, 1.0],
        );
    }

    #[test]
    fn portrait_and_upside_down_frames_do_not_get_an_ipad_half_turn() {
        let size = CGSize {
            width: 320.0,
            height: 480.0,
        };
        for (angle, expected) in [(0.0, [1.0, 0.0]), (PI, [-1.0, 0.0])] {
            assert_maps(
                presentation_texture_rotation(
                    Matrix::z_rotation(angle),
                    CGAffineTransformIdentity,
                    size,
                )
                .unwrap(),
                [1.0, 0.0],
                expected,
            );
        }
        assert!(presentation_texture_rotation(
            Matrix::identity(),
            CGAffineTransform::make_scale(0.0, 1.0),
            size,
        )
        .is_none());
    }
}

#[cfg(test)]
mod fullscreen_layer_tests {
    use super::*;
    use crate::window::DeviceOrientation;

    #[test]
    fn accepts_identity_and_the_current_device_rotation_only() {
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransformIdentity,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::Identity),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(-std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeRight,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
    }

    #[test]
    fn rejects_transforms_that_change_fullscreen_coverage() {
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_4),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_scale(0.9, 1.0),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_translation(1.0, 0.0),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
    }

    #[test]
    fn autorotated_child_eagl_layer_covers_a_portrait_uikit_screen() {
        let screen = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.0,
                height: 568.0,
            },
        };
        let mut window_layer = CALayerHostObject::default();
        window_layer.bounds = screen;
        window_layer.position = CGPoint { x: 160.0, y: 284.0 };
        window_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        window_layer.affine_transform = CGAffineTransformIdentity;
        let window_to_screen = window_layer.superlayer_to_layer_transform();

        let mut root_view_layer = CALayerHostObject::default();
        root_view_layer.bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 568.0,
                height: 320.0,
            },
        };
        root_view_layer.position = CGPoint { x: 160.0, y: 284.0 };
        root_view_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        root_view_layer.affine_transform =
            CGAffineTransform::make_rotation(-std::f32::consts::FRAC_PI_2);
        let root_view_to_screen = root_view_layer
            .superlayer_to_layer_transform()
            .concat(window_to_screen);

        let mut eagl_layer = CALayerHostObject::default();
        eagl_layer.bounds = root_view_layer.bounds;
        eagl_layer.position = CGPoint { x: 284.0, y: 160.0 };
        eagl_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        eagl_layer.affine_transform = CGAffineTransformIdentity;
        let eagl_to_parent = eagl_layer.superlayer_to_layer_transform();
        let eagl_to_screen = eagl_to_parent.concat(root_view_to_screen);
        let eagl_bounds = eagl_layer.bounds;
        let eagl_bounds_rect = CGRect {
            origin: eagl_bounds.origin,
            size: eagl_bounds.size,
        };
        let local_frame = eagl_to_parent.apply_to_rect(eagl_bounds_rect);
        let screen_frame = eagl_to_screen.apply_to_rect(eagl_bounds_rect);

        assert_eq!(
            classify_fullscreen_layer_transform(
                root_view_layer.affine_transform,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                eagl_layer.affine_transform,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::Identity),
        );
        assert!(!fullscreen_frame_matches_screen(local_frame, screen));
        assert!(fullscreen_frame_matches_screen(screen_frame, screen));
    }

    #[test]
    fn rejects_frames_that_do_not_cover_the_screen() {
        let screen = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.0,
                height: 568.0,
            },
        };
        let almost_fullscreen = CGRect {
            origin: CGPoint {
                x: -0.002,
                y: 0.003,
            },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.001,
                height: 567.999,
            },
        };
        let partial = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 319.0,
                height: 568.0,
            },
        };

        assert!(fullscreen_frame_matches_screen(almost_fullscreen, screen));
        assert!(!fullscreen_frame_matches_screen(partial, screen));
    }
}

#[cfg(test)]
mod layer_visibility_tests {
    use super::*;

    #[test]
    fn treats_zero_sized_and_invisible_layers_as_non_occluding() {
        let no_bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 0.0,
                height: 480.0,
            },
        };
        let fullscreen = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.0,
                height: 480.0,
            },
        };

        assert!(!layer_has_visible_area(false, 1.0, no_bounds));
        assert!(!layer_has_visible_area(true, 1.0, fullscreen));
        assert!(!layer_has_visible_area(false, 0.0, fullscreen));
        assert!(layer_has_visible_area(false, 1.0, fullscreen));
    }
}

// =========================================================================
// MARK: - Pixel buffer helpers (used by EAGLContext)
// =========================================================================

/// Takes the pixel buffer out of the layer so it can be refilled by
/// `EAGLContext presentRenderBuffer:`. Pass the buffer back via
/// [present_pixels] once it is filled.
pub fn get_pixels_vec_for_presenting(env: &mut Environment, layer: id) -> Vec<u8> {
    env.objc
        .borrow_mut::<CALayerHostObject>(layer)
        .presented_pixels
        .take()
        .map(|(vec, _w, _h)| vec)
        .unwrap_or_default()
}

/// Clears pixels previously presented by a movie player view.
pub fn clear_presented_pixels(env: &mut Environment, layer: id) {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(layer);
    host_obj.presented_pixels = None;
    host_obj.presented_directly = false;
    host_obj.gles_texture_is_up_to_date = false;
}

/// Records that `EAGLContext presentRenderbuffer:` presented this layer's
/// drawable directly to the window, bypassing `presented_pixels`.
pub fn mark_presented_directly(env: &mut Environment, layer: id) {
    let host = env.objc.borrow_mut::<CALayerHostObject>(layer);
    // Retire the old readback when presentation changes back to the GPU.
    // Otherwise run-loop composition keeps swapping the cached frame over
    // the newer direct frames, including stale text and control state.
    host.presented_pixels = None;
    host.presented_directly = true;
}

/// Whether the drawable is already on screen without Core Animation.
pub(super) fn is_presented_directly(objc: &ObjC, layer: id) -> bool {
    let host = objc.borrow::<CALayerHostObject>(layer);
    host.presented_directly && host.presented_pixels.is_none()
}

#[cfg(test)]
mod cached_frame_tests {
    use super::*;
    use crate::frameworks::core_graphics::{cg_bitmap_context, CGSize};
    use crate::objc::retain;

    // CAEAGLLayer contents still participate in Core Animation composition.
    // A cached GL frame cannot suppress later UIKit updates (Doodle Jump's
    // game-over name field), even after a direct-to-readback fallback.
    // https://developer.apple.com/documentation/quartzcore/caeagllayer
    #[test]
    fn cached_gl_frame_keeps_text_field_updates_visible() {
        let mut env = crate::environment::touch_test_environment();
        let env = &mut env;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let drawable: id = msg_class![env; CAEAGLLayer layer];
        let field: id = msg_class![env; UITextField alloc];
        let field: id = msg![env; field initWithFrame:(CGRect {
            origin: CGPoint::default(),
            size: CGSize { width: 132.0, height: 26.0 },
        })];
        let subviews: id = msg![env; field subviews];
        let label: id = msg![env; subviews objectAtIndex:0u32];
        let layer: id = msg![env; label layer];
        retain(env, drawable);
        mark_presented_directly(env, drawable);
        present_pixels(env, drawable, vec![255; 16], 2, 2);
        crate::frameworks::uikit::ui_responder::handle_host_edit(env, field, "Test".into());
        let _: () = msg![env; field layoutIfNeeded];
        assert!(!is_presented_directly(&env.objc, drawable),
            "a cached frame must keep the compositor delivering UI edits");
        super::super::composition::display_layers(env, layer);
        let context = env.objc.borrow::<CALayerHostObject>(layer).cg_context
            .expect("the text label should have been rasterized");
        let (width, height, data) = cg_bitmap_context::get_data(&env.objc, context);
        let pixels = env.mem.bytes_at(data.cast(), width * height * 4);
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        mark_presented_directly(env, drawable);
        assert!(is_presented_directly(&env.objc, drawable),
            "returning to GPU presentation must retire the cached readback frame");
        release(env, drawable);
        release(env, field);
        release(env, pool);
    }
}

/// Stores the new rendered frame in the layer and marks the GLES texture as
/// stale. Data must be in RGBA8 format.
pub fn present_pixels(env: &mut Environment, layer: id, pixels: Vec<u8>, width: u32, height: u32) {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(layer);
    host_obj.presented_pixels = Some((pixels, width, height));
    host_obj.gles_texture_is_up_to_date = false;
}

/// Returns whether the layer's backing store should be retained after
/// presentation (i.e. `kEAGLDrawablePropertyRetainedBacking` is YES).
/// Convenience wrapper for use by `EAGLContext`.
pub fn is_retained_backing(env: &mut Environment, layer: id) -> bool {
    msg![env; layer _touchHLE_retainedBacking]
}

/// Returns the drawable's pixel width (from the presented pixel buffer if
/// available, otherwise from the layer's bounds).
pub fn drawable_width(env: &mut Environment, layer: id) -> u32 {
    let host = env.objc.borrow::<CALayerHostObject>(layer);
    if let Some((_, w, _)) = host.presented_pixels {
        return w;
    }
    host.bounds.size.width as u32
}

/// Returns the drawable's pixel height.
pub fn drawable_height(env: &mut Environment, layer: id) -> u32 {
    let host = env.objc.borrow::<CALayerHostObject>(layer);
    if let Some((_, _, h)) = host.presented_pixels {
        return h;
    }
    host.bounds.size.height as u32
}
