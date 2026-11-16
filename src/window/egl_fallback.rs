/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
/// Retry the entire EGL/context initialization, not just library loading.
pub(super) fn try_egl_backends<T>(
    use_angle: bool,
    mut initialize: impl FnMut(bool) -> Result<T, String>,
) -> Result<T, String> {
    if !use_angle {
        return initialize(false);
    }
    match initialize(true) {
        Ok(egl) => Ok(egl),
        Err(angle_error) => {
            log!("Vetus ANGLE initialization failed ({angle_error}); trying native GLES");
            initialize(false).map_err(|native_error| format!(
                "ANGLE initialization failed ({angle_error}); native GLES initialization failed ({native_error})"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_context_failure_retries_native_initialization() {
        let mut attempts = Vec::new();
        let result = try_egl_backends(true, |angle| {
            attempts.push(angle);
            if angle {
                Err("eglMakeCurrent: EGL_BAD_MATCH".into())
            } else {
                Ok("native")
            }
        });
        assert_eq!(result.unwrap(), "native");
        assert_eq!(attempts, [true, false]);
    }

    #[test]
    fn working_angle_does_not_initialize_native_driver() {
        let mut attempts = Vec::new();
        let result = try_egl_backends(true, |angle| {
            attempts.push(angle);
            Ok::<_, String>("angle")
        });
        assert_eq!(result.unwrap(), "angle");
        assert_eq!(attempts, [true]);
    }

    #[test]
    fn explicit_native_selection_does_not_probe_angle() {
        let mut attempts = Vec::new();
        let result = try_egl_backends(false, |angle| {
            attempts.push(angle);
            Err::<(), _>("native failed".into())
        });
        assert_eq!(result.unwrap_err(), "native failed");
        assert_eq!(attempts, [false]);
    }

    #[test]
    fn failed_fallback_preserves_both_driver_errors() {
        let error = try_egl_backends(true, |angle| {
            Err::<(), _>(
                if angle {
                    "ANGLE context failed"
                } else {
                    "native config failed"
                }
                .into(),
            )
        })
        .unwrap_err();
        assert!(error.contains("ANGLE context failed"));
        assert!(error.contains("native config failed"));
    }
}
