# Licensing and distribution

The emulator's source code and VetusCore modifications to it remain under
[MPL 2.0](LICENSE). Preserve existing copyright and license notices. Files
with their own license notices and third-party components retain those terms;
the MPL does not replace their licenses. See [NOTICE](NOTICE) for the fork's
lineage, credits, and source location.

## Source and binary releases

Source for VetusCore is available at
<https://github.com/MarkusMaribu/VetusCore>. A binary release must identify
its exact source commit or tag and make the corresponding MPL-covered
source, including modifications, available to recipients. Include `LICENSE`,
`NOTICE`, this document, and third-party notices with the binary, including
when a host app distributes `libvetus_core.so`. A link to an unmodified
upstream project does not supply the fork's modified source.

The requirements follow MPL sections 3.1, 3.2, and 3.4; see also
[Mozilla's distribution FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).

`python dev-scripts/collect-licenses.py OUTPUT.txt` collects license texts
from the resolved Cargo packages and checked-out native dependencies. Run
it after fetching dependencies and initializing recursive submodules. It
includes build/development dependencies conservatively and records package
versions and license expressions. It is a notice collector, not a source
archive or a substitute for the obligations described below.

## Third-party components

- **Rust crates:** versions and sources are pinned in `Cargo.lock`;
  `Cargo.toml` files declare their licenses. The collector includes their
  supplied license and notice files, including native code inside crates.
  The patched AAC codec retains `vendor/symphonia-codec-aac/LICENSE`
  (MPL 2.0).
- **Dynarmic:** `vendor/dynarmic/LICENSE.txt` (0BSD) and the Legal section of
  its `README.md`; nested dependencies retain their own licenses under
  `vendor/dynarmic/externals/`.
- **OpenAL Soft:** `vendor/openal-soft/COPYING` (GNU Library GPL version 2;
  source headers permit version 2 or later). This fork uses the modified
  source at <https://github.com/MarkusMaribu/openal-soft>, pinned by the
  `vendor/openal-soft` Git submodule.
- **SDL:** `vendor/SDL/LICENSE.txt` (zlib), plus separate notices for bundled
  components such as HIDAPI. Android files adapted from SDL retain their
  SDL notices.
- **libxml2:** `vendor/libxml2/Copyright` and individual source notices
  (MIT-style terms).
- **stb:** `vendor/stb/LICENSE` and individual headers (MIT or public-domain
  alternatives).
- **PVRTDecompress:** `vendor/PVRTDecompress/LICENSE.md` (MIT).
- **Boost headers:** `dev-scripts/licenses/Boost.txt` (Boost Software License
  1.0), copied verbatim from `vendor/boost/LICENSE_1_0.txt`.
- **Fonts:** `touchHLE_fonts/LICENSE.liberation` and `LICENSE.noto` (SIL Open
  Font License 1.1); provenance is in `touchHLE_fonts/README.md`. The TestApp
  font has its notice in `tests/TestApp.app/LICENSE.liberation`.
  The legacy `Arial.ttf` and `helvetica-bold.ttf` paths now contain Alpine
  Emulation Sans, modified TeX Gyre Heros fonts under the GUST Font License
  (LPPL 1.3c or later), not proprietary Arial or Helvetica font software.
  Preserve `LICENSE.heros`, `LICENSE.lppl-1.3c`, and
  `README-Alpine-Emulation-Sans.txt`, including the modification log and
  original-source locations. Unmodified inputs are in `touchHLE_fonts/sources`.
- **Guest libraries:** preserve `touchHLE_dylibs/COPYING.*` and
  `touchHLE_dylibs/README.md`, which lists source locations and the GPL
  runtime exceptions for libgcc and libstdc++. These binaries are separate
  from the MPL-licensed emulator source.
- **Platform hosts and tools:** retain file-level notices, including SDL
  attribution in Android files and Applesauce attribution in
  `platform/ios/README.md`. `dev-scripts/gpl-3.0.txt` is retained for release
  packaging; it does not replace the emulator's MPL license.

## OpenAL Soft and other binary obligations

The default and embedded features statically link OpenAL Soft. Distributors
must satisfy its LGPL terms as well as the MPL: include the library's
copyright/license notices, provide the complete corresponding library
source (including the pinned fork's changes), and provide what recipients
need to modify the library and relink the application. For a fully open
build this includes the application source, dependency sources, build
scripts and instructions needed to rebuild/relink; a closed host may need
relinkable object files or a suitable replaceable shared-library design.
Do not restrict the modification/debugging rights granted by the LGPL.
See sections 4 and 6 of `vendor/openal-soft/COPYING`.

Record exact core and recursive submodule revisions (`git rev-parse HEAD`
and `git submodule status --recursive`), retain `Cargo.lock`, and supply
matching build instructions and dependency sources with each release.
`cargo vendor --locked` can collect Cargo dependency sources; an ordinary
GitHub source ZIP omits submodule contents. A notice bundle alone does not
fulfil source or relinking requirements.

If redistributing NDK runtime libraries such as `libc++_shared.so`, include
the notices for the NDK version used. Preserve any additional required
notices and source obligations of the selected dependencies. Open-source
copyright licenses do not by themselves resolve codec patent rights.

Local commercial apps, captured game assets, SDK downloads, and diagnostic
captures are excluded from commits; this repository's license does not
authorize redistributing them.
