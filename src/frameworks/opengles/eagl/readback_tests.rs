/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use super::*;

#[test]
fn presentation_keeps_drawable_and_color_buffer_paired() {
    let layer = id::from_bits(0x1234);
    let other_layer = id::from_bits(0x5678);
    let mut bindings = HashMap::from([(7, layer)]);
    assert_eq!(resolve_present_drawable(&bindings, 7), Some((7, layer)));
    // A depth renderbuffer left bound must not replace the drawable's color
    // storage when applying the existing single-drawable fallback.
    assert_eq!(resolve_present_drawable(&bindings, 8), Some((7, layer)));
    bindings.insert(9, other_layer);
    assert_eq!(
        resolve_present_drawable(&bindings, 9),
        Some((9, other_layer))
    );
    assert_eq!(resolve_present_drawable(&bindings, 8), None);
    assert_eq!(resolve_present_drawable(&HashMap::new(), 8), None);
}

struct ReadbackDriver {
    renderbuffer: GLuint,
    framebuffer: GLuint,
    attachment: GLuint,
    temporary_deleted: bool,
}

#[allow(non_snake_case)]
impl GLES for ReadbackDriver {
    unsafe fn GetIntegerv(&mut self, pname: GLenum, value: *mut GLint) {
        *value = match pname {
            gles11::RENDERBUFFER_BINDING_OES => self.renderbuffer as GLint,
            gles11::FRAMEBUFFER_BINDING_OES => self.framebuffer as GLint,
            _ => panic!("unexpected query {pname:#x}"),
        };
    }
    unsafe fn BindRenderbufferOES(&mut self, target: GLenum, buffer: GLuint) {
        assert_eq!(target, gles11::RENDERBUFFER_OES);
        self.renderbuffer = buffer;
    }
    unsafe fn GetRenderbufferParameterivOES(
        &mut self,
        target: GLenum,
        pname: GLenum,
        value: *mut GLint,
    ) {
        assert_eq!(target, gles11::RENDERBUFFER_OES);
        let dimensions = if self.renderbuffer == 7 {
            (4, 2)
        } else {
            (1, 1)
        };
        *value = match pname {
            gles11::RENDERBUFFER_WIDTH_OES => dimensions.0,
            gles11::RENDERBUFFER_HEIGHT_OES => dimensions.1,
            _ => panic!("unexpected renderbuffer query {pname:#x}"),
        };
    }
    unsafe fn GetFramebufferAttachmentParameterivOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        pname: GLenum,
        value: *mut GLint,
    ) {
        assert_eq!(target, gles11::FRAMEBUFFER_OES);
        assert_eq!(attachment, gles11::COLOR_ATTACHMENT0_OES);
        assert_eq!(pname, gles11::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES);
        *value = self.attachment as GLint;
    }
    unsafe fn CheckFramebufferStatusOES(&mut self, _: GLenum) -> GLenum {
        gles11::FRAMEBUFFER_COMPLETE_OES
    }
    unsafe fn GenFramebuffersOES(&mut self, n: GLsizei, buffers: *mut GLuint) {
        assert_eq!(n, 1);
        *buffers = 99;
    }
    unsafe fn BindFramebufferOES(&mut self, _: GLenum, buffer: GLuint) {
        self.framebuffer = buffer;
    }
    unsafe fn FramebufferRenderbufferOES(
        &mut self,
        _: GLenum,
        _: GLenum,
        _: GLenum,
        buffer: GLuint,
    ) {
        assert_eq!(self.framebuffer, 99);
        assert_eq!(buffer, 7);
        self.attachment = buffer;
    }
    unsafe fn DeleteFramebuffersOES(&mut self, n: GLsizei, buffers: *const GLuint) {
        assert_eq!(n, 1);
        assert_eq!(*buffers, 99);
        self.temporary_deleted = true;
    }
    unsafe fn Finish(&mut self) {}
    unsafe fn ReadPixels(
        &mut self,
        x: GLint,
        y: GLint,
        width: GLsizei,
        height: GLsizei,
        format: GLenum,
        type_: GLenum,
        pixels: *mut GLvoid,
    ) {
        assert_eq!((x, y, width, height), (0, 0, 4, 2));
        assert_eq!((format, type_), (gles11::RGBA, gles11::UNSIGNED_BYTE));
        assert_eq!(self.attachment, 7);
        for (i, byte) in std::slice::from_raw_parts_mut(pixels.cast::<u8>(), 32)
            .iter_mut()
            .enumerate()
        {
            *byte = i as u8;
        }
    }
}

#[test]
fn readback_uses_color_dimensions_and_restores_bindings() {
    // The selected display color storage differs from the bound depth buffer.
    // Exercise both the app's complete FBO and the temporary-FBO copy route.
    for attachment in [7, 10] {
        let mut driver = ReadbackDriver {
            renderbuffer: 8,
            framebuffer: 12,
            attachment,
            temporary_deleted: false,
        };
        let (pixels, width, height) = unsafe { read_renderbuffer(&mut driver, 7, vec![255; 32]) };
        assert_eq!((width, height), (4, 2));
        assert_eq!(pixels, (0u8..32).collect::<Vec<_>>());
        assert_eq!(driver.renderbuffer, 8);
        assert_eq!(driver.framebuffer, 12);
        assert_eq!(driver.temporary_deleted, attachment != 7);
    }
}
