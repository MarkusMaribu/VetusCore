/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Coordinate conversions shared by window presentation and input.

use super::DeviceOrientation;
use crate::frameworks::core_graphics::CGRect;
use crate::matrix::Matrix;

/// One coordinate-space policy for UIKit geometry, presentation and input.
/// iOS 8 moved screen/window coordinates into the interface orientation;
/// physical display dimensions and motion sensor coordinates stay fixed.
/// https://developer.apple.com/documentation/uikit/uicoordinatespace
#[derive(Clone, Copy)]
pub(super) struct ScreenCoordinates {
    interface_oriented: bool,
}

impl ScreenCoordinates {
    pub(super) fn new(options: &crate::options::Options) -> Self {
        Self {
            interface_oriented: options.interface_oriented_screen_coordinates(),
        }
    }

    pub(super) fn size(self, portrait: (u32, u32), orientation: DeviceOrientation) -> (u32, u32) {
        if self.interface_oriented
            && matches!(
                orientation,
                DeviceOrientation::LandscapeLeft | DeviceOrientation::LandscapeRight
            )
        {
            (portrait.1, portrait.0)
        } else {
            portrait
        }
    }

    pub(super) fn rotation(self, orientation: DeviceOrientation) -> Matrix<2> {
        if self.interface_oriented {
            return Matrix::identity();
        }
        use std::f32::consts::{FRAC_PI_2, PI};
        match orientation {
            DeviceOrientation::Portrait => Matrix::identity(),
            DeviceOrientation::PortraitUpsideDown => Matrix::z_rotation(PI),
            DeviceOrientation::LandscapeLeft => Matrix::z_rotation(-FRAC_PI_2),
            DeviceOrientation::LandscapeRight => Matrix::z_rotation(FRAC_PI_2),
        }
    }

    pub(super) fn frame_to_viewport(
        self,
        frame: CGRect,
        portrait: (u32, u32),
        orientation: DeviceOrientation,
        viewport: (u32, u32, u32, u32),
    ) -> (i32, i32, i32, i32) {
        let (width, height) = self.size(portrait, orientation);
        let (vx, vy, vw, vh) = viewport;
        if width == 0 || height == 0 || vw == 0 || vh == 0 {
            return (0, 0, 0, 0);
        }
        let rotation = self.rotation(orientation);
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for x in [frame.origin.x, frame.origin.x + frame.size.width] {
            for y in [frame.origin.y, frame.origin.y + frame.size.height] {
                let point = rotation.transform([x / width as f32 - 0.5, y / height as f32 - 0.5]);
                for axis in 0..2 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
        }
        (
            (vx as f32 + (low[0] + 0.5) * vw as f32).round() as i32,
            (vy as f32 + (low[1] + 0.5) * vh as f32).round() as i32,
            ((high[0] - low[0]) * vw as f32).round().max(0.0) as i32,
            ((high[1] - low[1]) * vh as f32).round().max(0.0) as i32,
        )
    }
}

/// Convert SDL window coordinates to drawable pixels. Query both sizes from
/// the same window; their ratio can change when moving between displays.
pub(super) fn window_to_drawable(
    (x, y): (f32, f32),
    (window_width, window_height): (u32, u32),
    (drawable_width, drawable_height): (u32, u32),
) -> (f32, f32) {
    if window_width == 0 || window_height == 0 {
        return (0.0, 0.0);
    }
    (
        x * drawable_width as f32 / window_width as f32,
        y * drawable_height as f32 / window_height as f32,
    )
}

/// Center the app in the drawable, preserving its aspect ratio.
pub(super) fn fit_viewport(
    (app_width, app_height): (u32, u32),
    (screen_width, screen_height): (u32, u32),
) -> (u32, u32, u32, u32) {
    if app_width == 0 || app_height == 0 || screen_width == 0 || screen_height == 0 {
        return (0, 0, 0, 0);
    }
    let app_aspect = app_width as f32 / app_height as f32;
    let screen_aspect = screen_width as f32 / screen_height as f32;
    let (scaled_width, scaled_height) = if app_aspect < screen_aspect {
        (
            (screen_height as f32 * app_aspect).round() as u32,
            screen_height,
        )
    } else {
        (
            screen_width,
            (screen_width as f32 / app_aspect).round() as u32,
        )
    };
    let x = (screen_width - scaled_width) / 2;
    let y = (screen_height - scaled_height) / 2;
    (x, y, scaled_width, scaled_height)
}

/// Choose the same presentation region for rendering and touch input.
#[cfg(any(feature = "embedded", test))]
pub(super) fn display_viewport(
    app: (u32, u32),
    screen: (u32, u32),
    preserve_aspect_ratio: bool,
) -> (u32, u32, u32, u32) {
    if preserve_aspect_ratio {
        fit_viewport(app, screen)
    } else {
        (0, 0, screen.0, screen.1)
    }
}

/// Map coordinates to a unit square centered on the viewport. Coordinates
/// outside the viewport are clamped to its edges (-0.5 and 0.5).
/// An empty viewport maps to the center, avoiding division by zero while hidden.
pub(super) fn normalize_viewport_coords(
    (x, y): (f32, f32),
    (vx, vy, vw, vh): (u32, u32, u32, u32),
) -> (f32, f32) {
    if vw == 0 || vh == 0 {
        return (0.0, 0.0);
    }
    (
        ((x - vx as f32) / vw as f32).clamp(0.0, 1.0) - 0.5,
        ((y - vy as f32) / vh as f32).clamp(0.0, 1.0) - 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_screen_coordinates_keep_rendering_and_input_in_the_same_space() {
        // These are screen-space contracts, not device-rotation overrides.
        // https://developer.apple.com/documentation/uikit/uicoordinatespace
        let portrait = (320, 568);
        let landscape = DeviceOrientation::LandscapeLeft;
        let mut options = crate::options::Options::default();
        for (version, sdk, expected_size, top_left) in [
            (None, None, (320, 568), [320.0, 0.0]),
            (None, Some((7, 1, 0)), (320, 568), [320.0, 0.0]),
            (None, Some((8, 0, 0)), (568, 320), [0.0, 0.0]),
            (Some((7, 1, 2)), Some((8, 0, 0)), (320, 568), [320.0, 0.0]),
            (Some((8, 0, 0)), Some((7, 1, 0)), (320, 568), [320.0, 0.0]),
            (Some((8, 0, 0)), Some((8, 0, 0)), (568, 320), [0.0, 0.0]),
            (Some((12, 0, 0)), Some((9, 0, 0)), (568, 320), [0.0, 0.0]),
        ] {
            options.ios_version = version;
            options.linked_sdk_version = sdk;
            let coordinates = ScreenCoordinates::new(&options);
            assert_eq!(coordinates.size(portrait, landscape), expected_size);
            for orientation in [
                DeviceOrientation::Portrait,
                DeviceOrientation::PortraitUpsideDown,
            ] {
                assert_eq!(coordinates.size(portrait, orientation), portrait);
            }
            assert_eq!(
                coordinates.size(portrait, DeviceOrientation::LandscapeRight),
                expected_size
            );
            let mapped = coordinates
                .rotation(landscape)
                .inverse()
                .unwrap()
                .transform([-0.5, -0.5]);
            let actual = [
                (mapped[0] + 0.5) * expected_size.0 as f32,
                (mapped[1] + 0.5) * expected_size.1 as f32,
            ];
            for axis in 0..2 {
                assert!((actual[axis] - top_left[axis]).abs() < 0.001);
            }
            let full = CGRect {
                origin: Default::default(),
                size: crate::frameworks::core_graphics::CGSize {
                    width: expected_size.0 as f32,
                    height: expected_size.1 as f32,
                },
            };
            // A full-screen host control must cover exactly the same viewport
            // as the rendered scene, including letterbox offsets.
            assert_eq!(
                coordinates.frame_to_viewport(full, portrait, landscape, (100, 40, 1136, 640)),
                (100, 40, 1136, 640)
            );
        }
    }

    #[test]
    fn modern_and_legacy_host_control_rectangles_follow_their_screen_space() {
        use crate::frameworks::core_graphics::{CGPoint, CGSize};
        let frame = CGRect {
            origin: CGPoint { x: 20.0, y: 30.0 },
            size: CGSize {
                width: 40.0,
                height: 50.0,
            },
        };
        let mut options = crate::options::Options::default();
        options.ios_version = Some((8, 0, 0));
        options.linked_sdk_version = Some((8, 0, 0));
        assert_eq!(
            ScreenCoordinates::new(&options).frame_to_viewport(
                frame,
                (320, 480),
                DeviceOrientation::LandscapeLeft,
                (10, 20, 960, 640)
            ),
            (50, 80, 80, 100)
        );
        options.ios_version = Some((7, 1, 2));
        assert_eq!(
            ScreenCoordinates::new(&options).frame_to_viewport(
                frame,
                (320, 480),
                DeviceOrientation::LandscapeLeft,
                (10, 20, 960, 640)
            ),
            (70, 540, 100, 80)
        );
    }

    #[test]
    fn matching_sizes_preserve_coordinates() {
        assert_eq!(fit_viewport((480, 320), (480, 320)), (0, 0, 480, 320));
        assert_eq!(
            window_to_drawable((123.0, 45.0), (480, 320), (480, 320)),
            (123.0, 45.0)
        );
    }

    #[test]
    fn stretching_fills_portrait_and_landscape_and_keeps_touch_edges_aligned() {
        for app in [(320, 480), (320, 568), (768, 1024)] {
            for (app, screen) in [(app, (1080, 2200)), ((app.1, app.0), (2200, 1080))] {
                assert_eq!(
                    display_viewport(app, screen, true),
                    fit_viewport(app, screen)
                );
                let viewport = display_viewport(app, screen, false);
                assert_eq!(viewport, (0, 0, screen.0, screen.1));
                assert_eq!(
                    normalize_viewport_coords((0.0, 0.0), viewport),
                    (-0.5, -0.5)
                );
                assert_eq!(
                    normalize_viewport_coords((screen.0 as f32, screen.1 as f32), viewport),
                    (0.5, 0.5)
                );
                assert_eq!(
                    normalize_viewport_coords(
                        (screen.0 as f32 / 2.0, screen.1 as f32 / 2.0),
                        viewport,
                    ),
                    (0.0, 0.0)
                );
            }
        }
    }

    #[test]
    fn mouse_and_finger_agree_on_hidpi_viewport() {
        let viewport = fit_viewport((480, 320), (960, 640));
        assert_eq!(viewport, (0, 0, 960, 640));
        let mouse = window_to_drawable((120.0, 240.0), (480, 320), (960, 640));
        // SDL finger coordinates are normalized to the entire drawable.
        let finger = (0.25 * 960.0, 0.75 * 640.0);
        assert_eq!(mouse, finger);
        assert_eq!(normalize_viewport_coords(mouse, viewport), (-0.25, 0.25));
    }

    #[test]
    fn fractional_scale_uses_current_window_dimensions() {
        assert_eq!(
            window_to_drawable((400.0, 300.0), (800, 600), (1000, 750)),
            (500.0, 375.0)
        );
        assert_eq!(fit_viewport((480, 320), (1000, 750)), (0, 41, 1000, 667));
    }

    #[test]
    fn pillarbox_center_edges_and_bars() {
        let viewport = fit_viewport((320, 480), (1280, 960));
        assert_eq!(viewport, (320, 0, 640, 960));
        assert_eq!(
            normalize_viewport_coords((640.0, 480.0), viewport),
            (0.0, 0.0)
        );
        assert_eq!(
            normalize_viewport_coords((320.0, 480.0), viewport),
            (-0.5, 0.0)
        );
        assert_eq!(
            normalize_viewport_coords((960.0, 480.0), viewport),
            (0.5, 0.0)
        );
        assert_eq!(
            normalize_viewport_coords((0.0, 480.0), viewport),
            (-0.5, 0.0)
        );
        assert_eq!(
            normalize_viewport_coords((1280.0, 480.0), viewport),
            (0.5, 0.0)
        );
    }

    #[test]
    fn landscape_in_portrait_window_is_centered() {
        let viewport = fit_viewport((480, 320), (960, 1280));
        assert_eq!(viewport, (0, 320, 960, 640));
        assert_eq!(
            normalize_viewport_coords((480.0, 640.0), viewport),
            (0.0, 0.0)
        );
        assert_eq!(
            normalize_viewport_coords((480.0, 320.0), viewport),
            (0.0, -0.5)
        );
        assert_eq!(
            normalize_viewport_coords((480.0, 960.0), viewport),
            (0.0, 0.5)
        );
    }

    #[test]
    fn mouse_accelerometer_center_is_neutral_after_letterboxing() {
        let viewport = fit_viewport((480, 320), (960, 1280));
        let mouse = window_to_drawable((240.0, 320.0), (480, 640), (960, 1280));
        let (x, y) = normalize_viewport_coords(mouse, viewport);
        assert_eq!((x * 2.0, y * 2.0), (0.0, 0.0));
    }

    #[test]
    fn empty_dimensions_produce_finite_coordinates() {
        assert_eq!(window_to_drawable((1.0, 1.0), (0, 0), (0, 0)), (0.0, 0.0));
        assert_eq!(fit_viewport((480, 320), (0, 0)), (0, 0, 0, 0));
        assert_eq!(
            normalize_viewport_coords((1.0, 1.0), (0, 0, 0, 0)),
            (0.0, 0.0)
        );
    }
}
