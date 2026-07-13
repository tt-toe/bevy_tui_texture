# bevy_tui_texture — WASM demo site

Browser-ready build of `examples/wasm_demo.rs` - a thin wasm-bindgen shim
around `examples/retro_crt.rs`'s scene (glTF model + `ExtendedMaterial` CRT
shader + additive reflection + overlay UI + camera modes, running on
WebGL2). `wasm_demo.rs` includes `retro_crt.rs` as a module and calls its
`build_app()` rather than duplicating the source - the wasm-only bits
(canvas config, tonemapping without LUTs, OIT skipped) live inline in
`retro_crt.rs` behind `#[cfg(target_arch = "wasm32")]`.

**This directory is not self-contained** - the demo fetches its runtime
assets (glTF model, WGSL shaders) from `../assets/` at runtime, i.e. it
expects to be served alongside its sibling `examples/assets/` directory
with the same relative layout: `examples/{web,assets}/`. See "Deploy"
below for what that means when publishing to a host that doesn't preserve
the repo layout as-is.

## Contents

| file | what |
|---|---|
| `index.html` | loader page (`<canvas id="bevy">` + module script) |
| `wasm_demo.js` | wasm-bindgen JS glue (generated) |
| `wasm_demo_bg.wasm` | the compiled demo (generated) |
| `../assets/models/retro_crt.glb` | CRT computer model (fetched at runtime, sibling dir) |
| `../assets/shaders/*.wgsl` | CRT / reflection shaders (fetched at runtime, sibling dir) |

`index.html` and this README are hand-maintained; only `wasm_demo.js` /
`wasm_demo_bg.wasm` are build outputs.

## Local preview

WASM must be served over HTTP — opening `index.html` via `file://` fails
(module scripts and `fetch()` of the `.wasm`/assets are blocked). Because
the demo fetches assets from the sibling `../assets/` directory, the
server must be rooted at **`examples/`**, not this directory itself -
rooting it here would put `assets/` outside what the server can serve at
all:

```bash
# From the repository root:
cd examples
python3 -m http.server 8080
# then open http://127.0.0.1:8080/web/
```

Alternatives: `npx serve .`, `caddy file-server --listen :8080` (run from
`examples/`), or any equivalent static server. No special headers are
required (no SharedArrayBuffer/COOP/COEP needed). First load fetches the
`.wasm` (still ~24MB even optimized - be patient) plus the `.glb` model —
watch the browser devtools Network/Console tabs if the canvas stays black
(a 404 on `../assets/...` usually means the server wasn't rooted at
`examples/`).

## Deploy

Upload BOTH `examples/web/` and `examples/assets/` to your host, preserving
their sibling relationship (`<upload-root>/web/` + `<upload-root>/assets/`),
and point visitors at `<upload-root>/web/`. Where `<upload-root>` lives is
up to you — e.g. rename/copy `examples/` itself to the root of a
`*.github.io` repo, or configure your host's document root at `examples/`.
If your host can only serve a single directory and can't preserve this
sibling layout, mirror both directories into a fresh directory of your
own (e.g. `rsync -a examples/{web,assets} deploy/`) before uploading -
just keep `web/` and `assets/` as siblings in whatever you upload.

## Rebuilding the generated files

From the repository root (requires `wasm32-unknown-unknown` target, a
`wasm-bindgen` CLI matching the `wasm-bindgen` version in `Cargo.lock`, and
`wasm-opt` from [binaryen](https://github.com/WebAssembly/binaryen) for the
size-reduction pass):

```bash
cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir examples/web \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm

# Shrink the binary in place (~31MB cargo output -> ~24MB). The
# --enable-* flags are required because Rust's wasm32-unknown-unknown
# target emits bulk-memory / nontrapping-float-to-int / sign-ext /
# mutable-globals / simd / reference-types instructions by default, which
# wasm-opt's validator otherwise rejects.
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm
```

No asset-copying step is needed anymore - `examples/assets/` is fetched
directly at runtime from its sibling location, the same directory native
builds read from (see `examples/retro_crt.rs`'s `build_app` doc comment).

**After regenerating, bump `ASSET_VERSION` in `index.html`** (a `const` near
the top of its `<script type="module">`) to any new value (e.g. today's
date). `wasm_demo.js` and `wasm_demo_bg.wasm` are a matched pair from the
same build; a host that caches aggressively (GitHub Pages sends
`cache-control: max-age=600`) could otherwise pair a cached copy of one
with a fresh copy of the other on a revisit within that window -
`WebAssembly.instantiate` then fails with a `LinkError: function import
requires a callable` (the mismatched JS glue and wasm binary disagree on
import names). `index.html` fetches both through `?v=${ASSET_VERSION}`, so
bumping it forces a re-fetch of both files together instead of silently
mixing an old and a new one.

### Binary size

The `.wasm` is large mainly because it statically links a full Bevy
renderer + PBR pipeline + glTF loader + font shaping/rasterization stack;
there's no server-side asset to trim it further at this layer. What's
already done to keep it as small as practical:

- `[profile.wasm-release]` in `Cargo.toml`: `opt-level = "z"`, `lto =
  "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- `wasm-opt -Oz --strip-debug --strip-producers` (above): cuts a further
  ~15% off the wasm-bindgen output.
- `examples/wasm_demo.rs`'s own `bevy` dev-dependency (see Cargo.toml's
  wasm32-only `[target.'cfg(target_arch = "wasm32")'.dev-dependencies]`
  block) skips the `tonemapping_luts`/`zstd_rust` features (the ~680KB
  embedded LUT textures + ktx2/zstd decoder) — the wasm build instead sets
  an explicit `Tonemapping::KhronosPbrNeutral` on the camera (see the
  `#[cfg(target_arch = "wasm32")]` branch in `setup()`), since
  `Camera3d`'s default tonemapper (`TonyMcMapface`) needs those features
  and panics without them. It also skips the native-only `x11` feature.
- True `#![no_std]` is not on the table: Bevy (and most of this crate's
  own dependencies — `raqote`, `rustybuzz`, `ratatui`) use `std`
  extensively; making the dependency graph `no_std`-compatible would mean
  forking or patching Bevy itself, not a build-flag change.

## Controls

- **SPACE** — toggle CRT effects
- **LEFT/RIGHT** (or click the tab bar on the CRT screen) — switch tabs
- Overlay panel radio buttons — camera mode (mouse follow / fixed / orbit)
- Click the overlay panel's title bar — collapse it down to just the
  title bar, or expand it back (toggles; the panel's on-screen size
  follows, via `Tui::request_resize`)
- BUG: Some inputs don't working on macOS with wasm current version

## Loading screen

The `.wasm` is tens of MB even after `wasm-opt`, so `index.html` shows a
real progress bar (bytes downloaded vs. `Content-Length`, or an
indeterminate sliding stripe if the server doesn't send one) instead of a
static label that looks stuck partway through the load, and keeps a
staged status message up (see `report_wasm_boot_status` in
`examples/wasm_demo.rs`) through renderer init, asset loading, and shader
compilation - the canvas stays black through all of that, so the overlay
is the only feedback the page can give.

## Known WebGL2 differences vs. native

- **WebGL2 is required.** If it is unavailable (unsupported browser, or
  blocked - e.g. by Brave's fingerprinting shields or disabled hardware
  acceleration), the page shows "WebGL2 is not available in this browser"
  and exits instead of starting the demo. Probe your browser at
  `about:gpu` (Chromium) or https://get.webgl.org/webgl2/.
- Order Independent Transparency is disabled on wasm (WebGL2 has no
  storage buffers); overlapping transparent surfaces sort per-mesh
  instead. All other features (glTF, custom materials, terminal input)
  match native.
- **Known issue: startup can hang indefinitely on some browser/GPU
  combinations**, past the WebGL2 check, with no console error (Firefox
  has been seen to eventually report "Script terminated by timeout"; the
  tab's content process can become unrecoverable, requiring it to be
  closed). Confirmed via a from-scratch isolation test that this reproduces
  with a *pure* Bevy 0.19 app (`DefaultPlugins` + a bare camera, zero
  `bevy_tui_texture` code) on this Bevy 0.19.0 / wgpu 29.0.4 / winit
  0.30.12 / wasm-bindgen 0.2.108 combination - it is not a bug in this
  crate or in `retro_crt.rs`/`wasm_demo.rs`. Forcing
  `WgpuSettingsPriority::WebGL2` (conservative, known-safe device
  limits/features instead of trusting the adapter's raw report) and
  switching `opt-level` between `"z"`/`"s"` were both tried and neither
  resolved it. If you hit this, try a different browser, GPU, or OS first;
  if it's universal, check Bevy's issue tracker for wasm32 startup hangs
  around this version combination.
