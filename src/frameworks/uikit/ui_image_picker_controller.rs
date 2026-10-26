/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#![allow(dead_code)]
//! `UIImagePickerController`

use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::foundation::ns_string::{self, get_static_str, to_rust_string};
use crate::frameworks::foundation::NSInteger;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::core_graphics::cg_affine_transform::{CGAffineTransform, CGAffineTransformIdentity};
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes,
    release, retain, ClassExports, NSZonePtr,
};
use crate::Environment;
use std::time::Instant;
#[cfg(feature = "embedded")]
use std::time::Duration;

type UIImagePickerControllerSourceType = NSInteger;
type UIImagePickerControllerQualityType = NSInteger;
type UIImagePickerControllerCameraCaptureMode = NSInteger;
type UIImagePickerControllerCameraDevice = NSInteger;
type UIImagePickerControllerCameraFlashMode = NSInteger;

// UIImagePickerControllerSourceType values
const UIImagePickerControllerSourceTypePhotoLibrary: NSInteger = 0;
const UIImagePickerControllerSourceTypeCamera: NSInteger = 1;
const UIImagePickerControllerSourceTypeSavedPhotosAlbum: NSInteger = 2;

// UIImagePickerControllerQualityType values
const UIImagePickerControllerQualityTypeHigh: NSInteger = 0;
const UIImagePickerControllerQualityTypeMedium: NSInteger = 1;
const UIImagePickerControllerQualityTypeLow: NSInteger = 2;

// UIImagePickerControllerCameraDevice values
const UIImagePickerControllerCameraDeviceRear: NSInteger = 0;
const UIImagePickerControllerCameraDeviceFront: NSInteger = 1;

// UIImagePickerControllerCameraCaptureMode values
const UIImagePickerControllerCameraCaptureModePhoto: UIImagePickerControllerCameraCaptureMode = 0;
const UIImagePickerControllerCameraCaptureModeVideo: UIImagePickerControllerCameraCaptureMode = 1;


// UIImagePickerControllerCameraFlashMode values
const UIImagePickerControllerCameraFlashModeOff: NSInteger = -1;
const UIImagePickerControllerCameraFlashModeAuto: NSInteger = 0;
const UIImagePickerControllerCameraFlashModeOn: NSInteger = 1;

#[derive(Default)]
struct UIImagePickerControllerHostObject {
    superclass: super::ui_view_controller::ui_navigation_controller::UINavigationControllerHostObject,
    source_type: UIImagePickerControllerSourceType,
    media_types: id, // NSArray* of NSString*
    allows_editing: bool,
    video_quality: UIImagePickerControllerQualityType,
    camera_device: UIImagePickerControllerCameraDevice,
    camera_flash_mode: UIImagePickerControllerCameraFlashMode,
    camera_capture_mode: UIImagePickerControllerCameraCaptureMode,
    shows_camera_controls: bool,
    camera_overlay_view: id,
    camera_view_transform: CGAffineTransform,
    preview_view: id,
    controls_view: id,
}
impl_HostObject_with_superclass!(UIImagePickerControllerHostObject);

#[derive(Default)]
pub struct State {
    pending: Option<(id, u32)>,
    next_request: u32,
    camera_picker: Option<id>,
    capture: Option<(id, u32)>,
    capture_result: Option<Option<Vec<u8>>>,
    last_preview: Option<Instant>,
}

fn decoded_image(env: &mut Environment, bytes: &[u8]) -> id {
    let ptr = env.mem.alloc(bytes.len() as u32);
    env.mem.bytes_at_mut(ptr.cast(), bytes.len() as u32).copy_from_slice(bytes);
    let data: id = msg_class![env; NSData dataWithBytes:ptr length:(bytes.len() as u32)];
    env.mem.free(ptr.cast_void());
    msg_class![env; UIImage imageWithData:data]
}

/// The preview is a sibling below the default controls and custom overlay.
/// cameraViewTransform applies to its pixels, never to the overlay hierarchy.
/// https://developer.apple.com/documentation/uikit/uiimagepickercontroller/cameraoverlayview
/// https://developer.apple.com/documentation/uikit/uiimagepickercontroller/cameraviewtransform
fn configure_camera_views(env: &mut Environment, picker: id, root: id) {
    let (source, mut preview, mut controls, overlay, transform, show) = {
        let h = env.objc.borrow::<UIImagePickerControllerHostObject>(picker);
        (h.source_type, h.preview_view, h.controls_view, h.camera_overlay_view,
            h.camera_view_transform, h.shows_camera_controls)
    };
    if source != UIImagePickerControllerSourceTypeCamera { return; }
    let bounds: CGRect = msg![env; root bounds];
    if preview == nil {
        preview = msg_class![env; UIImageView alloc];
        preview = msg![env; preview initWithFrame:bounds];
        () = msg![env; preview setContentMode:(2i32)]; // UIViewContentModeScaleAspectFill
        () = msg![env; preview setClipsToBounds:true];
        () = msg![env; preview setAutoresizingMask:(2u32 | 16u32)];
        env.objc.borrow_mut::<UIImagePickerControllerHostObject>(picker).preview_view = preview;
    }
    () = msg![env; preview setTransform:CGAffineTransformIdentity];
    () = msg![env; preview setFrame:bounds];
    () = msg![env; preview setTransform:transform];
    () = msg![env; root addSubview:preview];
    () = msg![env; root sendSubviewToBack:preview];
    if controls == nil {
        controls = msg_class![env; UIView alloc];
        let frame = CGRect { origin: CGPoint { x: 0.0, y: (bounds.size.height - 60.0).max(0.0) },
            size: CGSize { width: bounds.size.width, height: 60.0 } };
        controls = msg![env; controls initWithFrame:frame];
        () = msg![env; controls setAutoresizingMask:(2u32 | 8u32)];
        let black: id = msg_class![env; UIColor blackColor];
        () = msg![env; controls setBackgroundColor:black];
        for (label, selector, x, width) in [
            ("Cancel", "_touchHLE_cameraCancel:", 8.0, 80.0),
            ("Take Photo", "_touchHLE_cameraShutter:", (bounds.size.width - 150.0).max(90.0), 140.0),
        ] {
            let button: id = msg_class![env; UIButton buttonWithType:(1i32)];
            let frame = CGRect { origin: CGPoint { x, y: 8.0 }, size: CGSize { width, height: 44.0 } };
            () = msg![env; button setFrame:frame];
            let title = get_static_str(env, label);
            () = msg![env; button setTitle:title forState:(0u32)];
            let selector = env.objc.register_host_selector(selector.to_string(), &mut env.mem);
            () = msg![env; button addTarget:picker action:selector forControlEvents:(1u32 << 6)];
            () = msg![env; controls addSubview:button];
        }
        env.objc.borrow_mut::<UIImagePickerControllerHostObject>(picker).controls_view = controls;
    }
    () = msg![env; controls setHidden:(!show)];
    () = msg![env; root addSubview:controls];
    if overlay != nil { () = msg![env; root addSubview:overlay]; }
}

fn update_camera_views_if_loaded(env: &mut Environment, picker: id) {
    let loaded: bool = msg![env; picker isViewLoaded];
    if loaded {
        let root: id = msg![env; picker view];
        configure_camera_views(env, picker, root);
    }
}

fn start_camera_preview(env: &mut Environment, picker: id) {
    #[cfg(feature = "embedded")]
    {
        let (front, flash) = {
            let h = env.objc.borrow::<UIImagePickerControllerHostObject>(picker);
            (h.camera_device == UIImagePickerControllerCameraDeviceFront, h.camera_flash_mode)
        };
        crate::embed::start_camera(front, flash);
    }
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.camera_picker == Some(picker) { return; }
    let old = state.camera_picker.replace(picker);
    state.last_preview = None;
    retain(env, picker);
    if let Some(old) = old { release(env, old); }
}

fn stop_camera_preview(env: &mut Environment, picker: id) {
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.camera_picker != Some(picker) { return; }
    state.camera_picker = None;
    state.last_preview = None;
    #[cfg(feature = "embedded")]
    crate::embed::stop_camera();
    release(env, picker);
}

fn source_available(source: NSInteger, camera: bool, library: bool) -> bool {
    match source {
        UIImagePickerControllerSourceTypeCamera => camera,
        UIImagePickerControllerSourceTypePhotoLibrary |
        UIImagePickerControllerSourceTypeSavedPhotosAlbum => library,
        _ => false,
    }
}

fn library_available() -> bool {
    // A host selection bridge is currently provided by the embedded host.
    cfg!(feature = "embedded")
}

fn responds(env: &mut Environment, target: id, name: &str) -> bool {
    let selector = env.objc.register_host_selector(name.to_string(), &mut env.mem);
    target != nil && msg![env; target respondsToSelector:selector]
}

fn cancel(env: &mut Environment, picker: id, delegate: id) {
    if responds(env, delegate, "imagePickerControllerDidCancel:") {
        () = msg![env; delegate imagePickerControllerDidCancel:picker];
    }
}

/// Deliver on the guest main run loop, with the picker alive through callbacks.
/// Apple documents the modern callback taking priority over the deprecated one:
/// https://developer.apple.com/documentation/uikit/uiimagepickercontrollerdelegate
fn deliver_image(env: &mut Environment, picker: id, bytes: &[u8]) {
    let delegate = env.objc.borrow::<UIImagePickerControllerHostObject>(picker).superclass.delegate;
    retain(env, delegate);
    let image = decoded_image(env, bytes);
    if image == nil {
        log!("UIImagePickerController: host image could not be decoded");
        cancel(env, picker, delegate);
    } else {
        let info: id = msg_class![env; NSMutableDictionary dictionary];
        let key = get_static_str(env, "UIImagePickerControllerOriginalImage");
        () = msg![env; info setObject:image forKey:key];
        let key = get_static_str(env, "UIImagePickerControllerMediaType");
        let media_type = get_static_str(env, "public.image");
        () = msg![env; info setObject:media_type forKey:key];
        if responds(env, delegate, "imagePickerController:didFinishPickingMediaWithInfo:") {
            log!("UIImagePickerController: delivering image via didFinishPickingMediaWithInfo");
            () = msg![env; delegate imagePickerController:picker didFinishPickingMediaWithInfo:info];
        } else if responds(env, delegate, "imagePickerController:didFinishPickingImage:editingInfo:") {
            log!("UIImagePickerController: delivering image via didFinishPickingImage");
            () = msg![env; delegate imagePickerController:picker didFinishPickingImage:image editingInfo:nil];
        }
    }
    release(env, delegate);
}

fn poll_library(env: &mut Environment) -> bool {
    #[cfg(feature = "embedded")]
    let result = crate::embed::take_photo_picker_result();
    #[cfg(not(feature = "embedded"))]
    let result: Option<(u32, Option<Vec<u8>>)> = env.framework_state.uikit.ui_image_picker.pending.map(|(_, request)| (request, None));
    let Some((request, bytes)) = result else { return false; };
    let state = &mut env.framework_state.uikit.ui_image_picker;
    let Some((picker, pending_request)) = state.pending else { return false; };
    if pending_request != request { return false; }
    state.pending = None;
    if let Some(bytes) = bytes {
        deliver_image(env, picker, &bytes);
    } else {
        log!("UIImagePickerController: selection cancelled or access denied");
        let delegate = env.objc.borrow::<UIImagePickerControllerHostObject>(picker).superclass.delegate;
        cancel(env, picker, delegate);
    }
    release(env, picker);
    true
}

pub fn poll(env: &mut Environment) -> bool {
    let mut handled = poll_library(env);
    #[cfg(feature = "embedded")]
    let result = crate::embed::take_camera_photo_result();
    #[cfg(not(feature = "embedded"))]
    let result = {
        let state = &mut env.framework_state.uikit.ui_image_picker;
        state.capture_result.take().and_then(|bytes| state.capture.map(|(_, request)| (request, bytes)))
    };
    if let Some((request, bytes)) = result {
        let state = &mut env.framework_state.uikit.ui_image_picker;
        if let Some((picker, pending)) = state.capture {
            if request == pending {
                state.capture = None;
                let pool: id = msg_class![env; NSAutoreleasePool new];
                if let Some(bytes) = bytes { deliver_image(env, picker, &bytes); }
                else {
                    let delegate = env.objc.borrow::<UIImagePickerControllerHostObject>(picker).superclass.delegate;
                    cancel(env, picker, delegate);
                }
                release(env, pool);
                release(env, picker);
                handled = true;
            }
        }
    }
    #[cfg(feature = "embedded")]
    if let Some(picker) = env.framework_state.uikit.ui_image_picker.camera_picker {
        let state = &mut env.framework_state.uikit.ui_image_picker;
        let now = Instant::now();
        if state.last_preview.is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_millis(66)) {
            state.last_preview = Some(now);
            if let Some(bytes) = crate::embed::take_camera_frame() {
                let pool: id = msg_class![env; NSAutoreleasePool new];
                let image = decoded_image(env, &bytes);
                if image != nil {
                    let preview = env.objc.borrow::<UIImagePickerControllerHostObject>(picker).preview_view;
                    () = msg![env; preview setImage:image];
                    handled = true;
                }
                release(env, pool);
            }
        }
    }
    handled
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIImagePickerController: UINavigationController

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIImagePickerControllerHostObject {
        superclass: super::ui_view_controller::ui_navigation_controller::UINavigationControllerHostObject::new(),
        source_type: UIImagePickerControllerSourceTypePhotoLibrary,
        media_types: nil,
        allows_editing: false,
        video_quality: UIImagePickerControllerQualityTypeMedium,
        camera_device: UIImagePickerControllerCameraDeviceRear,
        camera_flash_mode: UIImagePickerControllerCameraFlashModeAuto,
        camera_capture_mode: 0,
        shows_camera_controls: true,
        camera_overlay_view: nil,
        camera_view_transform: CGAffineTransformIdentity,
        preview_view: nil,
        controls_view: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Source type

+ (bool)isSourceTypeAvailable:(UIImagePickerControllerSourceType)source_type {
    source_available(source_type,
        crate::android_media::has_camera(false) || crate::android_media::has_camera(true),
        library_available())
}

+ (id)availableMediaTypesForSourceType:(UIImagePickerControllerSourceType)source_type {
    let available: bool = msg_class![env; UIImagePickerController isSourceTypeAvailable:source_type];
    if available {
        // Every implemented source supports still images.
        let image_type = get_static_str(env, "public.image");
        let arr: id = msg_class![env; NSArray arrayWithObject:image_type];
        return arr;
    }
    nil
}

+ (bool)isCameraDeviceAvailable:(UIImagePickerControllerCameraDevice)device {
    let has_back = crate::android_media::has_camera(false);
    let has_front = crate::android_media::has_camera(true);
    match device {
        UIImagePickerControllerCameraDeviceRear => has_back,
        UIImagePickerControllerCameraDeviceFront => has_front,
        _ => false,
    }
}

+ (bool)isFlashAvailableForCameraDevice:(UIImagePickerControllerCameraDevice)device {
    crate::android_media::has_flash(device == UIImagePickerControllerCameraDeviceFront)
}

+ (id)availableCaptureModesForCameraDevice:(UIImagePickerControllerCameraDevice)device {
    let available = match device {
        UIImagePickerControllerCameraDeviceRear => crate::android_media::has_camera(false),
        UIImagePickerControllerCameraDeviceFront => crate::android_media::has_camera(true),
        _ => false,
    };
    if !available {
        return msg_class![env; NSArray new];
    }
    // Photo capture is available on a real camera.
    let photo: id = msg_class![env; NSNumber numberWithInteger:
        (UIImagePickerControllerCameraCaptureModePhoto)];
    let arr: id = msg_class![env; NSArray arrayWithObject:photo];
    arr
}

// Apple's UIImagePickerController contract is portrait-only. Custom overlays
// use the picker's portrait coordinate space even in a landscape application.
// https://developer.apple.com/documentation/uikit/uiimagepickercontroller
- (bool)shouldAutorotateToInterfaceOrientation:(NSInteger)orientation {
    orientation == 1
}

- (id)init {
    let image_type = get_static_str(env, "public.image");
    let types: id = msg_class![env; NSArray arrayWithObject:image_type];
    retain(env, types);
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).media_types = types;
    this
}

- (())dealloc {
    let host = env.objc.borrow::<UIImagePickerControllerHostObject>(this);

    let (media_types, camera_overlay_view, preview, controls) =
        (host.media_types, host.camera_overlay_view, host.preview_view, host.controls_view);

    release(env, media_types);
    release(env, camera_overlay_view);
    release(env, preview);
    release(env, controls);
    () = msg_super![env; this dealloc];
}

- (UIImagePickerControllerSourceType)sourceType {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).source_type
}

- (())setSourceType:(UIImagePickerControllerSourceType)source_type {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).source_type = source_type;
    update_camera_views_if_loaded(env, this);
}

// MARK: - Media types

- (id)mediaTypes {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).media_types
}

- (())setMediaTypes:(id)media_types {
    let old = env.objc.borrow::<UIImagePickerControllerHostObject>(this).media_types;
    release(env, old);
    retain(env, media_types);
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).media_types = media_types;
}

// MARK: - Editing

- (bool)allowsEditing {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).allows_editing
}

- (())setAllowsEditing:(bool)allows {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).allows_editing = allows;
}

// Deprecated alias.
- (bool)allowsImageEditing {
    msg![env; this allowsEditing]
}
- (())setAllowsImageEditing:(bool)allows {
    msg![env; this setAllowsEditing:allows]
}

// MARK: - Video quality

- (UIImagePickerControllerQualityType)videoQuality {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).video_quality
}

- (())setVideoQuality:(UIImagePickerControllerQualityType)quality {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).video_quality = quality;
}

// MARK: - Camera device & flash

- (UIImagePickerControllerCameraDevice)cameraDevice {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_device
}

- (())setCameraDevice:(UIImagePickerControllerCameraDevice)device {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).camera_device = device;
    if env.framework_state.uikit.ui_image_picker.camera_picker == Some(this) {
        start_camera_preview(env, this);
    }
}

- (UIImagePickerControllerCameraFlashMode)cameraFlashMode {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_flash_mode
}

- (())setCameraFlashMode:(UIImagePickerControllerCameraFlashMode)mode {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).camera_flash_mode = mode;
    if env.framework_state.uikit.ui_image_picker.camera_picker == Some(this) {
        start_camera_preview(env, this);
    }
}

- (UIImagePickerControllerCameraCaptureMode)cameraCaptureMode {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_capture_mode
}

- (())setCameraCaptureMode:(UIImagePickerControllerCameraCaptureMode)mode {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).camera_capture_mode = mode;
}

// MARK: - Camera controls & overlay

- (bool)showsCameraControls {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).shows_camera_controls
}

- (())setShowsCameraControls:(bool)shows {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).shows_camera_controls = shows;
    update_camera_views_if_loaded(env, this);
}

- (id)cameraOverlayView {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_overlay_view
}

- (())setCameraOverlayView:(id)view {
    let old = env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_overlay_view;
    if old == view { return; }
    retain(env, view);
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).camera_overlay_view = view;
    if old != nil { () = msg![env; old removeFromSuperview]; }
    release(env, old);
    update_camera_views_if_loaded(env, this);
}

- (CGAffineTransform)cameraViewTransform {
    env.objc.borrow::<UIImagePickerControllerHostObject>(this).camera_view_transform
}
- (())setCameraViewTransform:(CGAffineTransform)transform {
    env.objc.borrow_mut::<UIImagePickerControllerHostObject>(this).camera_view_transform = transform;
    update_camera_views_if_loaded(env, this);
}

- (())viewDidLoad {
    () = msg_super![env; this viewDidLoad];
    let root: id = msg![env; this view];
    configure_camera_views(env, this, root);
}
- (())viewDidLayoutSubviews {
    () = msg_super![env; this viewDidLayoutSubviews];
    update_camera_views_if_loaded(env, this);
}
- (())_touchHLE_cameraShutter:(id)_sender { () = msg![env; this takePicture]; }
- (())_touchHLE_cameraCancel:(id)_sender {
    let delegate = env.objc.borrow::<UIImagePickerControllerHostObject>(this).superclass.delegate;
    cancel(env, this, delegate);
}

// MARK: - Camera actions (stubs)

- (bool)startVideoCapture {
    log!("UIImagePickerController startVideoCapture: stubbed, returning false");
    false
}

- (())stopVideoCapture {
    log!("UIImagePickerController stopVideoCapture: stubbed");
}

- (())takePicture {
    let host = env.objc.borrow::<UIImagePickerControllerHostObject>(this);
    if host.source_type != UIImagePickerControllerSourceTypeCamera { return; }
    #[cfg(not(feature = "embedded"))]
    let front = host.camera_device == UIImagePickerControllerCameraDeviceFront;
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.capture.is_some() { return; }
    state.next_request = state.next_request.wrapping_add(1).max(1);
    let request = state.next_request;
    state.capture = Some((this, request));
    retain(env, this);
    #[cfg(feature = "embedded")]
    crate::embed::request_camera_photo(request);
    #[cfg(not(feature = "embedded"))]
    { env.framework_state.uikit.ui_image_picker.capture_result = Some(crate::android_media::take_photo(front)); }
}

- (())viewDidAppear:(bool)animated {
    () = msg_super![env; this viewDidAppear:animated];
    let source = env.objc.borrow::<UIImagePickerControllerHostObject>(this).source_type;
    if source == UIImagePickerControllerSourceTypeCamera {
        update_camera_views_if_loaded(env, this);
        start_camera_preview(env, this);
        return;
    }
    if source != UIImagePickerControllerSourceTypePhotoLibrary
        && source != UIImagePickerControllerSourceTypeSavedPhotosAlbum { return; }
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.pending.is_some() { return; }
    state.next_request = state.next_request.wrapping_add(1);
    let request = state.next_request;
    state.pending = Some((this, request));
    retain(env, this);
    log!("UIImagePickerController: requesting host photo library (request {})", request);
    #[cfg(feature = "embedded")]
    crate::embed::request_photo_picker(request);
}

- (())viewDidDisappear:(bool)animated {
    () = msg_super![env; this viewDidDisappear:animated];
    stop_camera_preview(env, this);
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.capture.is_some_and(|(picker, _)| picker == this) {
        state.capture = None;
        state.capture_result = None;
        release(env, this);
    }
    let state = &mut env.framework_state.uikit.ui_image_picker;
    if state.pending.is_some_and(|(picker, _)| picker == this) {
        state.pending = None;
        release(env, this);
    }
}

@end

};

/// `UIVideoAtPathIsCompatibleWithSavedPhotosAlbum(NSString*)` — Apple's API
/// to ask whether a movie file can be saved to the Camera Roll. touchHLE
/// has no Photos library, so the honest answer is "no". Returning `false`
/// also matches the documented behaviour for files with unsupported
/// codecs/containers.
fn UIVideoAtPathIsCompatibleWithSavedPhotosAlbum(env: &mut Environment, video_path: id) -> bool {
    let path_str = if video_path == nil {
        "(nil)".to_string()
    } else {
        to_rust_string(env, video_path).into_owned()
    };
    log!(
        "UIVideoAtPathIsCompatibleWithSavedPhotosAlbum({:?}): stubbed, returning false",
        path_str
    );
    false
}

/// `UISaveVideoAtPathToSavedPhotosAlbum(NSString*, id, SEL, void*)` — Apple's
/// API to copy a movie to the Camera Roll. touchHLE has no Photos library
/// so we simply log and do nothing; the optional completion selector is
/// not invoked because the documented contract on real iOS is that the
/// callback signals "saved", not "ignored", and we'd rather have a quiet
/// no-op than spoof a false success.
fn UISaveVideoAtPathToSavedPhotosAlbum(
    env: &mut Environment,
    video_path: id,
    _completion_target: id,
    _completion_selector: crate::objc::SEL,
    _context_info: crate::mem::MutVoidPtr,
) {
    let path_str = if video_path == nil {
        "(nil)".to_string()
    } else {
        to_rust_string(env, video_path).into_owned()
    };
    log!(
        "UISaveVideoAtPathToSavedPhotosAlbum({:?}): stubbed (no Photos library in touchHLE)",
        path_str
    );
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(UIVideoAtPathIsCompatibleWithSavedPhotosAlbum(_)),
    export_c_func!(UISaveVideoAtPathToSavedPhotosAlbum(_, _, _, _)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_overlay_mounts_above_preview_and_transform_does_not_move_overlay() {
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let picker: id = msg_class![env; UIImagePickerController new];
        let bounds = CGRect { origin: CGPoint::default(), size: CGSize { width: 320.0, height: 480.0 } };
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:bounds];
        () = msg![env; picker setView:root];
        () = msg![env; picker setSourceType:(1i32)];
        let overlay: id = msg_class![env; UIView alloc];
        let overlay: id = msg![env; overlay initWithFrame:bounds];
        () = msg![env; picker setCameraOverlayView:overlay];
        let refs = env.objc.get_refcount(overlay).get();
        () = msg![env; picker setCameraOverlayView:overlay];
        assert_eq!(env.objc.get_refcount(overlay).get(), refs);
        let parent: id = msg![env; overlay superview];
        assert_eq!(parent, root);
        let transform = CGAffineTransform::make_translation(0.0, 25.0);
        () = msg![env; picker setCameraViewTransform:transform];
        () = msg![env; picker setShowsCameraControls:false];
        let host = env.objc.borrow::<UIImagePickerControllerHostObject>(picker);
        let (preview, controls) = (host.preview_view, host.controls_view);
        let actual: CGAffineTransform = msg![env; preview transform];
        assert_eq!(actual, transform);
        let actual: CGAffineTransform = msg![env; overlay transform];
        assert_eq!(actual, CGAffineTransformIdentity);
        let hidden: bool = msg![env; controls isHidden];
        assert!(hidden);
        let subviews = &env.objc.borrow::<super::super::ui_view::UIViewHostObject>(root).subviews;
        assert_eq!(subviews.last(), Some(&overlay));
        () = msg![env; picker setCameraOverlayView:nil];
        let parent: id = msg![env; overlay superview];
        assert_eq!(parent, nil);
        assert_eq!(env.objc.get_refcount(overlay).get(), 1);
        release(env, overlay);
        release(env, picker);
        release(env, root);
        release(env, pool);
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn camera_capture_retains_picker_and_delivers_failure_asynchronously() {
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let picker: id = msg_class![env; UIImagePickerController new];
        () = msg![env; picker setSourceType:(1i32)];
        let before = env.objc.get_refcount(picker).get();
        () = msg![env; picker takePicture];
        assert!(env.framework_state.uikit.ui_image_picker.capture.is_some());
        assert_eq!(env.objc.get_refcount(picker).get(), before + 1);
        // A second shutter press cannot replace or leak the in-flight request.
        let pending = env.framework_state.uikit.ui_image_picker.capture;
        () = msg![env; picker takePicture];
        assert_eq!(env.framework_state.uikit.ui_image_picker.capture, pending);
        assert!(poll(env));
        assert!(env.framework_state.uikit.ui_image_picker.capture.is_none());
        assert_eq!(env.objc.get_refcount(picker).get(), before);
        release(env, picker);
        release(env, pool);
    }

    // Source availability describes a picker source, independently of permission.
    // https://developer.apple.com/documentation/uikit/uiimagepickercontroller/issourcetypeavailable(_:)
    #[test]
    fn library_sources_do_not_require_a_camera() {
        for source in [0, 2] {
            assert!(source_available(source, false, true));
            assert!(!source_available(source, true, false));
        }
        assert!(!source_available(1, false, true));
        assert!(source_available(1, true, false));
        assert!(!source_available(3, true, true));
        assert!(!source_available(-1, true, true));
    }
}
