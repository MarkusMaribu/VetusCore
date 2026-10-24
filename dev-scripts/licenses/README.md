# License supplements

These files supply terms omitted from some published dependency packages.
The collector uses version-specific mappings; review them when updating
dependencies. They do not change any component's license.

- `Apache-2.0.txt`: copied verbatim from
  <https://github.com/rust-windowing/gl-rs/blob/ea503e8d5fb6d73c6030e6191ce738cd3bf3433e/LICENSE>.
  Applies to `gl_generator` 0.14.0, `khronos_api` 3.1.0, and `mach_object`
  0.1.17, whose published manifests declare Apache-2.0. The latter's
  published source revision is `flier/rust-macho` commit
  `4bf5fb1f2a0ab02dc09e7f51a95a46e780b1354c`.
- `gl-rs-NOTICE.txt`: header copied from `gl_generator` 0.14.0 `lib.rs` and
  `khronos_api` 3.1.0 `src/lib.rs`. The collector separately includes the
  Khronos registry/IDL copyright comments.
- `winapi-MIT.txt`: copied verbatim from `winapi` 0.3.9's `LICENSE-MIT`;
  its companion architecture packages 0.4.0 declare MIT/Apache-2.0 and use
  the same winapi-rs repository. This bundle selects MIT for those packages.
- `openh264-BSD-2-Clause.txt`: preserves the wrapper author and license
  declaration from the pinned upstream README, with the linked standard
  BSD-2-Clause terms. The published package omits a standalone license file.
  Cisco's native codec notices are collected separately from `openh264-sys2`.
- `Boost.txt`: copied verbatim from `vendor/boost/LICENSE_1_0.txt`; the build
  downloads Boost headers separately, so packaging does not depend on that
  untracked directory being present.

`r-efi` 6.0.0 puts its license grant and notices in `AUTHORS`, which the
collector includes directly. The bundle preserves its MIT alternative;
its optional LGPL license does not require selecting LGPL.
