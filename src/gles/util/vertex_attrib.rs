/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Preserve guest vertex streams while drawing host presentation and overlays.

use crate::gles::{gles2_raw as gl, GLenum, GLint, GLuint, GLvoid, GLES};

struct AttribArray {
    index: GLuint,
    enabled: bool,
    buffer: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: u8,
    stride: GLint,
    pointer: *mut GLvoid,
    integer: bool,
}

/// `glVertexAttribPointer` stores the source buffer as well as the format and
/// pointer in each attribute. Restoring only ARRAY_BUFFER_BINDING and the enable
/// bits leaves guest draws reading the presenter's small quad VBO instead.
///
/// See Apple's OpenGL ES guide, "Consolidate Vertex Array State Changes":
/// https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/OpenGLES_ProgrammingGuide/TechniquesforWorkingwithVertexData/TechniquesforWorkingwithVertexData.html
/// and GLES 2.0 section 2.9 / table 6.2:
/// https://registry.khronos.org/OpenGL/specs/es/2.0/es_full_spec_2.0.pdf
/// Upstream's fixed-function presenter similarly restores client array state:
/// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/opengles/eagl.rs
pub(crate) struct SavedVertexAttribArrays<const N: usize> {
    array_buffer: GLuint,
    arrays: [AttribArray; N],
}

impl<const N: usize> SavedVertexAttribArrays<N> {
    /// The same GL context and vertex array object must remain current until
    /// `restore`. Only the specified slots may be modified in between.
    pub(crate) unsafe fn save(gles: &mut dyn GLES, indices: [GLuint; N]) -> Self {
        let mut array_buffer = 0;
        gles.GetIntegerv(gl::ARRAY_BUFFER_BINDING, &mut array_buffer);
        let es3 = gles.is_es3();
        let arrays = indices.map(|index| {
            let mut query = |pname| {
                let mut value = 0;
                gles.GetVertexAttribiv(index, pname, &mut value);
                value
            };
            let enabled = query(gl::VERTEX_ATTRIB_ARRAY_ENABLED) != 0;
            let buffer = query(gl::VERTEX_ATTRIB_ARRAY_BUFFER_BINDING) as GLuint;
            let size = query(gl::VERTEX_ATTRIB_ARRAY_SIZE);
            let type_ = query(gl::VERTEX_ATTRIB_ARRAY_TYPE) as GLenum;
            let normalized = query(gl::VERTEX_ATTRIB_ARRAY_NORMALIZED) as u8;
            let stride = query(gl::VERTEX_ATTRIB_ARRAY_STRIDE);
            let integer = es3 && query(crate::gles::gles30_raw::VERTEX_ATTRIB_ARRAY_INTEGER) != 0;
            let mut pointer = std::ptr::null_mut();
            gles.GetVertexAttribPointerv(index, gl::VERTEX_ATTRIB_ARRAY_POINTER, &mut pointer);
            AttribArray {
                index,
                enabled,
                buffer,
                size,
                type_,
                normalized,
                stride,
                pointer,
                integer,
            }
        });
        Self {
            array_buffer: array_buffer as GLuint,
            arrays,
        }
    }

    pub(crate) unsafe fn restore(self, gles: &mut dyn GLES) {
        for a in self.arrays {
            gles.BindBuffer(gl::ARRAY_BUFFER, a.buffer);
            if a.integer {
                gles.VertexAttribIPointer(a.index, a.size, a.type_, a.stride, a.pointer);
            } else {
                gles.VertexAttribPointer(
                    a.index,
                    a.size,
                    a.type_,
                    a.normalized,
                    a.stride,
                    a.pointer,
                );
            }
            if a.enabled {
                gles.EnableVertexAttribArray(a.index);
            } else {
                gles.DisableVertexAttribArray(a.index);
            }
        }
        gles.BindBuffer(gl::ARRAY_BUFFER, self.array_buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gles::{GLboolean, GLsizei};

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Array {
        enabled: bool,
        buffer: GLuint,
        size: GLint,
        type_: GLenum,
        normalized: GLboolean,
        stride: GLsizei,
        pointer: usize,
        integer: bool,
    }

    struct Driver {
        buffer: GLuint,
        arrays: [Array; 8],
        es3: bool,
    }

    #[allow(non_snake_case)]
    impl GLES for Driver {
        fn is_es3(&self) -> bool {
            self.es3
        }
        unsafe fn GetIntegerv(&mut self, pname: GLenum, value: *mut GLint) {
            assert_eq!(pname, gl::ARRAY_BUFFER_BINDING);
            *value = self.buffer as GLint;
        }
        unsafe fn GetVertexAttribiv(&mut self, i: GLuint, pname: GLenum, value: *mut GLint) {
            let a = self.arrays[i as usize];
            *value = match pname {
                gl::VERTEX_ATTRIB_ARRAY_ENABLED => a.enabled as GLint,
                gl::VERTEX_ATTRIB_ARRAY_BUFFER_BINDING => a.buffer as GLint,
                gl::VERTEX_ATTRIB_ARRAY_SIZE => a.size,
                gl::VERTEX_ATTRIB_ARRAY_TYPE => a.type_ as GLint,
                gl::VERTEX_ATTRIB_ARRAY_NORMALIZED => a.normalized as GLint,
                gl::VERTEX_ATTRIB_ARRAY_STRIDE => a.stride,
                crate::gles::gles30_raw::VERTEX_ATTRIB_ARRAY_INTEGER => {
                    assert!(self.es3, "ES 2 must not query ES 3 state");
                    a.integer as GLint
                }
                _ => panic!("unexpected attribute query {pname:#x}"),
            };
        }
        unsafe fn GetVertexAttribPointerv(
            &mut self,
            i: GLuint,
            pname: GLenum,
            p: *mut *mut GLvoid,
        ) {
            assert_eq!(pname, gl::VERTEX_ATTRIB_ARRAY_POINTER);
            *p = self.arrays[i as usize].pointer as *mut GLvoid;
        }
        unsafe fn BindBuffer(&mut self, target: GLenum, buffer: GLuint) {
            assert_eq!(target, gl::ARRAY_BUFFER);
            self.buffer = buffer;
        }
        unsafe fn VertexAttribPointer(
            &mut self,
            i: GLuint,
            size: GLint,
            type_: GLenum,
            normalized: GLboolean,
            stride: GLsizei,
            p: *const GLvoid,
        ) {
            let a = &mut self.arrays[i as usize];
            *a = Array {
                enabled: a.enabled,
                buffer: self.buffer,
                size,
                type_,
                normalized,
                stride,
                pointer: p as usize,
                integer: false,
            };
        }
        unsafe fn VertexAttribIPointer(
            &mut self,
            i: GLuint,
            size: GLint,
            type_: GLenum,
            stride: GLsizei,
            p: *const GLvoid,
        ) {
            assert!(self.es3);
            self.VertexAttribPointer(i, size, type_, 0, stride, p);
            self.arrays[i as usize].integer = true;
        }
        unsafe fn EnableVertexAttribArray(&mut self, i: GLuint) {
            self.arrays[i as usize].enabled = true;
        }
        unsafe fn DisableVertexAttribArray(&mut self, i: GLuint) {
            self.arrays[i as usize].enabled = false;
        }
    }

    fn guest_driver(es3: bool) -> Driver {
        Driver {
            buffer: 19,
            arrays: std::array::from_fn(|i| Array {
                enabled: i % 2 == 0,
                buffer: if i == 7 { 0 } else { i as GLuint + 10 },
                size: 4,
                type_: gl::UNSIGNED_BYTE,
                normalized: 1,
                stride: 36,
                pointer: if i == 7 { 0x1234_5678 } else { 20 + i * 4 },
                integer: false,
            }),
            es3,
        }
    }

    #[test]
    fn presenter_and_nested_overlay_restore_guest_vertex_streams() {
        // The presenter uses slots 6/7; an overlay may also use overlapping slots.
        // Both buffer offsets and client pointers, enabled and disabled, survive.
        let mut driver = guest_driver(false);
        let original = driver.arrays;
        unsafe {
            let present = SavedVertexAttribArrays::save(&mut driver, [6, 7]);
            driver.BindBuffer(gl::ARRAY_BUFFER, 99);
            for i in [6, 7] {
                driver.VertexAttribPointer(i, 2, gl::FLOAT, 0, 16, std::ptr::null());
                driver.EnableVertexAttribArray(i);
            }
            let after_present_setup = driver.arrays;
            let overlay = SavedVertexAttribArrays::save(&mut driver, [0, 6, 7]);
            driver.BindBuffer(gl::ARRAY_BUFFER, 100);
            for i in [0, 6, 7] {
                driver.VertexAttribPointer(i, 3, gl::SHORT, 0, 24, 8usize as _);
                driver.DisableVertexAttribArray(i);
            }
            overlay.restore(&mut driver);
            assert_eq!(driver.arrays, after_present_setup);
            assert_eq!(driver.buffer, 99);
            present.restore(&mut driver);
        }
        assert_eq!(driver.arrays, original);
        assert_eq!(driver.buffer, 19);
    }

    #[test]
    fn presenter_restores_es3_integer_attribute_format() {
        let mut driver = guest_driver(true);
        driver.arrays[6].integer = true;
        driver.arrays[6].normalized = 0;
        let original = driver.arrays;
        unsafe {
            let saved = SavedVertexAttribArrays::save(&mut driver, [6]);
            driver.BindBuffer(gl::ARRAY_BUFFER, 99);
            driver.VertexAttribPointer(6, 2, gl::FLOAT, 0, 16, std::ptr::null());
            saved.restore(&mut driver);
        }
        assert_eq!(driver.arrays, original);
        assert_eq!(driver.buffer, 19);
    }
}
