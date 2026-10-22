/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Window backend for the embedded Vetus build.
//!
//! Presents into an `ANativeWindow` with EGL (bundled ANGLE when it loads)
//! instead of SDL. Input arrives from the host process through [crate::embed].

#[path = "window/geometry.rs"]
mod geometry;

include!("window_shared.rs");

use crate::gles::present::present_frame;
use crate::gles::{
    create_gles1_ctx_no_parent_stack, create_gles2_ctx_no_parent_stack, GLESContext,
    LoggingGLESContext,
};
use crate::image::Image;
use crate::matrix::Matrix;
use crate::options::Options;
use crate::Environment;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{FRAC_PI_2, PI};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

const ORIENTATION_PORTRAIT: i32 = 0;
const ORIENTATION_PORTRAIT_UPSIDE_DOWN: i32 = 1;
const ORIENTATION_LANDSCAPE_LEFT: i32 = 2;
const ORIENTATION_LANDSCAPE_RIGHT: i32 = 3;

pub fn angle_backend_available() -> bool {
    true
}

pub fn host_screen_size() -> Option<(u32, u32)> {
    crate::embed::host_screen()
}

pub struct GLContext {
    raw: usize,
}

impl GLContext {
    pub fn is_current(&self) -> bool {
        CURRENT_CONTEXT.with(|current| current.get() == self.raw)
    }
}

thread_local! {
    static CURRENT_CONTEXT: Cell<usize> = const { Cell::new(0) };
}

pub struct Window {
    event_queue: VecDeque<Event>,
    high_priority_event: Option<Event>,
    last_polled: Instant,
    scale_hack: NonZeroU32,
    host_screen_size: Option<(u32, u32)>,
    drawable: (u32, u32),
    internal_gl_ins: Option<Box<dyn GLESContext>>,
    gl_driver_description: String,
    host_framebuffer: u32,
    splash_image: Option<Image>,
    splash_image_is_orientation_specific: bool,
    device_family: DeviceFamily,
    device_orientation: DeviceOrientation,
    screen_saver_enabled: bool,
    text_input_active: Cell<bool>,
    share_with_current: Cell<bool>,
    angle: bool,
    adreno: bool,
    blocked: bool,
    /// When the host first asked to pause while this thread was between frames.
    pause_seen: Option<Instant>,
    bound_window: usize,
    show_fps_counter: Cell<bool>,
    fps_frame_count: Cell<u32>,
    fps_last_log: RefCell<Instant>,
    perf_hints: crate::perf_hints::PerfHints,
    pub(super) on_main_stack: bool,
    #[cfg(target_os = "android")]
    egl: Egl,
}

impl Window {
    pub fn rotatable_fullscreen() -> bool {
        true
    }

    pub fn new(
        title: &str,
        icon: Option<Image>,
        launch_image: Option<(Image, bool)>,
        options: &Options,
    ) -> Result<Window, String> {
        let _ = (title, icon);
        #[cfg(not(target_os = "android"))]
        {
            let _ = (launch_image, options);
            return Err("The Vetus embedded window only runs on Android".to_string());
        }
        #[cfg(target_os = "android")]
        Self::new_android(launch_image, options)
    }

    #[cfg(target_os = "android")]
    fn new_android(
        launch_image: Option<(Image, bool)>,
        options: &Options,
    ) -> Result<Window, String> {
        let mut egl = Egl::new(!options.gles_native)?;
        let scale_hack = options.scale_hack;
        let host_screen_size = options
            .host_screen_size
            .map(normalize_portrait_size)
            .or_else(crate::embed::host_screen);
        let device_family = options.device_family.unwrap_or(DeviceFamily::iPhone);
        let device_orientation = options.initial_orientation;
        let (splash_image, splash_image_is_orientation_specific) = launch_image
            .map(|(image, specific)| (Some(image), specific))
            .unwrap_or((None, false));
        let drawable = egl.drawable_size().unwrap_or((0, 0));
        let mut window = Window {
            event_queue: VecDeque::new(),
            high_priority_event: None,
            last_polled: Instant::now() - Duration::from_secs(1),
            scale_hack,
            host_screen_size,
            drawable,
            internal_gl_ins: None,
            gl_driver_description: String::new(),
            host_framebuffer: 0,
            splash_image,
            splash_image_is_orientation_specific,
            device_family,
            device_orientation,
            screen_saver_enabled: true,
            text_input_active: Cell::new(false),
            share_with_current: Cell::new(false),
            angle: egl.angle,
            adreno: false,
            blocked: false,
            pause_seen: None,
            bound_window: 0,
            show_fps_counter: Cell::new(false),
            fps_frame_count: Cell::new(0),
            fps_last_log: RefCell::new(Instant::now()),
            perf_hints: crate::perf_hints::PerfHints::new(
                options.perf_hints,
                options
                    .fps_limit
                    .map(|fps| Duration::from_secs_f64(1.0 / fps))
                    .unwrap_or(Duration::from_micros(16_667)),
                options.affinity.as_deref(),
            ),
            on_main_stack: true,
            egl,
        };
        let mut gl_ins = if options.prefer_gles2_context {
            create_gles2_ctx_no_parent_stack(&mut window)
        } else {
            create_gles1_ctx_no_parent_stack(&mut window, options)
        }
        .map_err(|err| format!("Could not initialize an OpenGL ES context: {err}"))?;
        if options.trace_gl_errors {
            gl_ins = Box::new(LoggingGLESContext {
                inner: gl_ins,
                verbose: options.trace_gl_errors || options.verbose_gles,
            });
        }
        let gl_driver_description = {
            let gl_ctx = gl_ins.make_current(&mut window);
            unsafe { gl_ctx.driver_description() }
        };
        log!("Driver info: {gl_driver_description}");
        window.adreno = gl_driver_description.to_ascii_lowercase().contains("adreno");
        window.gl_driver_description = gl_driver_description;
        window.internal_gl_ins = Some(gl_ins);
        window.egl.set_swap_interval(0);
        populate_battery_cache();
        crate::embed::request_orientation(orientation_code(window.device_orientation));
        if window.splash_image.is_some() {
            window.display_splash();
        }
        Ok(window)
    }

    pub fn is_angle_backend(&self) -> bool {
        self.angle
    }

    pub fn is_adreno_gpu(&self) -> bool {
        self.adreno
    }

    pub fn gl_driver_description(&self) -> &str {
        &self.gl_driver_description
    }

    pub fn poll_for_events(&mut self, _options: &Options) {
        if !self.on_main_stack {
            return;
        }
        self.service_host(false);
        let now = Instant::now();
        if now.duration_since(self.last_polled) < Duration::from_secs_f64(1.0 / 120.0) {
            return;
        }
        self.last_polled = now;
        if crate::embed::should_quit() {
            self.high_priority_event = Some(Event::Quit);
            return;
        }
        self.drain_input();
    }

    /// t_swap is true when called right before a frame is presented, so the
    /// back buffer holds the finished frame. Parking there lets the last frame
    /// be read back before the surface goes away; between frames the buffer is
    /// undefined, so a pause waits (briefly) for the next swap instead.
    fn service_host(&mut self, at_swap: bool) {
        let parking = !self.blocked
            && !crate::embed::should_quit()
            && !crate::embed::is_runnable();
        if parking {
            if at_swap {
                self.capture_frame();
            } else {
                let since = *self.pause_seen.get_or_insert_with(Instant::now);
                if since.elapsed() < Duration::from_millis(300) {
                    return;
                }
            }
        } else {
            self.pause_seen = None;
        }
        self.sync_surface();
        if crate::embed::should_quit() {
            return;
        }
        if crate::embed::is_runnable() {
            if self.blocked {
                crate::guest_clock::set_frozen(false);
                crate::audio::openal::resume_all_contexts();
                self.blocked = false;
            }
            return;
        }
        if !self.blocked {
            self.pause_seen = None;
            crate::audio::openal::suspend_all_contexts();
            crate::guest_clock::set_frozen(true);
            self.blocked = true;
        }
        crate::embed::wait_until_runnable();
        self.sync_surface();
        if crate::embed::is_runnable() && self.blocked {
            crate::guest_clock::set_frozen(false);
            crate::audio::openal::resume_all_contexts();
            self.blocked = false;
        }
    }

    fn sync_surface(&mut self) {
        #[cfg(target_os = "android")]
        {
            let next = crate::embed::native_window();
            if next == self.bound_window {
                if next != 0 {
                    self.drawable = self.egl.drawable_size().unwrap_or(self.drawable);
                }
                return;
            }
            if self.bound_window != 0 {
                self.egl.destroy_window_surface();
                unsafe { ANativeWindow_release(self.bound_window as *mut _) };
            }
            self.bound_window = next;
            if next != 0 {
                if let Err(err) = self.egl.adopt_window(next as *mut _) {
                    log!("Could not adopt Vetus surface: {err}");
                }
                self.drawable = self.egl.drawable_size().unwrap_or(self.drawable);
                crate::embed::set_host_screen(self.drawable.0, self.drawable.1);
            }
        }
    }

    fn capture_frame(&mut self) {
        #[cfg(target_os = "android")]
        {
            let (width, height) = self.drawable;
            if width == 0 || height == 0 || self.internal_gl_ins.is_none() {
                return;
            }
            let pixels = (width as usize).saturating_mul(height as usize).saturating_mul(4);
            let mut rgba = vec![0u8; pixels];
            let Some(read) = self.egl.read_pixels else {
                return;
            };
            unsafe {
                let mut gl_ctx = self
                    .internal_gl_ins
                    .as_mut()
                    .unwrap()
                    .make_current_unchecked_for_window(
                        &mut |gl_ctx| {
                            self.egl.make_current(gl_ctx.raw);
                            CURRENT_CONTEXT.with(|current| current.set(gl_ctx.raw));
                        },
                        &mut |name| self.egl.proc_address(name),
                    );
                let _ = gl_ctx;
                read(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    0x1908,
                    0x1401,
                    rgba.as_mut_ptr(),
                );
            }
            flip_rows(width, height, &mut rgba);
            crate::embed::store_frame(width, height, rgba);
        }
    }

    fn drain_input(&mut self) {
        use crate::embed::InputKind;
        let mut down = HashMap::new();
        let mut moved = HashMap::new();
        let mut up = HashMap::new();
        for event in crate::embed::drain_input() {
            match event.kind {
                InputKind::Touch { action, id, x, y } => {
                    let coords = self.transform_input((x, y));
                    let finger = FingerId::Touch(id);
                    match action {
                        0 | 5 => {
                            down.insert(finger, coords);
                        }
                        2 => {
                            moved.insert(finger, coords);
                        }
                        _ => {
                            up.insert(finger, coords);
                        }
                    }
                }
                InputKind::Text(text) => {
                    self.event_queue.push_back(Event::TextInput(TextInputEvent::Text(text)));
                }
                InputKind::Backspace => {
                    self.event_queue
                        .push_back(Event::TextInput(TextInputEvent::Backspace));
                }
                InputKind::Return => {
                    self.event_queue
                        .push_back(Event::TextInput(TextInputEvent::Return));
                }
            }
        }
        if !down.is_empty() {
            self.event_queue.push_back(Event::TouchesDown(down));
        }
        if !moved.is_empty() {
            self.event_queue.push_back(Event::TouchesMove(moved));
        }
        if !up.is_empty() {
            self.event_queue.push_back(Event::TouchesUp(up));
        }
    }

    fn transform_input(&self, (in_x, in_y): (f32, f32)) -> (f32, f32) {
        let (vx, vy, vw, vh) = self.viewport();
        let (x, y) = geometry::normalize_viewport_coords((in_x, in_y), (vx, vy, vw, vh));
        let [x, y] = if crate::env_flag_cached!("TOUCHHLE_DISABLE_PRESENT_ROTATION")
            || crate::env_flag_cached!("TOUCHHLE_DISABLE_TOUCH_ROTATION")
        {
            [x, y]
        } else {
            self.rotation_matrix().inverse().unwrap().transform([x, y])
        };
        let (out_w, out_h) = self.size_unrotated_unscaled();
        let out_x = ((x + 0.5) * out_w as f32).clamp(0.0, out_w.saturating_sub(1) as f32);
        let out_y = ((y + 0.5) * out_h as f32).clamp(0.0, out_h.saturating_sub(1) as f32);
        (out_x.round(), out_y.round())
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        if let Some(event) = self.high_priority_event.take() {
            return Some(event);
        }
        self.event_queue.pop_front()
    }

    pub fn print_accelerometer_notice(&self, _options: &Options) {
        log!("Using the Android accelerometer and gyroscope.");
    }

    pub fn get_acceleration(&self, _options: &Options) -> (f32, f32, f32) {
        let (x, y, z) = crate::embed::accel();
        let gravity = 9.80665f32;
        accelerometer_compat_remap(-x / gravity, -y / gravity, -z / gravity)
    }

    pub fn has_gyroscope(&self) -> bool {
        crate::embed::gyro().is_some()
    }

    pub fn get_rotation_rate(&self) -> Option<(f32, f32, f32)> {
        crate::embed::gyro()
    }

    pub fn virtual_cursor_visible_at(&self) -> Option<(f32, f32, bool)> {
        None
    }

    pub fn create_gl_context(&self, version: GLVersion) -> Result<GLContext, String> {
        #[cfg(not(target_os = "android"))]
        {
            let _ = version;
            return Err("embedded GL requires Android".to_string());
        }
        #[cfg(target_os = "android")]
        {
            let share = if self.share_with_current.get() {
                CURRENT_CONTEXT.with(|current| current.get())
            } else {
                0
            };
            let raw = self.egl.create_context(version, share)?;
            Ok(GLContext { raw })
        }
    }

    pub fn gl_get_proc_address(&self, procname: &str) -> *const std::ffi::c_void {
        #[cfg(target_os = "android")]
        {
            return self.egl.proc_address(procname);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = procname;
            std::ptr::null()
        }
    }

    pub fn set_share_with_current_context(&self, value: bool) {
        self.share_with_current.set(value);
    }

    pub unsafe fn make_gl_context_current(&self, gl_ctx: &GLContext) {
        #[cfg(target_os = "android")]
        {
            self.egl.make_current(gl_ctx.raw);
            CURRENT_CONTEXT.with(|current| current.set(gl_ctx.raw));
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = gl_ctx;
        }
    }

    #[must_use]
    pub fn make_internal_gl_ctx_current<'win>(&'win mut self) -> Box<dyn crate::gles::GLES + 'win> {
        unsafe {
            self.internal_gl_ins
                .as_mut()
                .expect("internal GL context")
                .make_current_unchecked_for_window(
                    &mut |gl_ctx| {
                        #[cfg(target_os = "android")]
                        {
                            self.egl.make_current(gl_ctx.raw);
                            CURRENT_CONTEXT.with(|current| current.set(gl_ctx.raw));
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            let _ = gl_ctx;
                        }
                    },
                    &mut |name| {
                        #[cfg(target_os = "android")]
                        {
                            self.egl.proc_address(name)
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            let _ = name;
                            std::ptr::null()
                        }
                    },
                )
        }
    }

    pub fn swap_window(&mut self) {
        self.service_host(true);
        #[cfg(target_os = "android")]
        self.egl.swap();
        self.perf_hints.frame_presented();
        if self.show_fps_counter.get() {
            let frames = self.fps_frame_count.get().saturating_add(1);
            self.fps_frame_count.set(frames);
            let now = Instant::now();
            let mut last = self.fps_last_log.borrow_mut();
            let elapsed = now.duration_since(*last);
            if elapsed >= Duration::from_secs(1) {
                let fps = frames as f32 / elapsed.as_secs_f32().max(1e-6);
                echo!("FPS: {fps:.1}");
                *last = now;
                self.fps_frame_count.set(0);
            }
        }
    }

    pub fn rotate_device(&mut self, new_orientation: DeviceOrientation) {
        if !self.on_main_stack || new_orientation == self.device_orientation {
            return;
        }
        self.device_orientation = new_orientation;
        crate::embed::request_orientation(orientation_code(new_orientation));
        if self.splash_image.is_some() {
            self.display_splash();
        }
    }

    fn display_splash(&mut self) {
        let Some(image) = self.splash_image.as_ref() else {
            return;
        };
        let (image_width, image_height) = image.dimensions();
        let is_landscape = matches!(
            self.device_orientation,
            DeviceOrientation::LandscapeLeft | DeviceOrientation::LandscapeRight
        );
        let rotation = if self.splash_image_is_orientation_specific {
            Matrix::identity()
        } else if is_landscape && image_height > image_width {
            self.rotation_matrix().inverse().unwrap()
        } else {
            self.rotation_matrix()
        };
        let matrix = rotation.multiply(&Matrix::y_flip());
        let (vx, vy, vw, vh) = self.viewport();
        let viewport = (vx, vy + self.viewport_y_offset(), vw, vh);
        let pixels = image.pixels().as_ptr();
        unsafe {
            let mut gl_ctx = self
                .internal_gl_ins
                .as_mut()
                .unwrap()
                .make_current_unchecked_for_window(
                    &mut |gl_ctx| {
                        #[cfg(target_os = "android")]
                        {
                            self.egl.make_current(gl_ctx.raw);
                            CURRENT_CONTEXT.with(|current| current.set(gl_ctx.raw));
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            let _ = gl_ctx;
                        }
                    },
                    &mut |name| {
                        #[cfg(target_os = "android")]
                        {
                            self.egl.proc_address(name)
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            let _ = name;
                            std::ptr::null()
                        }
                    },
                );
            use crate::gles::gles11_raw as gles11;
            let mut texture = 0;
            gl_ctx.GenTextures(1, &mut texture);
            gl_ctx.BindTexture(gles11::TEXTURE_2D, texture);
            gl_ctx.TexImage2D(
                gles11::TEXTURE_2D,
                0,
                gles11::RGBA as _,
                image_width as _,
                image_height as _,
                0,
                gles11::RGBA,
                gles11::UNSIGNED_BYTE,
                pixels as *const _,
            );
            gl_ctx.TexParameteri(
                gles11::TEXTURE_2D,
                gles11::TEXTURE_MIN_FILTER,
                gles11::LINEAR as _,
            );
            gl_ctx.TexParameteri(
                gles11::TEXTURE_2D,
                gles11::TEXTURE_MAG_FILTER,
                gles11::LINEAR as _,
            );
            present_frame(gl_ctx.as_mut(), viewport, matrix, None);
            gl_ctx.DeleteTextures(1, &texture);
        }
        #[cfg(target_os = "android")]
        self.egl.swap();
    }

    pub fn device_family(&self) -> DeviceFamily {
        self.device_family
    }

    pub fn screen_size(&self) -> (u32, u32) {
        self.host_screen_size
            .unwrap_or_else(|| self.device_family.portrait_size())
    }

    pub fn screen_scale(&self) -> f32 {
        if self.host_screen_size.is_some() {
            1.0
        } else {
            self.device_family.scale_factor()
        }
    }

    pub fn current_rotation(&self) -> DeviceOrientation {
        self.device_orientation
    }

    pub fn size_unrotated_unscaled(&self) -> (u32, u32) {
        size_for_orientation_from_size(
            self.screen_size(),
            DeviceOrientation::Portrait,
            NonZeroU32::new(1).unwrap(),
        )
    }

    pub fn viewport(&self) -> (u32, u32, u32, u32) {
        let app = size_for_orientation_from_size(
            self.screen_size(),
            self.device_orientation,
            self.scale_hack,
        );
        let screen = if self.drawable.0 == 0 || self.drawable.1 == 0 {
            app
        } else {
            self.drawable
        };
        geometry::fit_viewport(app, screen)
    }

    pub fn guest_frame_to_window_px(
        &self,
        frame: crate::frameworks::core_graphics::CGRect,
    ) -> (i32, i32, i32, i32) {
        let (vp_x, vp_y, vp_w, vp_h) = self.viewport();
        let (app_w, app_h) = self.size_unrotated_unscaled();
        let (app_w, app_h) = (app_w as f32, app_h as f32);
        if app_w <= 0.0 || app_h <= 0.0 || vp_w == 0 || vp_h == 0 {
            return (0, 0, 0, 0);
        }
        let sx = vp_w as f32 / app_w;
        let sy = vp_h as f32 / app_h;
        let (x, y, w, h) = match self.device_orientation {
            DeviceOrientation::Portrait => (
                frame.origin.x as f32,
                frame.origin.y as f32,
                frame.size.width as f32,
                frame.size.height as f32,
            ),
            DeviceOrientation::PortraitUpsideDown => (
                app_w - frame.origin.x as f32 - frame.size.width as f32,
                app_h - frame.origin.y as f32 - frame.size.height as f32,
                frame.size.width as f32,
                frame.size.height as f32,
            ),
            DeviceOrientation::LandscapeLeft => (
                frame.origin.y as f32,
                app_w - frame.origin.x as f32 - frame.size.width as f32,
                frame.size.height as f32,
                frame.size.width as f32,
            ),
            DeviceOrientation::LandscapeRight => (
                app_h - frame.origin.y as f32 - frame.size.height as f32,
                frame.origin.x as f32,
                frame.size.height as f32,
                frame.size.width as f32,
            ),
        };
        (
            (vp_x as f32 + x * sx).round() as i32,
            (vp_y as f32 + y * sy).round() as i32,
            (w * sx).round().max(0.0) as i32,
            (h * sy).round().max(0.0) as i32,
        )
    }

    pub fn host_framebuffer(&self) -> u32 {
        self.host_framebuffer
    }

    pub fn viewport_y_offset(&self) -> u32 {
        0
    }

    pub fn rotation_matrix(&self) -> Matrix<2> {
        match self.device_orientation {
            DeviceOrientation::Portrait => Matrix::identity(),
            DeviceOrientation::PortraitUpsideDown => Matrix::z_rotation(PI),
            DeviceOrientation::LandscapeLeft => Matrix::z_rotation(-FRAC_PI_2),
            DeviceOrientation::LandscapeRight => Matrix::z_rotation(FRAC_PI_2),
        }
    }

    pub fn is_screen_saver_enabled(&self) -> bool {
        self.screen_saver_enabled
    }

    pub fn set_screen_saver_enabled(&mut self, enabled: bool) {
        self.screen_saver_enabled = enabled;
        crate::embed::notify_idle_timer(enabled);
    }

    pub fn start_text_input(&self) {
        self.text_input_active.set(true);
        crate::embed::notify_keyboard(true);
    }

    pub fn stop_text_input(&self) {
        self.text_input_active.set(false);
        crate::embed::notify_keyboard(false);
    }

    pub fn is_text_input_active(&self) -> bool {
        self.text_input_active.get()
    }

    pub fn is_screen_keyboard_shown(&self) -> bool {
        self.text_input_active.get()
    }

    pub fn on_main_stack(&self) -> bool {
        self.on_main_stack
    }

    pub fn set_show_fps_counter(&mut self, enabled: bool) {
        self.show_fps_counter.set(enabled);
    }
}

fn orientation_code(orientation: DeviceOrientation) -> i32 {
    match orientation {
        DeviceOrientation::Portrait => ORIENTATION_PORTRAIT,
        DeviceOrientation::PortraitUpsideDown => ORIENTATION_PORTRAIT_UPSIDE_DOWN,
        DeviceOrientation::LandscapeLeft => ORIENTATION_LANDSCAPE_LEFT,
        DeviceOrientation::LandscapeRight => ORIENTATION_LANDSCAPE_RIGHT,
    }
}

fn flip_rows(width: u32, height: u32, rgba: &mut [u8]) {
    let stride = width as usize * 4;
    if stride == 0 || rgba.len() < stride * height as usize {
        return;
    }
    let mut y = 0usize;
    let mut end = height as usize;
    while y + 1 < end {
        end -= 1;
        let top = y * stride;
        let bottom = end * stride;
        for offset in 0..stride {
            rgba.swap(top + offset, bottom + offset);
        }
        y += 1;
    }
}

pub fn open_url(_env: &mut Environment, url: &str) -> Result<(), String> {
    if crate::embed::open_url(url) {
        Ok(())
    } else {
        Err(format!("Could not open {url}"))
    }
}

pub fn launch_ipa_picker(_env: &mut Environment) -> Result<(), String> {
    Err("Import IPAs from the Vetus library".to_string())
}

pub fn show_error_messagebox(_window: Option<&Window>, error_message: &str) {
    log!("{error_message}");
    crate::embed::report_crash(error_message);
}

static BATTERY_CACHE: std::sync::OnceLock<(i32, BatteryState)> = std::sync::OnceLock::new();

pub fn populate_battery_cache() {
    let (percent, state) = crate::embed::battery();
    let state = match state {
        1 => BatteryState::OnBattery,
        2 => BatteryState::NoBattery,
        3 => BatteryState::Charging,
        4 => BatteryState::Full,
        _ => BatteryState::Unknown,
    };
    let _ = BATTERY_CACHE.set((percent, state));
}

pub fn get_battery_status() -> (i32, BatteryState) {
    if let Some((percent, state)) = BATTERY_CACHE.get() {
        return (*percent, *state);
    }
    let (percent, state) = crate::embed::battery();
    let state = match state {
        1 => BatteryState::OnBattery,
        2 => BatteryState::NoBattery,
        3 => BatteryState::Charging,
        4 => BatteryState::Full,
        _ => BatteryState::Unknown,
    };
    (percent, state)
}

pub fn get_preferred_language_codes(_env: &mut Environment) -> Vec<String> {
    crate::embed::languages()
}

pub fn get_preferred_country_codes(_env: &mut Environment) -> Vec<String> {
    crate::embed::countries()
}

pub fn show_alert_dialog(
    _env: &mut Environment,
    title: &str,
    message: &str,
    buttons: &[&str],
) -> i32 {
    crate::embed::alert(title, message, buttons)
}

#[cfg(target_os = "android")]
#[link(name = "android")]
extern "C" {
    fn ANativeWindow_release(window: *mut std::ffi::c_void);
    fn ANativeWindow_getWidth(window: *mut std::ffi::c_void) -> i32;
    fn ANativeWindow_getHeight(window: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "android")]
struct Egl {
    angle: bool,
    display: *mut std::ffi::c_void,
    config: *mut std::ffi::c_void,
    pbuffer: *mut std::ffi::c_void,
    window_surface: *mut std::ffi::c_void,
    window: *mut std::ffi::c_void,
    fns: EglFns,
    read_pixels: Option<GlReadPixels>,
}

#[cfg(target_os = "android")]
type GlReadPixels = unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut u8);

#[cfg(target_os = "android")]
struct EglFns {
    choose_config: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *const i32,
        *mut *mut std::ffi::c_void,
        i32,
        *mut i32,
    ) -> u32,
    create_context: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *const i32,
    ) -> *mut std::ffi::c_void,
    create_pbuffer: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *const i32,
    ) -> *mut std::ffi::c_void,
    create_window: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *const i32,
    ) -> *mut std::ffi::c_void,
    destroy_surface: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> u32,
    make_current: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
    ) -> u32,
    swap_buffers: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> u32,
    swap_interval: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> u32,
    get_proc: unsafe extern "C" fn(*const i8) -> *mut std::ffi::c_void,
}

#[cfg(target_os = "android")]
impl Egl {
    fn new(use_angle: bool) -> Result<Self, String> {
        let (fns, angle, lib) = load_egl(use_angle)?;
        unsafe {
            let get_display: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void =
                std::mem::transmute(load_required(lib, b"eglGetDisplay\0")?);
            let initialize: unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut i32,
                *mut i32,
            ) -> u32 = std::mem::transmute(load_symbol(lib, b"eglInitialize\0").ok_or("eglInitialize missing")?);
            let display = get_display(std::ptr::null_mut());
            if display.is_null() || initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            {
                return Err("eglInitialize failed".to_string());
            }
            let attribs = [
                0x3033, 0x0001 | 0x0004, // SURFACE_TYPE PBUFFER|WINDOW
                0x3040, 0x0001 | 0x0004 | 0x40, // RENDERABLE_TYPE ES1|ES2|ES3
                0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038,
            ];
            let mut config = std::ptr::null_mut();
            let mut count = 0;
            if (fns.choose_config)(display, attribs.as_ptr(), &mut config, 1, &mut count) == 0
                || count == 0
            {
                return Err("eglChooseConfig failed".to_string());
            }
            let pbuffer_attribs = [0x3057, 16, 0x3056, 16, 0x3038];
            let pbuffer = (fns.create_pbuffer)(display, config, pbuffer_attribs.as_ptr());
            if pbuffer.is_null() {
                return Err("eglCreatePbufferSurface failed".to_string());
            }
            let mut egl = Egl {
                angle,
                display,
                config,
                pbuffer,
                window_surface: std::ptr::null_mut(),
                window: std::ptr::null_mut(),
                fns,
                read_pixels: None,
            };
            if let Some(proc_addr) = egl.proc_address_raw(b"glReadPixels\0") {
                egl.read_pixels = Some(std::mem::transmute(proc_addr));
            }
            Ok(egl)
        }
    }

    fn create_context(&self, version: GLVersion, share: usize) -> Result<usize, String> {
        let client = match version {
            GLVersion::GLES11 => 1,
            GLVersion::GLES20 | GLVersion::GL21Compat => 2,
            GLVersion::GLES30 | GLVersion::GL33Core => 3,
        };
        let attribs = [0x3098, client, 0x3038];
        let share_ptr = share as *mut std::ffi::c_void;
        let context = unsafe {
            (self.fns.create_context)(
                self.display,
                self.config,
                if share == 0 { std::ptr::null_mut() } else { share_ptr },
                attribs.as_ptr(),
            )
        };
        if context.is_null() {
            Err(format!("eglCreateContext failed for GLES {client}"))
        } else {
            Ok(context as usize)
        }
    }

    fn make_current(&self, raw: usize) {
        let surface = if self.window_surface.is_null() {
            self.pbuffer
        } else {
            self.window_surface
        };
        unsafe {
            (self.fns.make_current)(self.display, surface, surface, raw as *mut _);
        }
    }

    fn swap(&self) {
        if self.window_surface.is_null() {
            return;
        }
        unsafe {
            (self.fns.swap_buffers)(self.display, self.window_surface);
        }
    }

    fn set_swap_interval(&self, interval: i32) {
        unsafe {
            (self.fns.swap_interval)(self.display, interval);
        }
    }

    fn adopt_window(&mut self, window: *mut std::ffi::c_void) -> Result<(), String> {
        self.destroy_window_surface();
        let surface = unsafe {
            (self.fns.create_window)(self.display, self.config, window, std::ptr::null())
        };
        if surface.is_null() {
            return Err("eglCreateWindowSurface failed".to_string());
        }
        self.window = window;
        self.window_surface = surface;
        Ok(())
    }

    fn destroy_window_surface(&mut self) {
        if !self.window_surface.is_null() {
            unsafe {
                (self.fns.make_current)(
                    self.display,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                (self.fns.destroy_surface)(self.display, self.window_surface);
            }
            self.window_surface = std::ptr::null_mut();
            CURRENT_CONTEXT.with(|current| current.set(0));
        }
        self.window = std::ptr::null_mut();
    }

    fn drawable_size(&self) -> Option<(u32, u32)> {
        if self.window.is_null() {
            return None;
        }
        let width = unsafe { ANativeWindow_getWidth(self.window) };
        let height = unsafe { ANativeWindow_getHeight(self.window) };
        if width <= 0 || height <= 0 {
            None
        } else {
            Some((width as u32, height as u32))
        }
    }

    fn proc_address(&self, name: &str) -> *const std::ffi::c_void {
        let mut raw = name.to_string();
        raw.push('\0');
        self.proc_address_raw(raw.as_bytes()).unwrap_or(std::ptr::null_mut()) as *const _
    }

    fn proc_address_raw(&self, name: &[u8]) -> Option<*mut std::ffi::c_void> {
        let ptr = unsafe { (self.fns.get_proc)(name.as_ptr() as *const i8) };
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }
}

#[cfg(target_os = "android")]
fn load_egl(use_angle: bool) -> Result<(EglFns, bool, *mut std::ffi::c_void), String> {
    unsafe {
        let mut lib = std::ptr::null_mut();
        let mut angle = false;
        if use_angle {
            let _ = libc::dlopen(
                b"libfeature_support_angle.so\0".as_ptr() as *const _,
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            let _ = libc::dlopen(
                b"libGLESv1_CM_angle.so\0".as_ptr() as *const _,
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            let _ = libc::dlopen(
                b"libGLESv2_angle.so\0".as_ptr() as *const _,
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            lib = libc::dlopen(
                b"libEGL_angle.so\0".as_ptr() as *const _,
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            angle = !lib.is_null();
        }
        if lib.is_null() {
            lib = libc::dlopen(b"libEGL.so\0".as_ptr() as *const _, libc::RTLD_NOW | libc::RTLD_GLOBAL);
            angle = false;
        }
        if lib.is_null() {
            return Err("Could not load EGL".to_string());
        }
        Ok((egl_fns(lib)?, angle, lib))
    }
}

#[cfg(target_os = "android")]
fn egl_fns(lib: *mut std::ffi::c_void) -> Result<EglFns, String> {
    unsafe {
        Ok(EglFns {
            choose_config: std::mem::transmute(load_required(lib, b"eglChooseConfig\0")?),
            create_context: std::mem::transmute(load_required(lib, b"eglCreateContext\0")?),
            create_pbuffer: std::mem::transmute(load_required(lib, b"eglCreatePbufferSurface\0")?),
            create_window: std::mem::transmute(load_required(lib, b"eglCreateWindowSurface\0")?),
            destroy_surface: std::mem::transmute(load_required(lib, b"eglDestroySurface\0")?),
            make_current: std::mem::transmute(load_required(lib, b"eglMakeCurrent\0")?),
            swap_buffers: std::mem::transmute(load_required(lib, b"eglSwapBuffers\0")?),
            swap_interval: std::mem::transmute(load_required(lib, b"eglSwapInterval\0")?),
            get_proc: std::mem::transmute(load_required(lib, b"eglGetProcAddress\0")?),
        })
    }
}

#[cfg(target_os = "android")]
fn load_required(lib: *mut std::ffi::c_void, name: &[u8]) -> Result<*mut std::ffi::c_void, String> {
    load_symbol(lib, name).ok_or_else(|| {
        format!(
            "Missing EGL symbol {}",
            String::from_utf8_lossy(name).trim_end_matches('\0')
        )
    })
}

#[cfg(target_os = "android")]
fn load_symbol(lib: *mut std::ffi::c_void, name: &[u8]) -> Option<*mut std::ffi::c_void> {
    let ptr = unsafe { libc::dlsym(lib, name.as_ptr() as *const _) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landscape_swaps_the_point_size() {
        let family = DeviceFamily::iPhone3GS;
        let portrait = size_for_orientation_from_size(
            family.portrait_size(),
            DeviceOrientation::Portrait,
            NonZeroU32::new(1).unwrap(),
        );
        let landscape = size_for_orientation_from_size(
            family.portrait_size(),
            DeviceOrientation::LandscapeLeft,
            NonZeroU32::new(1).unwrap(),
        );
        assert_eq!(portrait, (320, 480));
        assert_eq!(landscape, (480, 320));
    }

    #[test]
    fn orientation_codes_cover_every_device_orientation() {
        assert_eq!(orientation_code(DeviceOrientation::Portrait), 0);
        assert_eq!(orientation_code(DeviceOrientation::PortraitUpsideDown), 1);
        assert_eq!(orientation_code(DeviceOrientation::LandscapeLeft), 2);
        assert_eq!(orientation_code(DeviceOrientation::LandscapeRight), 3);
    }
}
