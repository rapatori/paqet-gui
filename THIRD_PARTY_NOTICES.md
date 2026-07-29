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

The pinned upstream release archive does not include its `LICENSE` file, so this notice preserves the license text from the pinned source commit. A dependency-license inventory for the bundled executable must be completed before distribution because upstream does not publish an SBOM or dependency notice bundle.

## Bundled Fonts

The interface bundles Latin variable-font subsets retrieved from the official Google Fonts CSS service on 2026-07-29 for offline use:

- Space Grotesk, copyright 2020 The Space Grotesk Project Authors, licensed under the SIL Open Font License 1.1. Project source: <https://github.com/google/fonts/tree/main/ofl/spacegrotesk>. Exact Google Fonts artifact: <https://fonts.gstatic.com/s/spacegrotesk/v22/V8mDoQDjQSkFtoMM3T6r8E7mPbF4Cw.woff2>. Bundled artifact SHA-256: `0640890476fc1198ab4de571fb658de443c4d85b66466ec09534a8737ab1ce9d`.
- JetBrains Mono, copyright 2020 The JetBrains Mono Project Authors, licensed under the SIL Open Font License 1.1. Project source: <https://github.com/google/fonts/tree/main/ofl/jetbrainsmono>. Exact Google Fonts artifact: <https://fonts.gstatic.com/s/jetbrainsmono/v24/tDbv2o-flEEny0FZhsfKu5WU4zr3E_BX0PnT8RD8yKwBNntkaToggR7BYRbKPxDcwg.woff2>. Bundled artifact SHA-256: `83c005d49d8a6a50474c73a5a36ac0468076e9c4a29da7bdb14995d80560a5be`.

The complete licenses are stored in source as `src/assets/fonts/SPACE_GROTESK_OFL.txt` and `src/assets/fonts/JETBRAINS_MONO_OFL.txt`. Application bundles include this notice as `licenses/THIRD_PARTY_NOTICES.md` and both font licenses under `licenses/fonts/`.
