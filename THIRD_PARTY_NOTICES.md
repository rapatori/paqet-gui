# Third-Party Notices

## paqet

`paqet-gui` is an independent, unofficial companion for the `paqet` project. It is not affiliated with or endorsed by the upstream project.

- Project: `hanselime/paqet`
- Source: <https://github.com/hanselime/paqet>
- Version: `v1.0.0-alpha.20`
- Commit: `f8ee6c130b6d44664e737419e99f7f677a6cf03a`
- License: MIT

The following license applies to paqet:

```text
MIT License

Copyright (c) 2025 hanselime

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The pinned upstream release archive does not include its `LICENSE` file, so this notice preserves the license text from the pinned source commit. Upstream does not publish an SBOM or dependency notice bundle. `licenses/PAQET_THIRD_PARTY_LICENSES.txt` records the complete module inventory embedded in the pinned executable, the exact reviewed module versions and license texts, and the Go standard-library license.

## Application Runtime Dependencies

The Windows application and local frontend assets include open-source runtime dependencies in addition to paqet and the fonts below:

- `licenses/RUST_THIRD_PARTY_LICENSES.txt` is generated from the locked `x86_64-pc-windows-msvc` production dependency graph. Build-only and test-only crates are excluded.
- `licenses/FRONTEND_THIRD_PARTY_LICENSES.txt` covers the Svelte and Tauri API code included in the production frontend bundle. Build, lint, and test-only npm packages are excluded.

The application uses the separately installed Microsoft Edge WebView2 Evergreen Runtime. The `paqet-gui` installer does not redistribute, download, install, update, or repair WebView2. WebView2 remains Microsoft software under Microsoft's terms and is serviced independently from `paqet-gui`; Microsoft's official runtime download and repair page is <https://developer.microsoft.com/microsoft-edge/webview2/consumer/>.

## Bundled Fonts

The interface bundles Latin variable-font subsets retrieved from the official Google Fonts CSS service on 2026-07-29 for offline use:

- Space Grotesk, copyright 2020 The Space Grotesk Project Authors, licensed under the SIL Open Font License 1.1. Project source: <https://github.com/google/fonts/tree/main/ofl/spacegrotesk>. Exact Google Fonts artifact: <https://fonts.gstatic.com/s/spacegrotesk/v22/V8mDoQDjQSkFtoMM3T6r8E7mPbF4Cw.woff2>. Bundled artifact SHA-256: `0640890476fc1198ab4de571fb658de443c4d85b66466ec09534a8737ab1ce9d`.
- JetBrains Mono, copyright 2020 The JetBrains Mono Project Authors, licensed under the SIL Open Font License 1.1. Project source: <https://github.com/google/fonts/tree/main/ofl/jetbrainsmono>. Exact Google Fonts artifact: <https://fonts.gstatic.com/s/jetbrainsmono/v24/tDbv2o-flEEny0FZhsfKu5WU4zr3E_BX0PnT8RD8yKwBNntkaToggR7BYRbKPxDcwg.woff2>. Bundled artifact SHA-256: `83c005d49d8a6a50474c73a5a36ac0468076e9c4a29da7bdb14995d80560a5be`.

The complete licenses are stored in source as `src/assets/fonts/SPACE_GROTESK_OFL.txt` and `src/assets/fonts/JETBRAINS_MONO_OFL.txt`.

Application bundles install this notice, the `paqet-gui` MIT license, the paqet/frontend/Rust inventories above, and both font licenses under `licenses/`.
