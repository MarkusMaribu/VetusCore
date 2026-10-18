/* Shared window types included by the SDL and embedded backends. */
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeviceFamily {
    iPhone,
    iPhone3G,
    iPhone3GS,
    iPhone4,
    iPhone4s,
    iPhone5,
    iPhone5c,
    iPad,
    iPad2,
    iPad3,
    iPad4,
    iPad5,
    iPadMini,
    iPadMini2,
    iPadMini3,
    iPodTouch,
    iPodTouch2,
    iPodTouch3,
    iPodTouch4,
    iPodTouch5,
}
impl std::fmt::Display for DeviceFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}
impl DeviceFamily {
    pub fn display_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iPhone",
            DeviceFamily::iPhone3G => "iPhone 3G",
            DeviceFamily::iPhone3GS => "iPhone 3GS",
            DeviceFamily::iPhone4 => "iPhone 4",
            DeviceFamily::iPhone4s => "iPhone 4s",
            DeviceFamily::iPhone5 => "iPhone 5",
            DeviceFamily::iPhone5c => "iPhone 5c",
            DeviceFamily::iPad => "iPad",
            DeviceFamily::iPad2 => "iPad 2",
            DeviceFamily::iPad3 => "iPad 3",
            DeviceFamily::iPad4 => "iPad 4",
            DeviceFamily::iPad5 => "iPad 5",
            DeviceFamily::iPadMini => "iPad mini",
            DeviceFamily::iPadMini2 => "iPad mini 2",
            DeviceFamily::iPadMini3 => "iPad mini 3",
            DeviceFamily::iPodTouch => "iPod touch",
            DeviceFamily::iPodTouch2 => "iPod touch 2",
            DeviceFamily::iPodTouch3 => "iPod touch 3",
            DeviceFamily::iPodTouch4 => "iPod touch 4",
            DeviceFamily::iPodTouch5 => "iPod touch 5",
        }
    }

    pub fn is_ipad(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPad
                | DeviceFamily::iPad2
                | DeviceFamily::iPad3
                | DeviceFamily::iPad4
                | DeviceFamily::iPad5
                | DeviceFamily::iPadMini
                | DeviceFamily::iPadMini2
                | DeviceFamily::iPadMini3
        )
    }

    pub fn is_ipod_touch(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPodTouch
                | DeviceFamily::iPodTouch2
                | DeviceFamily::iPodTouch3
                | DeviceFamily::iPodTouch4
                | DeviceFamily::iPodTouch5
        )
    }

    pub fn is_phone_568(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPhone5 | DeviceFamily::iPhone5c | DeviceFamily::iPodTouch5
        )
    }

    pub fn is_retina(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPhone4
                | DeviceFamily::iPhone4s
                | DeviceFamily::iPhone5
                | DeviceFamily::iPhone5c
                | DeviceFamily::iPodTouch4
                | DeviceFamily::iPodTouch5
                | DeviceFamily::iPad3
                | DeviceFamily::iPad4
                | DeviceFamily::iPad5
                | DeviceFamily::iPadMini2
                | DeviceFamily::iPadMini3
        )
    }

    /// Portrait (width, height) in logical points.
    pub fn portrait_size(&self) -> (u32, u32) {
        if self.is_ipad() {
            (768, 1024)
        } else if self.is_phone_568() {
            (320, 568)
        } else {
            (320, 480)
        }
    }

    /// UIScreen.scale — retina multiplier.
    pub fn scale_factor(&self) -> f32 {
        if self.is_retina() {
            2.0
        } else {
            1.0
        }
    }

    /// sysctl hw.machine / uname machine string.
    pub fn machine_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iPhone1,1",
            DeviceFamily::iPhone3G => "iPhone1,2",
            DeviceFamily::iPhone3GS => "iPhone2,1",
            DeviceFamily::iPhone4 => "iPhone3,1",
            DeviceFamily::iPhone4s => "iPhone4,1",
            DeviceFamily::iPhone5 => "iPhone5,1",
            DeviceFamily::iPhone5c => "iPhone5,3",
            DeviceFamily::iPad => "iPad1,1",
            DeviceFamily::iPad2 => "iPad2,1",
            DeviceFamily::iPad3 => "iPad3,1",
            DeviceFamily::iPad4 => "iPad3,4",
            DeviceFamily::iPad5 => "iPad6,11",
            DeviceFamily::iPadMini => "iPad2,5",
            DeviceFamily::iPadMini2 => "iPad4,4",
            DeviceFamily::iPadMini3 => "iPad4,7",
            DeviceFamily::iPodTouch => "iPod1,1",
            DeviceFamily::iPodTouch2 => "iPod2,1",
            DeviceFamily::iPodTouch3 => "iPod3,1",
            DeviceFamily::iPodTouch4 => "iPod4,1",
            DeviceFamily::iPodTouch5 => "iPod5,1",
        }
    }

    /// Amount of physical RAM (in bytes) the modelled device shipped with.
    ///
    /// Used by `NSProcessInfo.physicalMemory`, the `host_statistics`
    /// VM page counts and the `hw.memsize` / `hw.physmem` sysctls so that
    /// every memory-reporting API agrees with the chosen device family.
    /// Reporting too little (the old code reported ~50 MiB regardless of
    /// device) makes memory-budgeting engines such as Unreal Engine 3
    /// (UDKGame / Batman: Arkham City Lockdown) compute a bogus pool size and
    /// then attempt absurd allocations that fail, while reporting too much on
    /// a 32-bit address space risks overflow.
    /// Values come from Apple's published hardware specifications for each
    /// model.
    pub fn physical_memory(&self) -> u64 {
        const MIB: u64 = 1024 * 1024;
        match self {
            // Early devices shipped with 128 MiB of RAM.
            DeviceFamily::iPhone
            | DeviceFamily::iPhone3G
            | DeviceFamily::iPodTouch
            | DeviceFamily::iPodTouch2
            | DeviceFamily::iPodTouch3 => 128 * MIB,
            // iPhone 3GS / 4, iPad 1, iPod touch 4 shipped with 256 MiB.
            DeviceFamily::iPhone3GS
            | DeviceFamily::iPhone4
            | DeviceFamily::iPad
            | DeviceFamily::iPodTouch4 => 256 * MIB,
            // iPhone 4S / 5 / 5c, iPad 2/3/4/mini, iPod touch 5 shipped with
            // 512 MiB.
            DeviceFamily::iPhone4s
            | DeviceFamily::iPhone5
            | DeviceFamily::iPhone5c
            | DeviceFamily::iPad2
            | DeviceFamily::iPad3
            | DeviceFamily::iPad4
            | DeviceFamily::iPadMini
            | DeviceFamily::iPodTouch5 => 512 * MIB,
            // iPad 5 (2017) and the Retina iPad minis shipped with 1 GiB. We
            // intentionally cap this at 1 GiB even though the address space is
            // 4 GiB, leaving headroom for the guest heap, stacks and mapped
            // libraries.
            DeviceFamily::iPad5 | DeviceFamily::iPadMini2 | DeviceFamily::iPadMini3 => 1024 * MIB,
        }
    }

    /// Heuristically pick the emulated device family whose screen most closely
    /// matches an arbitrary host screen of `(width, height)` physical pixels.
    pub fn pick_for_screen(width: u32, height: u32) -> DeviceFamily {
        if width == 0 || height == 0 {
            return DeviceFamily::iPhone3GS;
        }
        let (short, long) = if width <= height {
            (width as f32, height as f32)
        } else {
            (height as f32, width as f32)
        };
        let host_ratio = short / long;
        const CANDIDATES: [DeviceFamily; 5] = [
            DeviceFamily::iPhone3GS,
            DeviceFamily::iPhone4,
            DeviceFamily::iPhone5,
            DeviceFamily::iPad2,
            DeviceFamily::iPad3,
        ];
        let mut best = DeviceFamily::iPhone3GS;
        let mut best_dist = f32::INFINITY;
        for family in CANDIDATES {
            let (w, h) = family.portrait_size();
            let ratio = w as f32 / h as f32;
            let dist = (ratio - host_ratio).abs();
            if dist < best_dist {
                best_dist = dist;
                best = family;
            }
        }
        best
    }

    /// CLI/option canonical name accepted by `--device-family=` and emitted by
    /// the app picker. Round-trips through `TryFrom<&str>`.
    pub fn option_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iphone-2g",
            DeviceFamily::iPhone3G => "iphone-3g",
            DeviceFamily::iPhone3GS => "iphone-3gs",
            DeviceFamily::iPhone4 => "iphone-4",
            DeviceFamily::iPhone4s => "iphone-4s",
            DeviceFamily::iPhone5 => "iphone-5",
            DeviceFamily::iPhone5c => "iphone-5c",
            DeviceFamily::iPad => "ipad-1",
            DeviceFamily::iPad2 => "ipad-2",
            DeviceFamily::iPad3 => "ipad-3",
            DeviceFamily::iPad4 => "ipad-4",
            DeviceFamily::iPad5 => "ipad-5",
            DeviceFamily::iPadMini => "ipad-mini",
            DeviceFamily::iPadMini2 => "ipad-mini-2",
            DeviceFamily::iPadMini3 => "ipad-mini-3",
            DeviceFamily::iPodTouch => "ipod-touch",
            DeviceFamily::iPodTouch2 => "ipod-touch-2",
            DeviceFamily::iPodTouch3 => "ipod-touch-3",
            DeviceFamily::iPodTouch4 => "ipod-touch-4",
            DeviceFamily::iPodTouch5 => "ipod-touch-5",
        }
    }

    /// Every device model the user can pick, in menu/display order. Used by the
    /// app picker's Quick Options "Device model" selector and by docs.
    pub const ALL_SELECTABLE: &'static [DeviceFamily] = &[
        DeviceFamily::iPhone,
        DeviceFamily::iPhone3G,
        DeviceFamily::iPhone3GS,
        DeviceFamily::iPhone4,
        DeviceFamily::iPhone4s,
        DeviceFamily::iPhone5,
        DeviceFamily::iPhone5c,
        DeviceFamily::iPad,
        DeviceFamily::iPad2,
        DeviceFamily::iPad3,
        DeviceFamily::iPad4,
        DeviceFamily::iPad5,
        DeviceFamily::iPadMini,
        DeviceFamily::iPadMini2,
        DeviceFamily::iPadMini3,
        DeviceFamily::iPodTouch,
        DeviceFamily::iPodTouch2,
        DeviceFamily::iPodTouch3,
        DeviceFamily::iPodTouch4,
        DeviceFamily::iPodTouch5,
    ];
}
impl TryFrom<u64> for DeviceFamily {
    type Error = ();
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DeviceFamily::iPhone),
            2 => Ok(DeviceFamily::iPad),
            _ => Err(()),
        }
    }
}
impl TryFrom<&str> for DeviceFamily {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_ascii_lowercase().as_str() {
            "iphone" => Ok(DeviceFamily::iPhone3GS),
            "iphone-2g" | "iphone1,1" => Ok(DeviceFamily::iPhone),
            "iphone-3g" | "iphone1,2" => Ok(DeviceFamily::iPhone3G),
            "iphone-3gs" | "iphone2,1" => Ok(DeviceFamily::iPhone3GS),
            "iphone-4" | "iphone3,1" => Ok(DeviceFamily::iPhone4),
            "iphone-4s" | "iphone4,1" => Ok(DeviceFamily::iPhone4s),
            "iphone-5" | "iphone5,1" => Ok(DeviceFamily::iPhone5),
            "iphone-5c" | "iphone5,3" => Ok(DeviceFamily::iPhone5c),
            "ipad" => Ok(DeviceFamily::iPad2),
            "ipad-1" | "ipad1,1" => Ok(DeviceFamily::iPad),
            "ipad-2" | "ipad2,1" => Ok(DeviceFamily::iPad2),
            "ipad-3" | "ipad3,1" => Ok(DeviceFamily::iPad3),
            "ipad-4" | "ipad3,4" => Ok(DeviceFamily::iPad4),
            "ipad-5" | "ipad6,11" => Ok(DeviceFamily::iPad5),
            "ipad-mini" | "ipad2,5" => Ok(DeviceFamily::iPadMini),
            "ipad-mini-2" | "ipad4,4" => Ok(DeviceFamily::iPadMini2),
            "ipad-mini-3" | "ipad4,7" => Ok(DeviceFamily::iPadMini3),
            "ipod-touch" | "ipod1,1" => Ok(DeviceFamily::iPodTouch),
            "ipod-touch-2" | "ipod2,1" => Ok(DeviceFamily::iPodTouch2),
            "ipod-touch-3" | "ipod3,1" => Ok(DeviceFamily::iPodTouch3),
            "ipod-touch-4" | "ipod4,1" => Ok(DeviceFamily::iPodTouch4),
            "ipod-touch-5" | "ipod5,1" => Ok(DeviceFamily::iPodTouch5),
            _ => Err(()),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DeviceOrientation {
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}
fn normalize_portrait_size(size: (u32, u32)) -> (u32, u32) {
    if size.0 <= size.1 {
        size
    } else {
        (size.1, size.0)
    }
}

fn size_for_orientation_from_size(
    size: (u32, u32),
    orientation: DeviceOrientation,
    scale_hack: NonZeroU32,
) -> (u32, u32) {
    let (width, height) = size;
    let scale_hack = scale_hack.get();
    match orientation {
        DeviceOrientation::Portrait => (width * scale_hack, height * scale_hack),
        DeviceOrientation::PortraitUpsideDown => (width * scale_hack, height * scale_hack),
        DeviceOrientation::LandscapeLeft => (height * scale_hack, width * scale_hack),
        DeviceOrientation::LandscapeRight => (height * scale_hack, width * scale_hack),
    }
}

fn size_for_orientation(
    family: DeviceFamily,
    orientation: DeviceOrientation,
    scale_hack: NonZeroU32,
) -> (u32, u32) {
    size_for_orientation_from_size(family.portrait_size(), orientation, scale_hack)
}
fn rotate_fullscreen_size(orientation: DeviceOrientation, screen_size: (u32, u32)) -> (u32, u32) {
    let (short_side, long_side) = if screen_size.0 < screen_size.1 {
        (screen_size.0, screen_size.1)
    } else {
        (screen_size.1, screen_size.0)
    };
    match orientation {
        DeviceOrientation::Portrait | DeviceOrientation::PortraitUpsideDown => {
            (short_side, long_side)
        }
        DeviceOrientation::LandscapeLeft | DeviceOrientation::LandscapeRight => {
            (long_side, short_side)
        }
    }
}
/// COMPAT: per-game accelerometer axis remap, applied to real-sensor data in
/// the hardware path of [Window::get_acceleration].
///
/// The values delivered to the guest are always in the device's portrait
/// frame (iOS semantics), which is correct for games that honour their
/// declared interface orientation. Some games, however, hard-code their
/// tilt math for one particular way of holding the phone — or the host
/// device reports sensors in a natural-orientation frame some games don't
/// expect (e.g. tablets) — and then steering/camera controls come out
/// mirrored or sideways (seen with e.g. Asphalt 7's tilt camera).
///
/// `TOUCHHLE_ACCELEROMETER_AXES` accepts a comma-separated list of:
/// - "swap": transpose x and y (sideways behaviour on some devices)
/// - "flipx": negate x (left/right inversion)
/// - "flipy": negate y (forward/backward inversion)
/// Both flips together make a 180-degree fix; all three together swap and
/// flip. Example: TOUCHHLE_ACCELEROMETER_AXES=swap,flipy
///
/// The knob is read once and cached. Gyroscope readings are NOT remapped.
fn accelerometer_compat_remap(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    use std::sync::OnceLock;
    static CACHE: OnceLock<(bool, bool, bool)> = OnceLock::new();
    let (swap, flipx, flipy) = *CACHE.get_or_init(|| {
        let var = std::env::var("TOUCHHLE_ACCELEROMETER_AXES").unwrap_or_default();
        let var = var.to_ascii_lowercase();
        (
            var.contains("swap"),
            var.contains("flipx"),
            var.contains("flipy"),
        )
    });
    let (mut x, mut y) = (x, y);
    if swap {
        std::mem::swap(&mut x, &mut y);
    }
    if flipx {
        x = -x;
    }
    if flipy {
        y = -y;
    }
    (x, y, z)
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FingerId {
    Mouse,
    Touch(i64),
    VirtualCursor,
    ButtonToTouch(crate::options::Button),
    StickToTouch,
    DpadToTouch,
}
pub type Coords = (f32, f32);

struct DpadState {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    active: bool,
}

#[derive(Debug)]
pub enum TextInputEvent {
    Text(String),
    Backspace,
    Return,
}

#[derive(Debug)]
pub enum Event {
    /// User requested quit.
    Quit,
    /// OS has informed touchHLE it will soon become inactive.
    /// (iOS `applicationWillResignActive:`, Android `onPause()`)
    AppWillResignActive,
    /// OS has informed touchHLE it has become inactive and entered the
    /// background. (iOS `applicationDidEnterBackground:`, Android
    /// `onPause()` completing / activity no longer visible)
    AppDidEnterBackground,
    /// OS has informed touchHLE it is about to leave the background and
    /// re-enter the foreground. (iOS `applicationWillEnterForeground:`,
    /// Android `onResume()` starting)
    AppWillEnterForeground,
    /// OS has informed touchHLE it has become active again after being
    /// inactive/backgrounded. (iOS `applicationDidBecomeActive:`)
    AppDidBecomeActive,
    /// OS has informed touchHLE it will soon terminate.
    /// (iOS `applicationWillTerminate:`, Android `onDestroy()`)
    AppWillTerminate,
    TouchesDown(HashMap<FingerId, Coords>),
    TouchesMove(HashMap<FingerId, Coords>),
    TouchesUp(HashMap<FingerId, Coords>),
    /// User pressed F12, requesting that execution be paused and the debugger
    /// take over.
    EnterDebugger,
    TextInput(TextInputEvent),
    /// Left (-1) or right (+1) arrow key was pressed. Used by the app picker
    /// to switch pages.
    ArrowKey(i8),
}

#[derive(Clone, Copy)]
pub enum BatteryState {
    Unknown,
    OnBattery,
    NoBattery,
    Charging,
    Full,
}

pub enum GLVersion {
    /// OpenGL ES 1.1
    GLES11,
    /// OpenGL ES 2.0
    GLES20,
    /// OpenGL ES 3.0
    GLES30,
    /// OpenGL 2.1 compatibility profile
    GL21Compat,
    /// OpenGL 3.3 Core profile
    GL33Core,
}
