# Prokoptas — Technical Specification

**CLI binary name: `prok`**

## 1. Overview

Prokoptas is a Rust image conversion and processing tool with two interfaces:

- **Web GUI** — runs entirely client-side via WebAssembly. No uploads, no server round-trip for conversion.
- **CLI (`prok`)** — native binary for batch/scripted workflows.

Both interfaces are thin shells around a single **Core Processing Library**. The Core owns all decoding, pixel transforms, and encoding for supported still-image formats; the shells only handle I/O, argument/UI binding, and orchestration. No conversion logic should live in either shell.

The project intentionally supports **still images only**. Multi-frame image workflows are out of scope for the current product.

**License: AGPL-3.0-only.** This is a hard constraint on crate selection, not a placeholder — see §6.

**Design priority: transparency about lossy vs. lossless.** Every format and every encode path in this spec is labeled with its actual lossless capability, not an assumed one. Several formats support both modes depending on options, while JPEG never supports lossless encoding. No UI control or CLI flag should imply losslessness that the underlying encoder does not actually deliver. See §3.2 and §4.

### Current implementation status

Phases 0–4 are currently implemented and verified for the Core model, PNG, JPEG, and WebP still-image paths. The CLI supports PNG, JPEG, and WebP input/output selected by format probe and output extension. JPEG output is lossy at the current default quality of 75; PNG output is lossless; WebP output defaults to lossy quality 75. The CLI does not yet expose the full option set listed in §5.1, including a user-selectable quality flag.

The remaining formats and the browser frontend are architectural targets and are not yet implemented.

---

## 2. Architecture

The workspace contains three Rust crates plus the web frontend:

- `prok-core` — the shared library containing format adapters, normalization, transforms, validation, errors, and encoding logic. It should remain as `no_std`-friendly as practical, but it may use `alloc` and the dependencies required by the selected codec crates.
- `prok-cli` — the native command-line shell that produces the `prok` binary. It depends on `prok-core` and is responsible for file-system I/O, command-line parsing, process exit codes, and terminal output.
- `prok-wasm` — the WebAssembly shell exposing `prok-core` through `wasm-bindgen` to JavaScript/TypeScript.
- `web/` — the browser frontend that consumes the generated WASM package. The frontend must not contain format conversion logic.

The runtime flow is always: input bytes → format probe → decoder → normalized `DecodedImage` representation → optional transforms → encoder → output bytes. Native and WASM shells use the same Core path and the same option model.

### 2.1 Crate layout (Cargo workspace)

The repository layout is:

- workspace root `Cargo.toml`
- `crates/prok-core/`
- `crates/prok-cli/`
- `crates/prok-wasm/`
- `web/`

The CLI crate produces the binary named `prok`. The WASM crate is a `cdylib` and provides bindings over the Core API.

### 2.2 Core data model

All decoders normalize to a single in-memory representation before any encoder touches the data.

`DecodedImage` contains:

- pixel data as RGBA8 or RGBA16, stored row-major;
- width and height as 32-bit unsigned dimensions;
- an explicit color-space value, including sRGB, Linear, Display P3, and future supported spaces;
- source/output bit depth metadata, including 8, 10, 12, and 16-bit values where supported;
- image orientation derived from EXIF, with an explicit indication of whether orientation has already been applied;
- retained image metadata covering EXIF, IPTC, and XMP until an encode path strips or rewrites it.

There is deliberately **no frame collection in the Core image representation** because Prokoptas is single-frame only.

`PixelBuffer` has two supported representations: RGBA8 using 8-bit components and RGBA16 using 16-bit components. Codec-specific pixel layouts are converted at the adapter boundary.

Every decoder exposes a decode operation plus a fast format-probe operation that can reject unrelated input without fully parsing the image. Every encoder exposes an encode operation plus a mandatory lossless-capability declaration. The declaration is the Core's source of truth for whether a target supports `Compression::Lossless`.

The conversion model is an M × N matrix: any registered decoder can feed the normalized representation, and any registered encoder can consume it. Adding a new format therefore requires one decoder adapter, one encoder adapter, or both, rather than pairwise conversion implementations.

### 2.3 Format registry

Decoders and encoders register into a static format registry keyed by a stable format tag. The CLI and WASM shells resolve the same format identifiers and the same target encoders.

Supported formats are:

- PNG
- JPEG
- TIFF
- BMP
- WebP, still images only
- HEIC/HEIF, decode-only
- AVIF, still images only
- JPEG XL, still images only
- camera RAW variants, including CR2, NEF, ARW, and DNG, decode-only

There is no general multi-frame format API exposed by Prokoptas.

RAW variants are represented as a family of decode-only inputs. There is no RAW/DNG encoder path.

---

## 3. Crate Matrix

All selected production codec paths must be pure Rust with no C/C++ FFI dependencies. Optional features that introduce native or C dependencies must remain disabled. Each selected crate should be verified for a clean `wasm32-unknown-unknown` build before it is committed to Core.

| Format | Decode crate | Encode crate | License | Notes |
|---|---|---|---|---|
| Core buffer/abstraction | `image` (0.25+) | — | MIT/Apache-2.0 | Central image/interchange dependency at trait boundaries; internal Core storage remains the custom normalized `PixelBuffer` model. |
| PNG | `png` | `png` | MIT/Apache-2.0 | Native Rust; PNG encoding is inherently lossless. |
| **JPEG** | `zune-jpeg` | `jpeg-encoder` | MIT/Apache-2.0/Zlib | Baseline + progressive decode only; no lossless JPEG input mode. JPEG output is always lossy regardless of quality setting. |
| TIFF | `tiff` | `tiff` | MIT/Apache-2.0 | Supported compression subset is baseline, LZW, and PackBits. No fax compression and no JPEG-in-TIFF path. |
| BMP | `image` (built-in) | `image` (built-in) | MIT/Apache-2.0 | Uncompressed and lossless. |
| **WebP** | `image-webp` 0.2.4 | **`zenwebp` 0.4.4** | MIT/Apache-2.0 for `image-webp`; AGPL-3.0-only OR LicenseRef-Imazen-Commercial for `zenwebp` | Still-image WebP only. `zenwebp` provides pure-Rust lossy VP8 and lossless VP8L encoding from RGBA, with explicit quality and effort controls. Animated WebP is rejected by the Core adapter. |
| **HEIC/HEIF** | **`heic` 0.1.6** | — | AGPL-3.0-only OR LicenseRef-Imazen-Commercial | Pure-Rust, still-image HEIC/HEIF decoder with no C/C++ dependencies. `#![forbid(unsafe_code)]`, `no_std + alloc` compatible, and supports RGBA8 decoding plus resource limits. HEIC encoding is out of scope. HEVC/H.265 patent rights are not granted by the crate's software license and must be treated as a separate legal consideration. |
| **AVIF** | `zenavif` 0.1.7 | `zenavif` 0.1.7 with `encode` | AGPL-3.0-only OR LicenseRef-Imazen-Commercial | Pure-Rust AVIF codec. Default decode path uses `rav1d-safe` and is safe Rust. Encoding is provided through `zenravif` when the `encode` feature is enabled. Prokoptas supports still images only even though the underlying codec can handle animation and other auxiliary image features. |
| **JXL** | `jxl-oxide` | `jxl-encoder` 0.3.1 | Decode: MIT/Apache-2.0. Encode: AGPL-3.0-only OR LicenseRef-Imazen-Commercial | Pure-Rust JPEG XL encoder supporting lossy VarDCT and true lossless Modular encoding. Prokoptas exposes still-image operation only. |
| RAW (decode only) | `rawler` | — | LGPL-2.1 | Decode-only camera RAW support. Pin the exact version and re-check its license on every upgrade. Confirm demosaic capabilities with real sample files before implementation is considered complete. |

### 3.1 Format-scope rules

Prokoptas is not required to support every feature that an underlying codec crate happens to expose. In particular, multi-frame APIs and other non-still-image facilities exposed by underlying codec crates are not part of the public Prokoptas format model.

HEIC/HEIF is an input format only. The supported workflow is HEIC/HEIF → decoded pixels/metadata → another supported output format. DNG is treated as one of the RAW-family inputs and is also decode-only.

### 3.2 Lossless Support Matrix

This is the canonical reference — every place in the UI or CLI that could imply “no quality loss” must trace back to this table, not to an assumption about the format name.

| Format | Decode | Encode | How losslessness is actually controlled |
|---|---|---|---|
| PNG | Lossless | **Always lossless** | No quality mode is needed; PNG has no lossy encode path. |
| JPEG | Lossy only | **Never lossless** | No setting can make JPEG output lossless. A quality value of 100 still does not become a lossless JPEG. |
| TIFF | Lossless | **Always lossless** under the supported compression subset | No separate lossless toggle is required. |
| BMP | Lossless | **Always lossless** | Uncompressed pixel storage. |
| WebP | Lossless or lossy | **Both** via `zenwebp` | Lossless and lossy are explicit encoder modes. The Core must never infer losslessness from a high quality value alone. |
| HEIC/HEIF | Lossy HEVC still-image decode | **Decode only** | HEIC/HEIF inputs are decoded to the normalized Core representation. There is no HEIC encoder path. HEVC patent/licensing status is separate from the crate's copyright license. |
| AVIF | Lossless or lossy | **Both** | `Compression::Lossless` must select the encoder's genuine lossless configuration; quality 100 in a lossy configuration is not equivalent to true lossless. |
| JXL | Lossless or lossy | **Both** | `Compression::Lossless` must select the encoder's true Modular/lossless configuration; a very low lossy distance is still lossy. |
| RAW | Sensor-data decode can be bit-exact | N/A — decode only | RAW sensor decoding can preserve the stored samples, but demosaic is an interpolation step. “High Quality” demosaic means a better interpolation, not a lossless reconstruction. |

**Concrete implication for `EncodeOptions`:** compression mode is explicit. The shared model uses either `Lossy` with a quality value from 1–100 or `Lossless`. `Lossless` is legal only when the selected target encoder reports that it supports true lossless output. JPEG must reject it rather than silently falling back to a high-quality lossy configuration.

### 3.3 Implementation verification items (resolve before first release)

- [ ] Confirm `rawler`'s demosaic capabilities (fast bilinear vs. adaptive) with a decode spike against 3–5 sample RAW files, with CR2, NEF, and DNG represented at minimum.
- [x] Verify that `zenwebp`, `zenavif`, `jxl-encoder`, and `heic` build cleanly for `wasm32-unknown-unknown` with only the project-approved pure-Rust feature set enabled.
- [ ] Keep `zenavif`'s native-only `unsafe-asm` feature disabled for the WASM build because it introduces C FFI. It may be evaluated separately for the native CLI only if the project accepts having different performance feature sets by platform.
- [ ] Confirm the exact public lossless configuration used by the selected `zenavif` and `jxl-encoder` versions before wiring their adapters to `Compression::Lossless`.
- [ ] Run a full dependency/license audit before the first release, including optional/transitive dependencies, and confirm that the production feature set contains no accidental C/C++ codec path.

---

## 4. Decode/Encode Options

These options are shared by the Core, CLI, and WASM UI. There must be one source of truth for validation and semantics; the shells may bind values differently, but they must not create parallel conversion logic.

### 4.1 Decode options

`DecodeOptions` contains:

- `strict_metadata` — whether CRC/checksum and similar metadata parse failures are rejected or tolerated;
- `color_space_override` — optional forced interpretation such as Linear or sRGB;
- `auto_rotate` — whether EXIF orientation is applied before the normalized image is returned;
- `demosaic_quality` — RAW-only choice between Fast and HighQuality; this controls interpolation quality and must never be labeled as lossless;
- `memory_limit_mb` — a decode-time memory cap that must be enforced before large pixel buffers are allocated.

There is deliberately no frame-selection option because Prokoptas supports still images only.

### 4.2 Encode options

`EncodeOptions` contains:

- `compression` — explicit Lossy with quality 1–100, or Lossless;
- `bit_depth` — 8, 10, or 12 where the target format and encoder support it;
- `chroma_subsampling` — YUV 4:4:4, 4:2:2, or 4:2:0 for lossy encoders that support it; disabled or ignored for true lossless modes;
- `png_filter` — Sub, Up, Average, or Paeth where the PNG encoder uses it;
- `effort` — 0–10 speed/quality tradeoff for codecs that expose an effort concept;
- `jxl_noise_synthesis` — JXL-specific tuning control;
- `jxl_gaborish` — JXL-specific tuning control;
- `metadata_retention` — bit flags for EXIF, IPTC, and XMP;
- `resize_long_edge` — optional target long-edge size;
- `crop` — optional crop rectangle.

Codec-specific parameters stay inside the relevant encoder adapter. The shared Core model should not expose codec-internal concepts such as JXL distance values.

### 4.3 Compression semantics

The public compression mode is explicitly either Lossy with a quality value from 1–100 or Lossless. It is never inferred from a quality value. A target that cannot perform true lossless encoding must report that fact through its capability declaration and reject a Lossless request during validation.

For JPEG, Lossless is invalid. For WebP, Lossless maps to the `zenwebp` lossless encoder path and Lossy maps to its lossy encoder path. For AVIF and JXL, Lossless maps to their genuine lossless encoder configurations.

### 4.4 Presets

Presets are named `EncodeOptions` constructors and do not create a separate conversion code path.

- `preset_web_optimized` targets AVIF or WebP with lossy compression at approximately quality 75 and strips EXIF.
- `preset_max_quality_archive` targets PNG or lossless WebP/AVIF with full metadata.
- `preset_social_media` targets JPEG with lossy compression, sRGB, and a long-edge resize of 1080/1350 px.

“Long edge” means the larger of width and height; resizing scales the other dimension proportionally. The archive preset must never choose JPEG because JPEG has no true lossless encode mode.

---

## 5. Interfaces

### 5.1 CLI (`prok`, via `clap`)

The target CLI accepts an input path, an output directory, and the documented general, decoder, encoder, and transform options. The current Phase 0–4 CLI implements only the input path, output path, format dispatch, and default encode settings; the remaining flags are planned interface surface.

General options:

- `--preset <name>`
- `--threads <n>`
- `--quiet`
- `--verbose`

Decoder options:

- `--dec-no-auto-rotate`
- `--dec-limit-memory <MB>`
- `--dec-demosaic <fast|hq>`
- `--dec-strict-metadata`

Encoder options:

- `--enc-quality <0-100>` — mutually exclusive with `--enc-lossless`;
- `--enc-lossless` — explicitly requests Lossless and must error for JPEG or any other target without true lossless support;
- `--enc-speed <0-10>`;
- `--enc-chroma <444|422|420>`;
- `--enc-bitdepth <8|10|12>`;
- `--enc-png-filter <sub|up|average|paeth>`;
- `--enc-jxl-noise`;
- `--enc-jxl-gaborish`;
- `--strip-metadata` — shorthand for keeping no metadata;
- `--keep-metadata <exif,iptc,xmp>`.

Transform options:

- `--resize-long-edge <PX>`
- `--crop <X,Y,W,H>`

Every CLI option must map directly to the shared Core option model. There must be no CLI-only conversion behavior. A Lossless request targeting JPEG must produce a clear error explaining that JPEG has no lossless encode mode rather than silently downgrading to lossy output.

### 5.2 WASM bindings (`prok-wasm`)

The WASM layer exposes three Core-facing operations:

- conversion from input bytes to target-format output bytes, using serialized decode and encode options;
- format probing for dispatch and UI display;
- a target-format lossless-capability query returning one of the semantic states Always, Never, or Configurable.

`DecodeOptions` and `EncodeOptions` should derive `Serialize` and `Deserialize` through Serde so JavaScript passes plain JSON representations. TypeScript bindings should be generated from the Rust model with tooling such as `tsify` or `specta` where practical rather than maintaining a second hand-written schema.

### 5.3 Web UI control surface

The browser UI binds directly to the shared Core option model:

| UI element | Bound field |
|---|---|
| Preset buttons (§4.4) | named preset constructors, then editable overrides |
| Strictness toggle | `DecodeOptions.strict_metadata` |
| Color-space force | `DecodeOptions.color_space_override` |
| Auto-rotate toggle | `DecodeOptions.auto_rotate` |
| RAW demosaic quality | `DecodeOptions.demosaic_quality`, labeled as interpolation quality |
| Lossless toggle | `EncodeOptions.compression = Lossless`; hidden or disabled for targets that cannot support it |
| Quality slider (1–100) | `EncodeOptions.compression = Lossy { quality }`; disabled while Lossless is active |
| Bit-depth selector | `EncodeOptions.bit_depth` |
| Chroma subsampling | `EncodeOptions.chroma_subsampling`; disabled while Lossless is active |
| PNG filter type | `EncodeOptions.png_filter` |
| Effort/speed | `EncodeOptions.effort` |
| Noise synthesis / Gaborish | `EncodeOptions.jxl_noise_synthesis` / `jxl_gaborish` |
| Metadata checkboxes | `EncodeOptions.metadata_retention` |

“Architect Mode” is a UI visibility gate over the same `EncodeOptions` structure. It must not fork the underlying model. Availability of the Lossless control is driven by the Core lossless-capability query at runtime rather than duplicated as frontend format logic.

---

## 6. Licensing

**Project license: AGPL-3.0-only.** This is required by the project's selected AVIF and JXL codec dependencies, including `zenavif` and `jxl-encoder`, which are published by Imazen under AGPL-3.0-only or LicenseRef-Imazen-Commercial terms. The project itself remains AGPL-3.0-only. Set this in the workspace manifest and repository `LICENSE` file at project initialization, not retroactively.

Practical implications:

- Public GitHub distribution and personal/hobbyist use are compatible with the project license.
- Downstream closed-source embedding of Core may require Imazen's commercial licensing for the relevant AVIF/JXL components; that is a downstream licensing concern rather than an implementation blocker for Prokoptas itself.
- If a server-side component is ever added, AGPL's network-use requirements must be reviewed. The current architecture performs conversion locally in WASM or locally in the CLI and has no server round-trip for conversion.

### 6.1 Additional codec licensing and patent considerations

`rawler` is LGPL-2.1. Pin the exact version and re-check the license on every upgrade. Because the overall project is already AGPL-3.0-only, the presence of LGPL-2.1 does not become the project's top-level license, but it must still be included in the dependency audit and distribution notices as appropriate.

HEIC/HEIF has a separate issue from copyright licensing: the `heic` crate uses HEVC/H.265 technology, and its software license does not grant patent rights. Third-party HEVC patents may apply depending on jurisdiction, distribution model, and use. Prokoptas should therefore describe HEIC as an engineering-supported decode path without claiming that HEIC/HEVC is patent-free. A formal public/commercial release should receive separate patent/licensing review.

Run `cargo license` or an equivalent dependency audit against the full dependency tree before first release. The production feature set must not accidentally enable optional native dependencies, including `jxl-oxide`'s optional `lcms2` C binding or `zenavif`'s C-FFI `unsafe-asm` decoder path.

---

## 7. WASM-specific constraints

- **Memory:** WASM linear memory has practical ceilings that can be substantially below native process limits depending on browser and build configuration. Large RAW files decoded into RGBA16 buffers can be substantial; for example, a 45MP image can require roughly 360MB for RGBA16 pixels alone before additional working memory. `DecodeOptions.memory_limit_mb` must be enforced before large allocations. Probe dimensions and estimate required memory first; reject or downsample rather than allocating beyond the configured limit.
- **Threading:** `--threads` applies to the native CLI. Parallel browser WASM execution is opt-in and requires `SharedArrayBuffer`, which in turn requires appropriate cross-origin isolation response headers, including `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. Single-threaded WASM is the safe default.
- **Native-only codec features:** Features that introduce C FFI or other native-only assumptions must remain disabled in the WASM build. In particular, the `unsafe-asm` path of `zenavif` is not part of the browser build.
- **Binary size:** AV1/JXL codec logic is nontrivial in code size. Track the final `.wasm` output size in CI once Core stabilizes and consider `wasm-opt` post-processing.

---

## 8. Error handling

The Core exposes deterministic error categories rather than `anyhow` or `Box<dyn Error>` at the public boundary, so CLI and WASM shells can map the same failure to process exit codes, terminal text, or structured JavaScript errors.

Core decode errors are:

- **UnsupportedFormat** — no registered decoder accepts the input;
- **CorruptData** — the input is malformed or fails required integrity checks;
- **MemoryLimitExceeded** — projected or required decode memory exceeds the configured limit, with the required and allowed sizes available to the caller;
- **IoError** — a native CLI I/O failure. This category is never populated by the WASM shell, which supplies bytes directly.

Core encode errors are:

- **InvalidOptions** — an option combination is invalid for the selected encoder, such as requesting chroma subsampling with a true lossless mode when the encoder does not allow it;
- **LosslessNotSupported** — the requested target does not provide a true lossless encode path, such as JPEG;
- **UnsupportedColorSpace** — the target encoder cannot represent the requested color space;
- **EncodingFailed** — the codec rejected the image or failed during encoding.

The CLI maps these errors to deterministic process exit codes and stderr messages. WASM maps them to a structured JavaScript-visible error object containing a stable machine-readable code and a human-readable message. Callers must branch on the code rather than parsing error strings.

`LosslessNotSupported` is intentionally distinct from `InvalidOptions` so the UI and CLI can give a direct explanation when a user asks for lossless output from a format that cannot provide it.
