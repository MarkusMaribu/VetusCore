/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::*;
use std::collections::VecDeque;

#[derive(Default)]
struct Driver {
    allocation: Option<(GLenum, GLsizei, GLenum, GLsizei, GLsizei)>,
    errors: VecDeque<GLenum>,
}

impl GLES for Driver {
    unsafe fn GetIntegerv(&mut self, _: GLenum, output: *mut GLint) {
        *output = 7;
    }

    unsafe fn TexStorage2D(
        &mut self,
        target: GLenum,
        levels: GLsizei,
        format: GLenum,
        width: GLsizei,
        height: GLsizei,
    ) {
        // Model a host with immutable RGBA8 storage but no native PVRTC.
        if crate::gles::util::pvrtc_format_properties(format).is_some() {
            self.errors.push_back(gles11::INVALID_ENUM);
        } else if levels <= 0 || width <= 0 || height <= 0 {
            self.errors.push_back(gles11::INVALID_VALUE);
        } else if self.allocation.is_some()
            || levels > (32 - (width.max(height) as u32).leading_zeros()) as i32
        {
            self.errors.push_back(gles11::INVALID_OPERATION);
        } else {
            self.allocation = Some((target, levels, format, width, height));
        }
    }

    unsafe fn GetError(&mut self) -> GLenum {
        self.errors.pop_front().unwrap_or(gles11::NO_ERROR)
    }
}

#[test]
fn cube_storage_tracks_every_face_and_error_flags_remain_guest_visible() {
    let mut driver = Driver::default();
    driver.errors.push_back(gles11::INVALID_ENUM);
    let mut shadow = GLShadowState::default();
    let format = gles11::COMPRESSED_RGB_PVRTC_2BPPV1_IMG;
    unsafe {
        texture_storage_2d(&mut driver, &mut shadow, TEXTURE_CUBE_MAP, 4, format, 8, 8);
    }
    for face in TEXTURE_CUBE_MAP_POSITIVE_X..=TEXTURE_CUBE_MAP_NEGATIVE_Z {
        assert_eq!(shadow.pvrtc_texture_level(face, 7, 3), Some((1, 1, format)));
    }
    assert_eq!(
        unsafe { guest_gl_error(&mut driver, &mut shadow) },
        gles11::INVALID_ENUM
    );
    assert_eq!(
        unsafe { guest_gl_error(&mut driver, &mut shadow) },
        gles11::NO_ERROR
    );
    // Reallocating immutable storage fails and must preserve the old metadata.
    unsafe {
        texture_storage_2d(
            &mut driver,
            &mut shadow,
            TEXTURE_CUBE_MAP,
            1,
            gles11::RGBA8_OES,
            16,
            16,
        );
    }
    assert_eq!(
        shadow.pvrtc_texture_level(TEXTURE_CUBE_MAP_POSITIVE_X, 7, 0),
        Some((8, 8, format))
    );
    assert_eq!(
        unsafe { guest_gl_error(&mut driver, &mut shadow) },
        gles11::INVALID_OPERATION
    );
    assert_eq!(
        unsafe { guest_gl_error(&mut driver, &mut shadow) },
        gles11::NO_ERROR
    );
}

#[test]
fn invalid_pvrtc_storage_does_not_create_levels_or_hide_errors() {
    for (levels, width, height, error) in [
        (0, 8, 8, gles11::INVALID_VALUE),
        (1, 7, 8, gles11::INVALID_VALUE),
        (1, 8, 0, gles11::INVALID_VALUE),
        (5, 8, 8, gles11::INVALID_OPERATION),
    ] {
        let mut driver = Driver::default();
        let mut shadow = GLShadowState::default();
        unsafe {
            texture_storage_2d(
                &mut driver,
                &mut shadow,
                gles11::TEXTURE_2D,
                levels,
                gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG,
                width,
                height,
            );
        }
        assert!(driver.allocation.is_none());
        assert!(shadow.pvrtc_texture_levels.is_empty());
        assert_eq!(unsafe { guest_gl_error(&mut driver, &mut shadow) }, error);
        assert_eq!(
            unsafe { guest_gl_error(&mut driver, &mut shadow) },
            gles11::NO_ERROR
        );
    }
}

#[test]
fn immutable_pvrtc_storage_allocates_decodable_mip_levels() {
    // Storage is immutable, with each mip level available for a full compressed
    // subimage upload. PVRTC subimages must cover the complete level.
    // https://registry.khronos.org/OpenGL/extensions/EXT/EXT_texture_storage.txt
    // https://registry.khronos.org/OpenGL/extensions/IMG/IMG_texture_compression_pvrtc.txt
    // https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/OpenGLES_ProgrammingGuide/AdoptingOpenGLES3/AdoptingOpenGLES3.html
    let mut driver = Driver::default();
    let mut shadow = GLShadowState::default();
    let format = gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG;
    unsafe {
        texture_storage_2d(
            &mut driver,
            &mut shadow,
            gles11::TEXTURE_2D,
            4,
            format,
            8,
            4,
        );
    }
    assert_eq!(
        driver.allocation,
        Some((gles11::TEXTURE_2D, 4, gles11::RGBA8_OES, 8, 4))
    );
    for (level, width, height) in [(0, 8, 4), (1, 4, 2), (2, 2, 1), (3, 1, 1)] {
        let stored = shadow.pvrtc_texture_level(gles11::TEXTURE_2D, 7, level);
        assert_eq!(stored, Some((width, height, format)));
        assert!(pvrtc_subimage_matches_level(
            stored, 0, 0, width, height, format
        ));
    }
    assert_eq!(unsafe { driver.GetError() }, gles11::NO_ERROR);
}
