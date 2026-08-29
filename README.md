# Prokoptas

Prokoptas is a Rust image conversion tool with a native CLI and a browser-facing WebAssembly shell around one shared Core library.

## Phase 0

The initial workspace contains:

- `prok-core`: shared error and result boundary
- `prok-cli`: native `prok` binary shell
- `prok-wasm`: `wasm-bindgen` shell

The CLI currently supports PNG, JPEG, and WebP paths. WebP output defaults to lossy quality 75:

```sh
cargo run -p prok-cli -- input.png output.jpg
cargo run -p prok-cli -- input.jpg output.png
cargo run -p prok-cli -- input.png output.webp
```

Run the checks with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check -p prok-wasm --target wasm32-unknown-unknown
```

The project is licensed under AGPL-3.0-only.

## Patent and Licensing Considerations

### HEIC / HEVC Patent Notice
HEIC/HEIF decoding is powered by the pure-Rust `heic` crate. While the crate is open-source, HEIC relies on HEVC (H.265) video coding technology. The software license does not grant patent rights for HEVC/H.265. Third-party HEVC patents may apply depending on jurisdiction, distribution model, and usage. Prokoptas provides HEIC as an engineering-supported decode input path without claiming that HEIC/HEVC is patent-free. Any commercial deployment or public distribution should undergo separate legal and patent review.

