/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Host integration for the embedded (no SDL) Vetus build.
//!
//! The Android launcher drives one session per process: it picks an IPA,
//! hands over an `ANativeWindow`, and parks the emulator while the surface
//! is gone. Callbacks are plain function pointers so the JNI crate can
//! install them without this module depending on JNI.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct HostFns {
    pub orientation: fn(i32),
    pub status_bar_hidden: fn(bool),
    pub keyboard: fn(bool, &str),
    pub idle_timer: fn(bool),
    pub open_url: fn(&str) -> bool,
    pub alert: fn(&str, &str, &[&str]) -> i32,
    pub crash: fn(&str),
    pub photo_picker: fn(u32) -> bool,
    pub save_image: fn(&str) -> bool,
    pub camera_available: fn(bool) -> bool,
    pub camera_flash_available: fn(bool) -> bool,
    pub camera_start: fn(bool, i32) -> bool,
    pub camera_stop: fn(),
    pub camera_frame: fn() -> Option<Vec<u8>>,
    pub camera_capture: fn(u32) -> bool,
}

fn noop_i32(_: i32) {}
fn noop_photo_picker(_: u32) -> bool { false }
fn noop_camera_query(_: bool) -> bool { false }
fn noop_camera_start(_: bool, _: i32) -> bool { false }
fn noop_camera_stop() {}
fn noop_camera_frame() -> Option<Vec<u8>> { None }
fn noop_bool(_: bool) {}
fn noop_keyboard(_: bool, _: &str) {}
fn noop_url(_: &str) -> bool {
    false
}
fn noop_alert(_: &str, _: &str, _: &[&str]) -> i32 {
    0
}
fn noop_crash(message: &str) {
    log!("Vetus crash (no host handler): {message}");
}

const NOOP_HOST: HostFns = HostFns {
    orientation: noop_i32,
    status_bar_hidden: noop_bool,
    keyboard: noop_keyboard,
    idle_timer: noop_bool,
    open_url: noop_url,
    alert: noop_alert,
    crash: noop_crash,
    photo_picker: noop_photo_picker,
    save_image: noop_url,
    camera_available: noop_camera_query,
    camera_flash_available: noop_camera_query,
    camera_start: noop_camera_start,
    camera_stop: noop_camera_stop,
    camera_frame: noop_camera_frame,
    camera_capture: noop_photo_picker,
};

fn host() -> &'static Mutex<HostFns> {
    static HOST: OnceLock<Mutex<HostFns>> = OnceLock::new();
    HOST.get_or_init(|| Mutex::new(NOOP_HOST))
}

pub fn set_host(fns: HostFns) {
    *host().lock().unwrap() = fns;
}

pub fn request_orientation(code: i32) {
    (host().lock().unwrap().orientation)(code);
}

pub fn notify_status_bar(hidden: bool) {
    (host().lock().unwrap().status_bar_hidden)(hidden);
}

pub fn notify_keyboard(show: bool) {
    let snapshot = keyboard_snapshot().lock().unwrap().clone();
    (host().lock().unwrap().keyboard)(show, &snapshot);
}

fn keyboard_snapshot() -> &'static Mutex<String> {
    static SNAPSHOT: OnceLock<Mutex<String>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| Mutex::new("{}".to_string()))
}

pub fn set_keyboard_snapshot(text: &str, password: bool, multiline: bool) {
    *keyboard_snapshot().lock().unwrap() = format!(
        "{{\"text\":{},\"password\":{password},\"multiline\":{multiline}}}",
        json_escape(text),
    );
}

pub fn notify_idle_timer(enabled: bool) {
    (host().lock().unwrap().idle_timer)(enabled);
}

pub fn open_url(url: &str) -> bool {
    (host().lock().unwrap().open_url)(url)
}

pub fn alert(title: &str, message: &str, buttons: &[&str]) -> i32 {
    (host().lock().unwrap().alert)(title, message, buttons)
}

pub fn report_crash(message: &str) {
    (host().lock().unwrap().crash)(message);
}

type PhotoPickerResult = (u32, Option<Vec<u8>>);
#[derive(Default)]
struct PhotoPickerMailbox {
    request: Option<u32>,
    result: Option<PhotoPickerResult>,
}
impl PhotoPickerMailbox {
    fn begin(&mut self, request: u32) {
        self.request = Some(request);
        self.result = None;
    }
    fn post(&mut self, request: u32, bytes: Option<Vec<u8>>) {
        if self.request == Some(request) && self.result.is_none() {
            self.result = Some((request, bytes));
        }
    }
    fn take(&mut self) -> Option<PhotoPickerResult> {
        let result = self.result.take()?;
        self.request = None;
        Some(result)
    }
}
static PHOTO_PICKER_RESULT: Mutex<PhotoPickerMailbox> = Mutex::new(PhotoPickerMailbox {
    request: None, result: None,
});

/// Start asynchronously: the emulation thread must keep servicing surfaces
/// while the host picker owns the screen.
pub fn request_photo_picker(request: u32) {
    PHOTO_PICKER_RESULT.lock().unwrap().begin(request);
    let callback = host().lock().unwrap().photo_picker;
    let allowed = camera_roll_allowed();
    let accepted = allowed && callback(request);
    log!("Photo picker request {request}: camera_roll_allowed={allowed}, host_accepted={accepted}");
    if !accepted {
        post_photo_picker_result(request, None);
    }
}

pub fn post_photo_picker_result(request: u32, bytes: Option<Vec<u8>>) {
    PHOTO_PICKER_RESULT.lock().unwrap().post(request, bytes);
}

pub fn take_photo_picker_result() -> Option<PhotoPickerResult> {
    PHOTO_PICKER_RESULT.lock().unwrap().take()
}

static CAMERA_RESULT: Mutex<PhotoPickerMailbox> = Mutex::new(PhotoPickerMailbox {
    request: None, result: None,
});

/// Hardware discovery does not open the camera or request permission.
pub fn camera_available(front: bool) -> bool {
    let callback = host().lock().unwrap().camera_available;
    callback(front)
}
pub fn camera_flash_available(front: bool) -> bool {
    let callback = host().lock().unwrap().camera_flash_available;
    callback(front)
}
pub fn start_camera(front: bool, flash: i32) -> bool {
    let callback = host().lock().unwrap().camera_start;
    camera_allowed() && callback(front, flash)
}
pub fn stop_camera() {
    *CAMERA_RESULT.lock().unwrap() = PhotoPickerMailbox::default();
    let callback = host().lock().unwrap().camera_stop;
    callback();
}
/// The host replaces its latest preview rather than accumulating queued frames.
pub fn take_camera_frame() -> Option<Vec<u8>> {
    if !camera_allowed() { return None; }
    let callback = host().lock().unwrap().camera_frame;
    callback()
}
pub fn request_camera_photo(request: u32) {
    CAMERA_RESULT.lock().unwrap().begin(request);
    let callback = host().lock().unwrap().camera_capture;
    if !camera_allowed() || !callback(request) {
        post_camera_photo_result(request, None);
    }
}
pub fn post_camera_photo_result(request: u32, bytes: Option<Vec<u8>>) {
    CAMERA_RESULT.lock().unwrap().post(request, bytes);
}
pub fn take_camera_photo_result() -> Option<PhotoPickerResult> {
    CAMERA_RESULT.lock().unwrap().take()
}

/// Export through a temporary file so large photos never cross Binder as bytes.
/// The host only adds the guest's own image; it does not read the photo library.
pub fn save_image(png: &[u8]) -> bool {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = user_data_path().join("photo_exports");
    if std::fs::create_dir_all(&directory).is_err() { return false; }
    let path = directory.join(format!("image-{}.png", NEXT.fetch_add(1, Ordering::Relaxed)));
    if std::fs::write(&path, png).is_err() {
        let _ = std::fs::remove_file(path);
        return false;
    }
    let callback = host().lock().unwrap().save_image;
    let saved = callback(&path.to_string_lossy());
    let _ = std::fs::remove_file(path);
    saved
}

fn path_slot() -> &'static Mutex<PathBuf> {
    static PATH: OnceLock<Mutex<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| Mutex::new(PathBuf::new()))
}

pub fn set_user_data_path(path: &Path) {
    *path_slot().lock().unwrap() = path.to_path_buf();
}

pub fn user_data_path() -> PathBuf {
    path_slot().lock().unwrap().clone()
}

fn screen_slot() -> &'static Mutex<(u32, u32)> {
    static SCREEN: OnceLock<Mutex<(u32, u32)>> = OnceLock::new();
    SCREEN.get_or_init(|| Mutex::new((0, 0)))
}

pub fn set_host_screen(width: u32, height: u32) {
    *screen_slot().lock().unwrap() = (width, height);
}

pub fn host_screen() -> Option<(u32, u32)> {
    let (w, h) = *screen_slot().lock().unwrap();
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

fn locale_slot() -> &'static Mutex<(String, String)> {
    static LOCALE: OnceLock<Mutex<(String, String)>> = OnceLock::new();
    LOCALE.get_or_init(|| Mutex::new(("en".to_string(), "US".to_string())))
}

pub fn set_locale(language: &str, country: &str) {
    *locale_slot().lock().unwrap() = (language.to_string(), country.to_string());
}

pub fn languages() -> Vec<String> {
    vec![locale_slot().lock().unwrap().0.clone()]
}

pub fn countries() -> Vec<String> {
    let country = locale_slot().lock().unwrap().1.clone();
    if country.is_empty() {
        Vec::new()
    } else {
        vec![country]
    }
}

fn battery_slot() -> &'static Mutex<(i32, i32)> {
    static BATTERY: OnceLock<Mutex<(i32, i32)>> = OnceLock::new();
    BATTERY.get_or_init(|| Mutex::new((100, 0)))
}

/// `state` matches [crate::window::BatteryState] discriminants: 0 unknown,
/// 1 on battery, 2 no battery, 3 charging, 4 full.
pub fn set_battery(percent: i32, state: i32) {
    *battery_slot().lock().unwrap() = (percent, state);
}

pub fn battery() -> (i32, i32) {
    *battery_slot().lock().unwrap()
}

#[derive(Clone)]
pub struct InputEvent {
    pub kind: InputKind,
}

#[derive(Clone)]
pub enum InputKind {
    Touch { action: i32, id: i64, x: f32, y: f32 },
    Text(String),
    EditingText(String),
    DismissKeyboard,
    Backspace,
    Return,
}

fn input_queue() -> &'static Mutex<VecDeque<InputEvent>> {
    static QUEUE: OnceLock<Mutex<VecDeque<InputEvent>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn push_input(event: InputEvent) {
    input_queue().lock().unwrap().push_back(event);
    wake();
}

pub fn drain_input() -> Vec<InputEvent> {
    input_queue().lock().unwrap().drain(..).collect()
}

fn accel_slot() -> &'static Mutex<(f32, f32, f32)> {
    static ACCEL: OnceLock<Mutex<(f32, f32, f32)>> = OnceLock::new();
    ACCEL.get_or_init(|| Mutex::new((0.0, 0.0, 9.80665)))
}

pub fn set_accel(x: f32, y: f32, z: f32) {
    *accel_slot().lock().unwrap() = (x, y, z);
}

pub fn accel() -> (f32, f32, f32) {
    *accel_slot().lock().unwrap()
}

fn gyro_slot() -> &'static Mutex<Option<(f32, f32, f32)>> {
    static GYRO: OnceLock<Mutex<Option<(f32, f32, f32)>>> = OnceLock::new();
    GYRO.get_or_init(|| Mutex::new(None))
}

pub fn set_gyro(x: f32, y: f32, z: f32) {
    *gyro_slot().lock().unwrap() = Some((x, y, z));
}

pub fn gyro() -> Option<(f32, f32, f32)> {
    *gyro_slot().lock().unwrap()
}

struct RunState {
    paused: bool,
    surface: SurfaceState,
    quit: bool,
}

#[derive(Default)]
struct SurfaceState {
    window: usize,
    generation: u64,
    released: u64,
}

impl SurfaceState {
    fn replace(&mut self, window: usize) -> u64 {
        // Duplicate pauses must share a fence: the emulator may already be
        // parked after acknowledging the first clear.
        if self.window != window {
            self.window = window;
            self.generation += 1;
        }
        self.generation
    }

    fn acknowledge(&mut self, generation: u64) {
        self.released = self.released.max(generation);
    }
}

fn run_state() -> &'static (Mutex<RunState>, Condvar) {
    static STATE: OnceLock<(Mutex<RunState>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| {
        (
            Mutex::new(RunState {
                paused: true,
                surface: SurfaceState::default(),
                quit: false,
            }),
            Condvar::new(),
        )
    })
}

fn wake() {
    run_state().1.notify_all();
}

pub fn set_paused(paused: bool) {
    run_state().0.lock().unwrap().paused = paused;
    wake();
}

pub fn is_paused() -> bool {
    run_state().0.lock().unwrap().paused
}

pub fn set_native_window(window: usize) -> u64 {
    let generation = run_state().0.lock().unwrap().surface.replace(window);
    wake();
    generation
}

pub fn native_window() -> usize {
    native_window_snapshot().0
}

pub fn native_window_snapshot() -> (usize, u64) {
    let state = run_state().0.lock().unwrap();
    (state.surface.window, state.surface.generation)
}

/// The render thread has destroyed any EGL surface older than this snapshot.
pub fn acknowledge_native_window(generation: u64) {
    run_state().0.lock().unwrap().surface.acknowledge(generation);
    wake();
}

/// Called off the Android UI thread before releasing a retired SurfaceTexture.
/// https://developer.android.com/reference/android/view/TextureView.SurfaceTextureListener
pub fn wait_for_surface_release(generation: u64) {
    let (lock, cv) = run_state();
    let mut state = lock.lock().unwrap();
    while !state.quit && state.surface.released < generation {
        state = cv.wait(state).unwrap();
    }
}

pub fn should_quit() -> bool {
    run_state().0.lock().unwrap().quit
}

/// Block the emulator thread while the user is away or the surface is gone.
pub fn wait_until_runnable() {
    let (lock, cv) = run_state();
    let mut state = lock.lock().unwrap();
    while !state.quit && (state.paused || state.surface.window == 0) {
        state = cv.wait(state).unwrap();
    }
}

pub fn is_runnable() -> bool {
    let state = run_state().0.lock().unwrap();
    state.quit || (!state.paused && state.surface.window != 0)
}

struct Frame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn frame_slot() -> &'static Mutex<Option<Frame>> {
    static FRAME: OnceLock<Mutex<Option<Frame>>> = OnceLock::new();
    FRAME.get_or_init(|| Mutex::new(None))
}

pub fn store_frame(width: u32, height: u32, rgba: Vec<u8>) {
    *frame_slot().lock().unwrap() = Some(Frame { width, height, rgba });
}

pub fn write_frame_bmp(path: &Path) -> bool {
    let frame = frame_slot().lock().unwrap();
    let Some(frame) = frame.as_ref() else {
        return false;
    };
    match std::fs::write(path, rgba_to_bmp(frame.width, frame.height, &frame.rgba)) {
        Ok(()) => true,
        Err(err) => {
            log!("Could not write Vetus frame: {err}");
            false
        }
    }
}

pub fn frame_bmp_bytes() -> Option<Vec<u8>> {
    let frame = frame_slot().lock().unwrap();
    frame
        .as_ref()
        .map(|frame| rgba_to_bmp(frame.width, frame.height, &frame.rgba))
}

fn resource_reader() -> &'static Mutex<fn(&str) -> Option<Vec<u8>>> {
    static READER: OnceLock<Mutex<fn(&str) -> Option<Vec<u8>>>> = OnceLock::new();
    READER.get_or_init(|| Mutex::new(|_| None))
}

pub fn set_resource_reader(reader: fn(&str) -> Option<Vec<u8>>) {
    *resource_reader().lock().unwrap() = reader;
}

pub fn read_resource(path: &str) -> Option<Vec<u8>> {
    (resource_reader().lock().unwrap())(path)
}

fn session_thread() -> &'static Mutex<Option<JoinHandle<()>>> {
    static SESSION: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Start the emulator on a background thread. One session per process.
pub fn start_session(ipa: &Path, data_dir: &Path, options_json: &str) -> Result<(), String> {
    let mut slot = session_thread().lock().unwrap();
    if slot.is_some() {
        return Err("A Vetus session is already running in this process".to_string());
    }
    set_user_data_path(data_dir);
    std::fs::create_dir_all(data_dir).map_err(|err| err.to_string())?;
    apply_screen_from_json(options_json);
    let ipa = ipa.to_path_buf();
    let options_json = options_json.to_string();
    let handle = std::thread::Builder::new()
        .name("vetus-emu".to_string())
        .spawn(move || {
            let args = session_args(&ipa, &options_json);
            match crate::main(args.into_iter()) {
                Ok(termination) => log!("Vetus session ended ({termination:?})"),
                Err(err) => report_crash(&err),
            }
        })
        .map_err(|err| err.to_string())?;
    *slot = Some(handle);
    Ok(())
}

pub fn request_quit() {
    {
        let mut state = run_state().0.lock().unwrap();
        state.quit = true;
        state.paused = false;
    }
    push_input(InputEvent {
        kind: InputKind::Return,
    });
    wake();
}

/// Ask the guest to quit and wait until the session thread leaves.
pub fn join_session(timeout: Duration) -> bool {
    request_quit();
    let handle = session_thread().lock().unwrap().take();
    let Some(handle) = handle else {
        return true;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    rx.recv_timeout(timeout).is_ok()
}

/// Resident memory limit in bytes for this process; 0 disables the guard.
static MEMORY_LIMIT_BYTES: AtomicU64 = AtomicU64::new(0);
static MEMORY_LIMIT_HIT: AtomicBool = AtomicBool::new(false);
static CAMERA_ALLOWED: AtomicBool = AtomicBool::new(false);
static MICROPHONE_ALLOWED: AtomicBool = AtomicBool::new(false);
static CAMERA_ROLL_ALLOWED: AtomicBool = AtomicBool::new(false);

pub fn camera_roll_allowed() -> bool { CAMERA_ROLL_ALLOWED.load(Ordering::Relaxed) }

pub fn camera_allowed() -> bool { CAMERA_ALLOWED.load(Ordering::Relaxed) }
pub fn microphone_allowed() -> bool { MICROPHONE_ALLOWED.load(Ordering::Relaxed) }

fn media_permissions(json: &str) -> (bool, bool) {
    (json_bool(json, "cameraAccess"), json_bool(json, "microphoneAccess"))
}
const MEMORY_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Resident set size of this process in bytes, if the host can tell us.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
        Some(pages * page_size)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        None
    }
}

/// Called once per run loop iteration on every thread. Every
/// [`MEMORY_LOG_INTERVAL`] it logs how much memory the process, the guest
/// heap and the Objective-C object table use, so growth is visible in
/// logcat, and it enforces the optional `memoryLimitMb` guard: a runaway
/// guest is stopped with a clear error instead of being left to push the
/// whole phone into the low-memory killer.
pub fn watch_memory(env: &mut crate::Environment) {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    if env.current_thread != 0 {
        return;
    }
    {
        let mut last = LAST.lock().unwrap();
        let now = std::time::Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MEMORY_LOG_INTERVAL) {
            return;
        }
        *last = Some(now);
    }

    const MB: u64 = 1024 * 1024;
    let rss = resident_bytes();
    let (chunks, guest_bytes) = env.mem.used_stats();
    log!(
        "Memory: rss={} MiB, guest_used={} MiB in {} blocks, objc_objects={}",
        rss.map_or(0, |bytes| bytes / MB),
        guest_bytes / MB,
        chunks,
        env.objc.object_count(),
    );

    let limit = MEMORY_LIMIT_BYTES.load(Ordering::Relaxed);
    if let (true, Some(rss)) = (limit > 0, rss) {
        if rss > limit && !MEMORY_LIMIT_HIT.swap(true, Ordering::Relaxed) {
            let message = format!(
                "The app used too much memory ({} MiB, limit {} MiB) and was stopped.",
                rss / MB,
                limit / MB
            );
            log!("{message}");
            report_crash(&message);
            request_quit();
        }
    }
}

fn apply_screen_from_json(json: &str) {
    let (camera, microphone) = media_permissions(json);
    CAMERA_ALLOWED.store(camera, Ordering::Relaxed);
    MICROPHONE_ALLOWED.store(microphone, Ordering::Relaxed);
    CAMERA_ROLL_ALLOWED.store(json_bool(json, "cameraRollAccess"), Ordering::Relaxed);
    MEMORY_LIMIT_HIT.store(false, Ordering::Relaxed);
    MEMORY_LIMIT_BYTES.store(
        u64::from(json_u32(json, "memoryLimitMb").unwrap_or(0)) * 1024 * 1024,
        Ordering::Relaxed,
    );
    if let (Some(w), Some(h)) = (json_u32(json, "screenWidth"), json_u32(json, "screenHeight")) {
        set_host_screen(w, h);
    }
}

fn session_args(ipa: &Path, json: &str) -> Vec<String> {
    let mut args = vec![
        "vetus".to_string(),
        ipa.to_string_lossy().into_owned(),
        "--fullscreen".to_string(),
        // Embedded sessions fit the guest display unless the host opts into stretching.
        "--preserve-aspect-ratio".to_string(),
    ];
    if let Some(family) = json_string(json, "deviceFamily") {
        args.push(format!("--device-family={family}"));
    }
    if let Some(scale) = json_u32(json, "scaleHack") {
        if scale > 0 {
            args.push(format!("--scale-hack={scale}"));
        }
    }
    if json_bool(json, "glesNative") {
        args.push("--gles-native".to_string());
    }
    if let (Some(w), Some(h)) = (json_u32(json, "screenWidth"), json_u32(json, "screenHeight")) {
        args.push(format!("--screen-size={w}x{h}"));
    }
    if let Some(extra) = json_string(json, "extraArgs") {
        args.extend(extra.split_whitespace().map(|part| part.to_string()));
    }
    // Alpine does not expose links or social account sign-in. Apply after extraArgs.
    args.push("--disable-links".to_string());
    args
}

/// Read an IPA without starting the emulator. The JSON always has `ok`.
pub fn inspect_ipa(path: &Path) -> String {
    match inspect_ipa_inner(path) {
        Ok(json) => json,
        Err(err) => format!(
            "{{\"ok\":false,\"error\":{}}}",
            json_escape(&err)
        ),
    }
}

fn inspect_ipa_inner(path: &Path) -> Result<String, String> {
    let bundle_data = crate::fs::BundleData::open_any(path)?;
    let (bundle, fs) = match crate::bundle::Bundle::new_bundle_and_fs_from_host_path(
        bundle_data,
        true,
    ) {
        Ok(opened) => opened,
        Err(err) => {
            let encrypted = err.to_lowercase().contains("encrypt");
            let arm32 = !(err.contains("32-bit")
                || err.contains("ARM")
                || err.contains("architecture")
                || err.contains("64"));
            return Ok(format!(
                "{{\"ok\":{},\"encrypted\":{},\"arm32\":{},\"error\":{}}}",
                if encrypted || !arm32 { "false" } else { "false" },
                encrypted,
                arm32 && !encrypted,
                json_escape(&err)
            ));
        }
    };
    let name = {
        let display = bundle.display_name();
        if display.is_empty() {
            bundle.bundle_name().to_string()
        } else {
            display.to_string()
        }
    };
    let bundle_id = bundle.bundle_identifier().to_string();
    let orientations = bundle.supported_interface_orientations();
    let portrait = orientations.iter().any(|o| o.contains("Portrait"));
    let landscape = orientations.iter().any(|o| o.contains("Landscape"));
    let caps = u32::from(portrait) | (u32::from(landscape) << 1);
    let hidden = bundle.status_bar_hidden();
    let icon = bundle.load_icon(&fs).ok().map(|image| {
        let (w, h) = image.dimensions();
        base64(&rgba_to_bmp(w, h, image.pixels()))
    });
    let icon_json = icon
        .map(|data| format!(",\"iconBmpBase64\":{data}"))
        .unwrap_or_default();
    Ok(format!(
        "{{\"ok\":true,\"encrypted\":false,\"arm32\":true,\"name\":{},\"bundleId\":{},\"orientations\":{caps},\"statusBarHidden\":{hidden}{icon_json}}}",
        json_escape(&name),
        json_escape(&bundle_id),
    ))
}

fn json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn json_u32(json: &str, key: &str) -> Option<u32> {
    let pattern = format!("\"{key}\"");
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn json_bool(json: &str, key: &str) -> bool {
    let pattern = format!("\"{key}\"");
    json.find(&pattern)
        .map(|index| json[index + pattern.len()..].trim_start().trim_start_matches(':').trim_start().starts_with("true"))
        .unwrap_or(false)
}

fn json_escape(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("\"");
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    if i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        out.push('=');
    }
    out.push('"');
    out
}

pub fn rgba_to_bmp(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row_stride = ((width * 3 + 3) & !3) as usize;
    let pixel_bytes = row_stride * height as usize;
    let header = 54u32;
    let mut out = vec![0u8; header as usize + pixel_bytes];
    out[0] = b'B';
    out[1] = b'M';
    let size = out.len() as u32;
    out[2..6].copy_from_slice(&size.to_le_bytes());
    out[10..14].copy_from_slice(&header.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&width.to_le_bytes());
    out[22..26].copy_from_slice(&height.to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    out[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
    for y in 0..height as usize {
        let src_y = height as usize - 1 - y;
        for x in 0..width as usize {
            let src = (src_y * width as usize + x) * 4;
            if src + 2 >= rgba.len() {
                continue;
            }
            let dst = header as usize + y * row_stride + x * 3;
            out[dst] = rgba[src + 2];
            out[dst + 1] = rgba[src + 1];
            out[dst + 2] = rgba[src];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_release_waits_for_render_thread_and_survives_reopen() {
        let mut surface = SurfaceState::default();
        surface.replace(10);
        let close = surface.replace(0);
        assert!(surface.released < close);
        assert_eq!(surface.replace(0), close);
        // Reopening before the renderer sees the clear must not lose the
        // retirement fence for the old texture.
        let reopen = surface.replace(20);
        assert!(surface.released < close);
        surface.acknowledge(reopen);
        assert!(surface.released >= close);
        assert_eq!(surface.window, 20);
        // A late acknowledgement cannot revoke a completed release.
        surface.acknowledge(close);
        assert_eq!(surface.released, reopen);
    }

    #[test]
    fn texture_owner_is_not_released_until_the_renderer_acknowledges() {
        use std::sync::mpsc::{channel, RecvTimeoutError};
        set_native_window(10);
        let close = set_native_window(0);
        assert_eq!(set_native_window(0), close);
        let (started_tx, started_rx) = channel();
        let (released_tx, released_rx) = channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            wait_for_surface_release(close);
            released_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        assert_eq!(
            released_rx.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Timeout)
        );
        let reopen = set_native_window(20);
        assert_eq!(
            released_rx.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Timeout)
        );
        acknowledge_native_window(reopen);
        released_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
        let close = set_native_window(0);
        acknowledge_native_window(close);
    }

    #[test]
    fn bmp_header_round_trips_dimensions() {
        let bmp = rgba_to_bmp(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[18..22].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bmp[22..26].try_into().unwrap()), 1);
    }

    #[test]
    fn options_json_maps_to_touchhle_args() {
        let args = session_args(
            Path::new("/tmp/game.ipa"),
            "{\"deviceFamily\":\"iphone-4\",\"scaleHack\":2,\"screenWidth\":1080,\"screenHeight\":1920,\"glesNative\":false}",
        );
        assert!(args.iter().any(|arg| arg == "--device-family=iphone-4"));
        assert!(args.iter().any(|arg| arg == "--scale-hack=2"));
        assert!(args.iter().any(|arg| arg == "--screen-size=1080x1920"));
        assert!(!args.iter().any(|arg| arg == "--gles-native"));
        assert_eq!(args.last().map(String::as_str), Some("--disable-links"));
    }

    #[test]
    fn media_permissions_default_deny_and_are_independent() {
        assert_eq!(media_permissions("{}"), (false, false));
        assert_eq!(media_permissions("{\"cameraAccess\":true}"), (true, false));
        assert_eq!(media_permissions("{\"microphoneAccess\":true}"), (false, true));
        assert_eq!(media_permissions("{\"cameraAccess\":false,\"microphoneAccess\":false}"), (false, false));
    }

    #[test]
    fn photo_picker_ignores_late_and_duplicate_results() {
        let mut mailbox = PhotoPickerMailbox::default();
        mailbox.begin(1);
        assert!(mailbox.take().is_none());
        mailbox.begin(2);
        mailbox.post(1, None);
        assert!(mailbox.take().is_none());
        mailbox.post(2, Some(vec![1, 2, 3]));
        mailbox.post(2, None);
        assert_eq!(mailbox.take(), Some((2, Some(vec![1, 2, 3]))));
        mailbox.post(2, Some(vec![4]));
        assert!(mailbox.take().is_none());
        mailbox.begin(3);
        mailbox.post(3, None);
        assert_eq!(mailbox.take(), Some((3, None)));
    }

    #[test]
    fn embedded_display_fits_by_default_and_stretch_overrides_it() {
        for stretch in [false, true] {
            let json = if stretch {
                "{\"extraArgs\":\"--allow-network-access --no-preserve-aspect-ratio\"}"
            } else {
                "{}"
            };
            let mut options = crate::options::Options::default();
            for arg in session_args(Path::new("/tmp/game.ipa"), json).iter().skip(2) {
                assert!(options.parse_argument(arg).unwrap());
            }
            assert_eq!(options.preserve_aspect_ratio, !stretch);
            assert_eq!(options.network_access, stretch);
        }
    }
}
