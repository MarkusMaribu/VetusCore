/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Model ANGLE's swap validation while exercising the real Egl handoff code.
//! https://github.com/google/angle/blob/main/src/libANGLE/validationEGL.cpp
//! ValidateSwapBuffers requires the requested surface to be current for drawing.

use super::*;
use std::ffi::c_void;

const CONTEXT: usize = 3;
const PBUFFER: usize = 2;

#[derive(Default)]
struct Driver {
    context: usize,
    draw_surface: usize,
    swaps: usize,
}

thread_local! {
    static DRIVER: RefCell<Driver> = RefCell::new(Driver::default());
}

unsafe extern "C" fn current_context() -> *mut c_void {
    DRIVER.with(|driver| driver.borrow().context as *mut _)
}

unsafe extern "C" fn make_current(
    _: *mut c_void,
    draw: *mut c_void,
    read: *mut c_void,
    context: *mut c_void,
) -> u32 {
    assert_eq!(draw, read);
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.context = context as usize;
        driver.draw_surface = draw as usize;
    });
    1
}

unsafe extern "C" fn swap(_: *mut c_void, surface: *mut c_void) -> u32 {
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        if driver.context == 0 || driver.draw_surface != surface as usize {
            return 0; // ANGLE reports EGL_BAD_SURFACE for this mismatch.
        }
        driver.swaps += 1;
        1
    })
}

unsafe extern "C" fn get_error() -> i32 {
    0x300d
}
unsafe extern "C" fn success(_: *mut c_void, _: *mut c_void) -> u32 {
    1
}
unsafe extern "C" fn terminate(_: *mut c_void) -> u32 {
    1
}
unsafe extern "C" fn interval(_: *mut c_void, _: i32) -> u32 {
    1
}
unsafe extern "C" fn config_attrib(_: *mut c_void, _: *mut c_void, _: i32, value: *mut i32) -> u32 {
    *value = 1;
    1
}
unsafe extern "C" fn choose_config(
    _: *mut c_void,
    _: *const i32,
    _: *mut *mut c_void,
    _: i32,
    _: *mut i32,
) -> u32 {
    0
}
unsafe extern "C" fn create_context(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *const i32,
) -> *mut c_void {
    CONTEXT as *mut _
}
unsafe extern "C" fn create_pbuffer(_: *mut c_void, _: *mut c_void, _: *const i32) -> *mut c_void {
    PBUFFER as *mut _
}
unsafe extern "C" fn create_window(
    _: *mut c_void,
    _: *mut c_void,
    window: *mut c_void,
    _: *const i32,
) -> *mut c_void {
    (window as usize + 100) as *mut _
}
unsafe extern "C" fn get_proc(_: *const i8) -> *mut c_void {
    std::ptr::null_mut()
}

#[allow(non_snake_case)]
pub(super) unsafe fn ANativeWindow_getWidth(_: *mut c_void) -> i32 {
    320
}
#[allow(non_snake_case)]
pub(super) unsafe fn ANativeWindow_getHeight(_: *mut c_void) -> i32 {
    480
}
#[allow(non_snake_case)]
pub(super) unsafe fn ANativeWindow_setBuffersGeometry(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: i32,
) -> i32 {
    0
}

fn egl() -> Egl {
    DRIVER.with(|driver| {
        *driver.borrow_mut() = Driver {
            context: CONTEXT,
            draw_surface: PBUFFER,
            swaps: 0,
        }
    });
    CURRENT_CONTEXT.with(|current| current.set(CONTEXT));
    Egl {
        angle: true,
        display: 1 as *mut _,
        config: 1 as *mut _,
        pbuffer: PBUFFER as *mut _,
        window_surface: std::ptr::null_mut(),
        window: std::ptr::null_mut(),
        read_pixels: None,
        contexts: RefCell::new(vec![CONTEXT]),
        fns: EglFns {
            get_error,
            get_current_context: current_context,
            get_config_attrib: config_attrib,
            destroy_context: success,
            terminate,
            choose_config,
            create_context,
            create_pbuffer,
            create_window,
            destroy_surface: success,
            make_current,
            swap_buffers: swap,
            swap_interval: interval,
            get_proc,
        },
    }
}

#[test]
fn first_swap_after_surface_attachment_uses_the_new_draw_surface() {
    let mut egl = egl();
    egl.adopt_window(10 as *mut _).unwrap();
    // iBeer reaches this swap before another GLES make_current call.
    egl.swap();
    DRIVER.with(|driver| assert_eq!(driver.borrow().swaps, 1));
}

#[test]
fn retiring_surface_keeps_context_current_on_the_pbuffer() {
    let mut egl = egl();
    egl.adopt_window(10 as *mut _).unwrap();
    egl.destroy_window_surface();
    DRIVER.with(|driver| {
        let driver = driver.borrow();
        assert_eq!(driver.context, CONTEXT);
        assert_eq!(driver.draw_surface, PBUFFER);
    });
}

#[test]
fn resumed_and_replaced_surfaces_can_swap_without_another_draw() {
    let mut egl = egl();
    egl.adopt_window(10 as *mut _).unwrap();
    egl.destroy_window_surface();
    egl.adopt_window(20 as *mut _).unwrap();
    egl.swap();
    egl.adopt_window(30 as *mut _).unwrap();
    egl.swap();
    DRIVER.with(|driver| assert_eq!(driver.borrow().swaps, 2));
}
