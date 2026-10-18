/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! JNI surface for Alpine's Vetus app.
//!
//! One process loads this library and runs one IPA. The launcher process
//! never loads it: a bound service in `:vetusN` does, and renders into a
//! Surface the launcher hands over.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::sync::OnceLock;

const JNI_VERSION_1_6: c_int = 0x00010006;
const JNI_OK: c_int = 0;
const JNI_EDETACHED: c_int = -2;

const FIND_CLASS: usize = 6;
const EXCEPTION_CLEAR: usize = 17;
const NEW_GLOBAL_REF: usize = 21;
const DELETE_LOCAL_REF: usize = 23;
const GET_STATIC_METHOD_ID: usize = 113;
const CALL_STATIC_INT_METHOD_A: usize = 131;
const CALL_STATIC_BOOLEAN_METHOD_A: usize = 119;
const CALL_STATIC_VOID_METHOD_A: usize = 143;
const NEW_STRING_UTF: usize = 167;
const GET_STRING_UTF_CHARS: usize = 169;
const RELEASE_STRING_UTF_CHARS: usize = 170;

#[repr(C)]
#[derive(Clone, Copy)]
union JValue {
    i: c_int,
    z: u8,
    l: *mut c_void,
    _pad: u64,
}

struct Cache {
    class: *mut c_void,
    orientation: *mut c_void,
    status_bar: *mut c_void,
    keyboard: *mut c_void,
    idle_timer: *mut c_void,
    open_url: *mut c_void,
    alert: *mut c_void,
    crash: *mut c_void,
}

unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}

static VM: OnceLock<usize> = OnceLock::new();
static CACHE: OnceLock<Cache> = OnceLock::new();
static ASSETS: OnceLock<usize> = OnceLock::new();

fn vm() -> *mut c_void {
    *VM.get().unwrap_or(&0) as *mut c_void
}

fn java_string(env: *mut c_void, value: &str) -> *mut c_void {
    let Ok(text) = CString::new(value) else {
        return std::ptr::null_mut();
    };
    let Some(new_string): Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void> =
        (unsafe { touchHLE::android_jni::table_fn(env, NEW_STRING_UTF) })
    else {
        return std::ptr::null_mut();
    };
    unsafe { new_string(env, text.as_ptr()) }
}

fn rust_string(env: *mut c_void, value: *mut c_void) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let Some(get): Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> *const c_char,
    > = (unsafe { touchHLE::android_jni::table_fn(env, GET_STRING_UTF_CHARS) })
    else {
        return None;
    };
    let Some(release): Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char)> =
        (unsafe { touchHLE::android_jni::table_fn(env, RELEASE_STRING_UTF_CHARS) })
    else {
        return None;
    };
    unsafe {
        let mut copy = 0u8;
        let chars = get(env, value, &mut copy);
        if chars.is_null() {
            return None;
        }
        let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
        release(env, value, chars);
        Some(text)
    }
}

fn clear_exception(env: *mut c_void) {
    let Some(clear): Option<unsafe extern "C" fn(*mut c_void)> =
        (unsafe { touchHLE::android_jni::table_fn(env, EXCEPTION_CLEAR) })
    else {
        return;
    };
    unsafe { clear(env) };
}

fn delete_local(env: *mut c_void, object: *mut c_void) {
    if object.is_null() {
        return;
    }
    let Some(delete): Option<unsafe extern "C" fn(*mut c_void, *mut c_void)> =
        (unsafe { touchHLE::android_jni::table_fn(env, DELETE_LOCAL_REF) })
    else {
        return;
    };
    unsafe { delete(env, object) };
}

fn with_env<T>(f: impl FnOnce(*mut c_void) -> T) -> Option<T> {
    let vm = vm();
    if vm.is_null() {
        return None;
    }
    let env = attach(vm)?;
    Some(f(env))
}

fn attach(vm: *mut c_void) -> Option<*mut c_void> {
    let table = unsafe { *(vm as *mut *mut c_void) };
    if table.is_null() {
        return None;
    }
    let get_env_slot = unsafe { *(table as *mut *mut c_void).add(6) };
    let get_env: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, c_int) -> c_int =
        unsafe { std::mem::transmute_copy(&get_env_slot) };
    let mut env = std::ptr::null_mut();
    let rc = unsafe { get_env(vm, &mut env, JNI_VERSION_1_6) };
    if rc == JNI_OK && !env.is_null() {
        return Some(env);
    }
    if rc != JNI_EDETACHED {
        return None;
    }
    let attach_slot = unsafe { *(table as *mut *mut c_void).add(4) };
    let attach: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, *mut c_void) -> c_int =
        unsafe { std::mem::transmute_copy(&attach_slot) };
    let rc = unsafe { attach(vm, &mut env, std::ptr::null_mut()) };
    if rc == JNI_OK && !env.is_null() {
        Some(env)
    } else {
        None
    }
}

fn ensure_cache(env: *mut c_void) -> Option<&'static Cache> {
    if let Some(cache) = CACHE.get() {
        return Some(cache);
    }
    let Some(find): Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void> =
        (unsafe { touchHLE::android_jni::table_fn(env, FIND_CLASS) })
    else {
        return None;
    };
    let name = CString::new("com/markusmaribu/alpinelauncher/vetus/VetusCore").ok()?;
    let class = unsafe { find(env, name.as_ptr()) };
    if class.is_null() {
        clear_exception(env);
        return None;
    }
    let Some(global): Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void> =
        (unsafe { touchHLE::android_jni::table_fn(env, NEW_GLOBAL_REF) })
    else {
        return None;
    };
    let class = unsafe { global(env, class) };
    let method = |name: &str, sig: &str| -> *mut c_void {
        let Ok(name) = CString::new(name) else {
            return std::ptr::null_mut();
        };
        let Ok(sig) = CString::new(sig) else {
            return std::ptr::null_mut();
        };
        let Some(get): Option<
            unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char, *const c_char) -> *mut c_void,
        > = (unsafe { touchHLE::android_jni::table_fn(env, GET_STATIC_METHOD_ID) })
        else {
            return std::ptr::null_mut();
        };
        let id = unsafe { get(env, class, name.as_ptr(), sig.as_ptr()) };
        if id.is_null() {
            clear_exception(env);
        }
        id
    };
    let cache = Cache {
        class,
        orientation: method("onRequestOrientation", "(I)V"),
        status_bar: method("onStatusBarHidden", "(Z)V"),
        keyboard: method("onKeyboard", "(Z)V"),
        idle_timer: method("onIdleTimer", "(Z)V"),
        open_url: method("onOpenUrl", "(Ljava/lang/String;)Z"),
        alert: method("onAlert", "(Ljava/lang/String;Ljava/lang/String;)I"),
        crash: method("onCrash", "(Ljava/lang/String;)V"),
    };
    let _ = CACHE.set(cache);
    CACHE.get()
}

fn call_void(method: fn(&Cache) -> *mut c_void, args: &[JValue]) {
    let _ = with_env(|env| {
        let Some(cache) = ensure_cache(env) else {
            return;
        };
        let id = method(cache);
        if id.is_null() {
            return;
        }
        let Some(call): Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const JValue)> =
            (unsafe { touchHLE::android_jni::table_fn(env, CALL_STATIC_VOID_METHOD_A) })
        else {
            return;
        };
        unsafe { call(env, cache.class, id, args.as_ptr()) };
        clear_exception(env);
    });
}

fn host_orientation(code: i32) {
    call_void(|cache| cache.orientation, &[JValue { i: code }]);
}

fn host_status_bar(hidden: bool) {
    call_void(
        |cache| cache.status_bar,
        &[JValue { z: u8::from(hidden) }],
    );
}

fn host_keyboard(show: bool) {
    call_void(|cache| cache.keyboard, &[JValue { z: u8::from(show) }]);
}

fn host_idle(enabled: bool) {
    call_void(
        |cache| cache.idle_timer,
        &[JValue { z: u8::from(enabled) }],
    );
}

fn host_open_url(url: &str) -> bool {
    with_env(|env| {
        let Some(cache) = ensure_cache(env) else {
            return false;
        };
        if cache.open_url.is_null() {
            return false;
        }
        let text = java_string(env, url);
        let args = [JValue { l: text }];
        let Some(call): Option<
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const JValue) -> u8,
        > = (unsafe { touchHLE::android_jni::table_fn(env, CALL_STATIC_BOOLEAN_METHOD_A) })
        else {
            delete_local(env, text);
            return false;
        };
        let result = unsafe { call(env, cache.class, cache.open_url, args.as_ptr()) };
        delete_local(env, text);
        clear_exception(env);
        result != 0
    })
    .unwrap_or(false)
}

fn host_alert(title: &str, message: &str, _buttons: &[&str]) -> i32 {
    with_env(|env| {
        let Some(cache) = ensure_cache(env) else {
            return 0;
        };
        if cache.alert.is_null() {
            return 0;
        }
        let title = java_string(env, title);
        let message = java_string(env, message);
        let args = [JValue { l: title }, JValue { l: message }];
        let Some(call): Option<
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const JValue) -> c_int,
        > = (unsafe { touchHLE::android_jni::table_fn(env, CALL_STATIC_INT_METHOD_A) })
        else {
            delete_local(env, title);
            delete_local(env, message);
            return 0;
        };
        let result = unsafe { call(env, cache.class, cache.alert, args.as_ptr()) };
        delete_local(env, title);
        delete_local(env, message);
        clear_exception(env);
        result
    })
    .unwrap_or(0)
}

fn host_crash(message: &str) {
    with_env(|env| {
        let Some(cache) = ensure_cache(env) else {
            return;
        };
        if cache.crash.is_null() {
            return;
        }
        let text = java_string(env, message);
        let args = [JValue { l: text }];
        let Some(call): Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const JValue)> =
            (unsafe { touchHLE::android_jni::table_fn(env, CALL_STATIC_VOID_METHOD_A) })
        else {
            delete_local(env, text);
            return;
        };
        unsafe { call(env, cache.class, cache.crash, args.as_ptr()) };
        delete_local(env, text);
        clear_exception(env);
    });
}

fn install_host() {
    touchHLE::embed::set_host(touchHLE::embed::HostFns {
        orientation: host_orientation,
        status_bar_hidden: host_status_bar,
        keyboard: host_keyboard,
        idle_timer: host_idle,
        open_url: host_open_url,
        alert: host_alert,
        crash: host_crash,
    });
    touchHLE::embed::set_resource_reader(read_asset);
    let _ = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        touchHLE::embed::report_crash(&info.to_string());
    }));
}

fn read_asset(path: &str) -> Option<Vec<u8>> {
    let manager = *ASSETS.get()? as *mut c_void;
    if manager.is_null() {
        return None;
    }
    let name = CString::new(path).ok()?;
    unsafe {
        let asset = AAssetManager_open(manager, name.as_ptr(), 3);
        if asset.is_null() {
            return None;
        }
        let length = AAsset_getLength(asset);
        if length <= 0 || length > 64 * 1024 * 1024 {
            AAsset_close(asset);
            return None;
        }
        let mut bytes = vec![0u8; length as usize];
        let read = AAsset_read(asset, bytes.as_mut_ptr() as *mut c_void, length as usize);
        AAsset_close(asset);
        if read as i64 != length {
            return None;
        }
        Some(bytes)
    }
}

#[link(name = "android")]
unsafe extern "C" {
    fn AAssetManager_fromJava(env: *mut c_void, manager: *mut c_void) -> *mut c_void;
    fn AAssetManager_open(manager: *mut c_void, name: *const c_char, mode: c_int) -> *mut c_void;
    fn AAsset_getLength(asset: *mut c_void) -> i64;
    fn AAsset_read(asset: *mut c_void, buf: *mut c_void, count: usize) -> c_int;
    fn AAsset_close(asset: *mut c_void);
    fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut c_void;
}

fn install_vm(vm: *mut c_void) {
    let _ = VM.set(vm as usize);
    install_host();
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: *mut c_void, _reserved: *mut c_void) -> c_int {
    install_vm(vm);
    JNI_VERSION_1_6
}

fn require_env(env: *mut c_void) {
    if VM.get().is_none() {
        install_host();
    }
    let _ = env;
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeAvailable(
    env: *mut c_void,
    _class: *mut c_void,
) -> u8 {
    require_env(env);
    let _ = ensure_cache(env);
    1
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeInspectIpa(
    env: *mut c_void,
    _class: *mut c_void,
    path: *mut c_void,
) -> *mut c_void {
    require_env(env);
    let Some(path) = rust_string(env, path) else {
        return java_string(env, "{\"ok\":false,\"error\":\"missing path\"}");
    };
    let json = std::panic::catch_unwind(|| touchHLE::embed::inspect_ipa(Path::new(&path)))
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"inspect panicked\"}".to_string());
    java_string(env, &json)
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeCreate(
    env: *mut c_void,
    _class: *mut c_void,
    ipa: *mut c_void,
    data_dir: *mut c_void,
    options: *mut c_void,
) -> *mut c_void {
    require_env(env);
    let _ = ensure_cache(env);
    let (Some(ipa), Some(data_dir)) = (rust_string(env, ipa), rust_string(env, data_dir)) else {
        return java_string(env, "missing path");
    };
    let options = rust_string(env, options).unwrap_or_else(|| "{}".to_string());
    match touchHLE::embed::start_session(Path::new(&ipa), Path::new(&data_dir), &options) {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => java_string(env, &err),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeDestroy(
    _env: *mut c_void,
    _class: *mut c_void,
) {
    touchHLE::embed::request_quit();
    touchHLE::embed::set_paused(false);
    let _ = touchHLE::embed::join_session(std::time::Duration::from_secs(3));
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeSetSurface(
    env: *mut c_void,
    _class: *mut c_void,
    surface: *mut c_void,
) {
    if surface.is_null() {
        touchHLE::embed::set_native_window(0);
        return;
    }
    let window = unsafe { ANativeWindow_fromSurface(env, surface) };
    touchHLE::embed::set_native_window(window as usize);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeClearSurface(
    _env: *mut c_void,
    _class: *mut c_void,
) {
    touchHLE::embed::set_native_window(0);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeResize(
    _env: *mut c_void,
    _class: *mut c_void,
    _width: c_int,
    _height: c_int,
) {
    let window = touchHLE::embed::native_window();
    touchHLE::embed::set_native_window(window);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativePostTouch(
    _env: *mut c_void,
    _class: *mut c_void,
    action: c_int,
    pointer_id: i64,
    x: f32,
    y: f32,
) {
    touchHLE::embed::push_input(touchHLE::embed::InputEvent {
        kind: touchHLE::embed::InputKind::Touch {
            action,
            id: pointer_id,
            x,
            y,
        },
    });
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativePostAccel(
    _env: *mut c_void,
    _class: *mut c_void,
    x: f32,
    y: f32,
    z: f32,
) {
    touchHLE::embed::set_accel(x, y, z);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativePostGyro(
    _env: *mut c_void,
    _class: *mut c_void,
    x: f32,
    y: f32,
    z: f32,
) {
    touchHLE::embed::set_gyro(x, y, z);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativePostText(
    env: *mut c_void,
    _class: *mut c_void,
    text: *mut c_void,
) {
    let Some(text) = rust_string(env, text) else {
        return;
    };
    touchHLE::embed::push_input(touchHLE::embed::InputEvent {
        kind: touchHLE::embed::InputKind::Text(text),
    });
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativePostKey(
    _env: *mut c_void,
    _class: *mut c_void,
    key: c_int,
) {
    let kind = if key == 0 {
        touchHLE::embed::InputKind::Backspace
    } else {
        touchHLE::embed::InputKind::Return
    };
    touchHLE::embed::push_input(touchHLE::embed::InputEvent { kind });
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeSetPaused(
    _env: *mut c_void,
    _class: *mut c_void,
    paused: u8,
) {
    touchHLE::embed::set_paused(paused != 0);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeCopyLastFrame(
    env: *mut c_void,
    _class: *mut c_void,
    path: *mut c_void,
) -> u8 {
    let Some(path) = rust_string(env, path) else {
        return 0;
    };
    u8::from(touchHLE::embed::write_frame_bmp(Path::new(&path)))
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeSetBattery(
    _env: *mut c_void,
    _class: *mut c_void,
    percent: c_int,
    state: c_int,
) {
    touchHLE::embed::set_battery(percent, state);
}

#[no_mangle]
pub extern "system" fn Java_com_markusmaribu_alpinelauncher_vetus_VetusCore_nativeSetAssetManager(
    env: *mut c_void,
    _class: *mut c_void,
    manager: *mut c_void,
) {
    if manager.is_null() {
        return;
    }
    let native = unsafe { AAssetManager_fromJava(env, manager) };
    if !native.is_null() {
        let _ = ASSETS.set(native as usize);
    }
}
