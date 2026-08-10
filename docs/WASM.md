# Wasm library compilation

Quire's library can be checked for the bare WebAssembly target with:

```sh
cargo check --target wasm32-unknown-unknown --lib
```

This is a compile-compatibility target only. It does not provide browser or
WASI integration, HTTP(S) or filesystem resource loading, a JavaScript API,
or PDF file output. Use in-memory HTML/CSS and pass a `Vec<u8>` to `write_pdf`
when experimenting with this target. A future platform integration can replace the
currently unsupported resource backend without changing the renderer core.
