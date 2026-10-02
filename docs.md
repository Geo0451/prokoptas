# Prokoptas project documentation

This document explains what the project is, how it is structured, what the major Rust files do, and how future work should fit into the current architecture.

It is written to be readable by humans and also usable by LLMs and future contributors who need to understand the codebase quickly.

---

## 1. Project purpose

Prokoptas is a Rust image conversion tool with:

- a native CLI binary named `prok`
- a WebAssembly shell for browser use
- a shared core library that owns the actual decode/normalize/encode logic

The design goal is simple:

- input bytes arrive
- the format is probed
- a decoder parses it into a normalized in-memory image
- optional transforms may run
- an encoder emits output bytes

This architecture keeps format-specific logic centralized in the Core layer instead of spreading it across CLI and browser code.

The project is intentionally still-image focused. It is not intended to be a general-purpose multi-frame image engine.

---

## 2. Current project status

From the code and technical spec, the current working state is:

- Core adapters exist for PNG, JPEG, TIFF, BMP, WebP, HEIC, AVIF, JXL, and RAW decode paths.
- The CLI converts among PNG, JPEG, TIFF, BMP, WebP, AVIF, and JXL through the shared Core pipeline; HEIC and RAW remain decode-only inputs.
- The core exposes strong typed abstractions for errors, format tags, compression behavior, and normalized image data.
- The broader CLI option surface and browser conversion interface remain planned work.

The important point is that the project is not just a single binary; it is a structured workspace with a shared processing model.

---

## 3. Workspace layout

The workspace root contains:

- `Cargo.toml` — workspace definition
- `crates/prok-core/` — shared logic and format adapters
- `crates/prok-cli/` — command-line shell
- `crates/prok-wasm/` — WebAssembly entry point
- `web/` — browser frontend target area

The relevant crates are:

- `prok-core` = the real engine
- `prok-cli` = command-line interface
- `prok-wasm` = wasm exports

---

## 4. High-level architecture

The runtime flow is:

1. Read bytes from disk or JS input.
2. Detect format using a probe function.
3. Decode native pixels and metadata in the codec adapter.
4. The `Decoder` trait normalizes color interpretation and EXIF orientation.
5. The `Encoder` trait converts to the target color space and supported bit depth, applies metadata policy, validates options, and encodes.

This is the prime invariant to preserve when adding new functionality:

`bytes -> probe -> decode -> normalized image -> encode -> bytes`

No conversion logic should live in shell layers. The CLI and browser shell should only do I/O and pass through options.

---

## 5. Core concepts

### 5.1 `DecodedImage`

Defined in `crates/prok-core/src/image.rs`.

`DecodedImage` is the normalized representation all decoders produce. It contains:

- `pixels`: `PixelBuffer`
- `width` and `height`
- `color_space`
- `bit_depth`
- `orientation`
- `orientation_applied`
- `metadata`

This is the shared image model used by encoders.

### 5.2 `PixelBuffer`

`PixelBuffer` is the internal storage abstraction. Right now it supports:

- `Rgba8(Vec<u8>)`
- `Rgba16(Vec<u16>)`

This is the core conversion boundary; format-specific layouts are normalized into RGBA before encode logic runs.

### 5.3 `Compression`

Defined in `crates/prok-core/src/options.rs`.

Compression is modeled explicitly as:

- `Lossy { quality: u8 }`
- `Lossless`

This is important because the project enforces semantics rather than guessing:

- JPEG is never lossless
- PNG is always lossless
- WebP can be either
- JXL is configurable; AVIF currently supports lossy encoding only

The code validates lossless requests using `encoder.lossless_capability()`.

### 5.4 `LosslessCapability`

In `registry.rs`:

- `Always`
- `Never`
- `Configurable`

This is the machine-readable contract for whether an encoder supports true lossless output.

### 5.5 `FormatTag`

Also in `registry.rs`.

Format tags are stable machine categories like:

- `Png`
- `Jpeg`
- `Tiff`
- `Bmp`
- `WebP`
- `Heif`
- `Avif`
- `Jxl`
- `Raw`

These tags are used by the registry dispatch system.

### 5.6 `FormatRegistry`

The registry is the central matching and dispatch system for format detection.

It exposes:

- `decoder_for(input)`
- `encoder_for(format_tag)`

A format is recognized by a `probe` implementation. Then the matching decoder or encoder is selected.

---

## 6. File-by-file guide

Below is the practical map of what each file does.

### `Cargo.toml`

The workspace root defines the Rust workspace and includes:

- `prok-core`
- `prok-cli`
- `prok-wasm`

It sets crate versions, edition, and shared lint settings.

### `crates/prok-core/src/lib.rs`

This is the central public API for the Core crate.

It exports:

- format types
- errors and result aliases
- `Decoder` and `Encoder` traits
- registry types
- option types
- all format-specific adapters as public modules

This file also contains the top-level test coverage for core validation rules. In other words, this is the hub of the whole core architecture.

### `crates/prok-core/src/codec.rs`

This file defines the core traits:

- `Decoder`
- `Encoder`

Every format adapter implements these traits. This is the key abstraction layer. Anything wanting to add support for a new format should match this interface.

### `crates/prok-core/src/registry.rs`

This file defines:

- stable format tags
- `LosslessCapability`
- `FormatRegistry`

This is the machinery that lets format detection and dispatch work without hardcoding a huge chain of `if` statements in each shell.

### `crates/prok-core/src/options.rs`

This file defines the encode/decode option model.

It includes:

- `Compression`
- `DecodeOptions`
- `EncodeOptions`
- `DemosaicQuality`
- `ChromaSubsampling`
- `PngFilter`
- `MetadataRetention`
- `CropRect`

This is the shared options model used by CLI and WASM. It also validates semantics such as:

- quality 1..=100
- lossless requests for non-lossless codecs
- rejecting chroma subsampling in lossless mode

### `crates/prok-core/src/image.rs`

This file is the actual normalized image model layer.

It contains:

- `PixelBuffer`
- `ColorSpace`
- `BitDepth`
- `Orientation`
- `ImageMetadata`
- `DecodedImage`
- EXIF orientation parsing
- orientation application helpers

This is the place where decoded images are normalized before being passed to encoders.

### `crates/prok-core/src/processing.rs`

Owns the format registry and `convert` pipeline. The CLI and Core callers use this path; codec trait defaults apply the same normalization and output preparation when adapters are called directly.

### `crates/prok-core/src/png.rs`

PNG adapter.

It implements:

- `PngDecoder`
- `PngEncoder`
- probe logic for PNG signatures
- decode of PNG bytes into `DecodedImage`
- encode to PNG bytes
- metadata extraction and preservation
- memory-limit enforcement

PNG is treated as a fully lossless path and reports `LosslessCapability::Always`.

### `crates/prok-core/src/jpeg.rs`

JPEG adapter.

It implements:

- `JpegDecoder`
- `JpegEncoder`
- JPEG magic-byte probing
- RGB→RGBA conversion and reverse
- lossy JPEG encode using `jpeg-encoder`
- validation that `Compression::Lossless` is rejected

This file is the clearest example of the project’s explicit lossless semantics: JPEG is “Never” for lossless output.

### `crates/prok-core/src/webp.rs`

WebP adapter.

It implements:

- `WebpDecoder`
- `WebpEncoder`
- RIFF/WEBP probing
- animated WebP rejection because the project is still-image only
- lossless and lossy config selection through `zenwebp`
- metadata retention

This is one of the more feature-complete format adapters.

### `crates/prok-core/src/bmp.rs`

BMP adapter.

It implements:

- `BmpDecoder`
- `BmpEncoder`
- BMP signature probe
- decode to normalized RGBA
- encode back to BMP
- always-lossless semantics

### `crates/prok-core/src/tiff.rs`

TIFF adapter.

It implements:

- `TiffDecoder`
- `TiffEncoder`
- TIFF magic detection
- decoding from TIFF by way of `image`
- encode as TIFF using the supported lossless subset

TIFF is treated as lossless under the supported compression subset.

### `crates/prok-core/src/heic.rs`

HEIC/HEIF adapter.

It implements a decoder for HEIC input using the `heic` crate.

This file is highly relevant because the project explicitly treats HEIC as a decode-only input path. It also contains the real sample-based test coverage for HEIC decoding and conversion into PNG/JPEG/WebP.

### `crates/prok-core/src/avif.rs`

AVIF adapter.

The adapter uses `zenavif` for pure-Rust still-image decoding and encoding.

The encoder declares:

- `lossless_capability() -> Never`

AVIF encoding is lossy-only because the selected encoder/decoder path did not preserve exact samples in lossless-mode verification. AVIF remains supported by the Web Optimized preset. AVIF probe precedence is above the broader HEIC-compatible-brand probe so AVIF inputs are dispatched to the correct decoder.

### `crates/prok-core/src/jxl.rs`

JPEG XL adapter.

The decoder registers the `jxl-oxide` image integration before decode. The encoder uses `jxl-encoder` and reports `LosslessCapability::Configurable`.

Both paths are still-image only.

### `crates/prok-core/src/raw.rs`

RAW adapter.

This is the decode-only family for camera sensor formats such as CR2, NEF, DNG, and similar. It is intentionally not an output encoder path.

The doc comments explicitly say the demosaic step is not lossless; it is an interpolation approximation. This is a core semantic rule the project preserves.

---

## 7. CLI (`prok-cli`)

The CLI entry point is:

- `crates/prok-cli/src/main.rs`

What it does:

- parse positional input/output paths plus the shared decode/encode options
- route all conversions through `prok_core::convert`
- optionally process supported files in one input directory using a bounded worker pool
- report per-file failures and return a stable non-zero exit code

For one file, the output extension selects the target format:

```text
prok input.png output.webp --enc-quality 82 --quiet
```

For batch conversion, pass an input directory, output directory, `--batch`, and `--format`. Batch enumeration is nonrecursive and includes recognized image extensions only:

```text
prok ./images ./converted --batch --format webp --threads 4 --preset web-optimized
```

The default mode prints a conversion summary, `--verbose` prints each completed path, and `--quiet` suppresses success output while errors remain on stderr. Exit codes distinguish invalid usage/options (2), unsupported input (3), corrupt data (4), memory limit (5), I/O (6), and encoding (7).

This is a thin shell around Core logic and is not where the real conversion rules live.

### CLI key property

The CLI should never “invent” conversion behavior. It must pass through the Core option model and use same explicit semantics.

---

## 8. WASM shell

The WebAssembly entry is `crates/prok-wasm/src/lib.rs`. It exposes `version()`, `probeFormat(Uint8Array)`, `losslessCapability(format)`, and `convert(input, target, decodeOptions, encodeOptions)`. Options are deserialized through Serde, and errors are returned as structured objects with stable `code` and `message` fields.

WASM conversion uses the same Core registry and pipeline as the CLI. Decode memory limits are checked before large decoded buffers are rendered, including AVIF frame limits and JXL dimension preflight. Browser conversion is single-threaded by default; threaded WASM remains an optional enhancement requiring shared-memory support and cross-origin isolation.

To run the browser UI, install the `wasm32-unknown-unknown` Rust target and the matching `wasm-bindgen-cli` version, then run `npm install` and `npm run dev` from `web/`. The dev and production build scripts generate the WASM bindings locally before building the UI.

---

## 9. How a conversion actually flows

A practical flow looks like this:

1. Input file read into bytes
2. Registry chooses a decoder by calling `probe()`
3. Decoder adapter returns native pixels and extracted metadata.
4. Core applies a decode color-space override to the samples, applies EXIF orientation once, and resets the retained EXIF orientation tag when pixels have been rotated.
5. Core converts pixels to sRGB and the requested target bit depth, then applies metadata-retention flags.
6. The target adapter validates codec-specific options and writes output bytes.

Example pseudocode:

```rust
let input = std::fs::read(path)?;
let decoder = PNG_REGISTRY.decoder_for(&input)?;
let image = decoder.decode(&input, &DecodeOptions::default())?;

let output = PngEncoder.encode(&image, &EncodeOptions {
    compression: Compression::Lossless,
    ..EncodeOptions::default()
})?;
```

The same conceptual flow is used regardless of whether the caller is CLI or WASM.

PNG and TIFF support 8-bit and 16-bit output. JPEG, BMP, WebP, AVIF, and JXL currently support 8-bit output; 10-bit and 12-bit requests are rejected until a target adapter supports them. Color conversion supports sRGB, linear sRGB, and Display P3 (D65). An override transforms samples rather than relabeling unchanged values.

Metadata is stripped by default. PNG can retain EXIF; WebP can retain EXIF and XMP. A request to retain extracted metadata unsupported by the selected target returns `InvalidOptions`. IPTC extraction and encoding are not currently implemented.

---

## 10. Why this architecture matters

This project is designed so future additions are easy:

- add a new decoder module
- implement `Decoder`
- implement `probe()`
- return a normalized `DecodedImage`
- add `Encoder` and `lossless_capability()`
- register it in the relevant registry

If you do that correctly, the rest of the system already knows how to work with it.

This is much better than scattering conversion logic across multiple dispatch branches.

---

## 11. Rules for future development

When adding a new format or fixing a bug, follow these principles:

### Rule 1: keep format logic inside Core

Do not add conversion logic in CLI or WASM code. Put it into the Core crate and expose a stable adapter interface.

### Rule 2: preserve explicit lossless semantics

Do not infer losslessness from a high quality number. Check the encoder capability and validate the request explicitly.

### Rule 3: normalize before encoding

All decode paths should end at `DecodedImage` and RGBA storage. Encoders should operate on the normalized internal buffer, not on raw codec-specific pixel layouts.

### Rule 4: use the registry system

A new format should have a `probe()` implementation and ideally be included in a registry so dispatch is centralized.

### Rule 5: maintain tests around real behavior

The project already has real-sample tests for HEIC and conversion checks for CLI flows. Keep that pattern when adding formats or fixing regressions.

---

## 12. Key takeaways for contributors

If you are new to this repo, the most important things to know are:

- the project is a shared Core + thin shells design
- `Decoder` and `Encoder` are the main abstraction contracts
- `FormatRegistry` controls format detection and dispatch
- `DecodedImage` is the canonical normalized representation
- lossless behavior is explicit and capability-based
- CLI code is intentionally thin and should stay that way

This is the mental model for moving quickly in the codebase.

---

## 13. Recommended “first read” order for future devs

If you need to understand the repo quickly, read in this order:

1. `crates/prok-core/src/lib.rs`
2. `crates/prok-core/src/codec.rs`
3. `crates/prok-core/src/registry.rs`
4. `crates/prok-core/src/options.rs`
5. `crates/prok-core/src/image.rs`
6. a representative format adapter such as `png.rs` or `jpeg.rs`
7. `crates/prok-cli/src/main.rs`
8. `crates/prok-wasm/src/lib.rs`

That gives the core operational picture in the shortest path.

---

## 14. Good summary sentence

Prokoptas is a Rust image conversion project built around a normalized Core image pipeline, explicit lossless capability validation, and thin CLI/WASM shells that orchestrate work rather than implement it directly.

---

## 15. Notes for AI / LLM agents

When working in this repo, assume the following:

- use the Core crate as the source of truth
- keep shell layers thin
- do not add format-specific logic directly in CLI or WASM
- preserve the `probe -> decode -> normalize -> validate -> encode` pipeline
- maintain explicit `LosslessCapability` semantics
- prefer registry-based dispatch over hardcoded branching

This makes future work much faster and safer.
