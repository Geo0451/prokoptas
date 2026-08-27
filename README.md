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
