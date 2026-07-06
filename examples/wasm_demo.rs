// WASM Browser Demo - a thin wasm-bindgen shim around the retro CRT scene.
//
// Reuses examples/retro_crt.rs's code directly (`#[path = "retro_crt.rs"]
// mod retro_crt;` below) instead of duplicating it - the scene, its wasm32/
// WebGL2-specific branches (canvas config, OIT skipped, tonemapping without
// LUTs), and `pub fn main()` all live in that one file. This file only adds
// what's specific to being loaded as a browser module:
// - `#[wasm_bindgen(start)]` entry + `console_error_panic_hook`,
// - a WebGL2 availability probe (see docs/index.html for the matching JS
//   probe, which runs first and avoids fetching the wasm at all if it fails).
//
// Build the browser-ready site into docs/ (see docs/README.md for local
// preview instructions):
//   cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
//   wasm-bindgen --target web --no-typescript --out-dir docs \
//     target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm
//   wasm-opt -Oz --strip-debug --strip-producers --enable-nontrapping-float-to-int \
//     --enable-bulk-memory --enable-sign-ext --enable-mutable-globals \
//     --enable-simd --enable-reference-types \
//     -o docs/wasm_demo_bg.wasm docs/wasm_demo_bg.wasm

#[path = "retro_crt.rs"]
mod retro_crt;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

// WASM entry point - invoked by the JS glue's `init()` (see docs/index.html).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();

    // WebGL2 availability probe, on a THROWAWAY canvas - probing #bevy
    // itself would take its context and break wgpu's own getContext later
    // ("canvas already in use"). Without this guard, an unsupported/blocked
    // WebGL2 (e.g. Brave with aggressive fingerprinting shields, software-
    // rendering-only environments) panics deep inside wgpu surface
    // creation; per the demo's contract we instead report and exit.
    // (index.html performs the same probe before even fetching the wasm -
    // this is the defense for hosts that serve the module differently.)
    let webgl2_available = (|| {
        use wasm_bindgen::JsCast;
        let canvas = web_sys::window()?
            .document()?
            .create_element("canvas")
            .ok()?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .ok()?;
        canvas.get_context("webgl2").ok().flatten()
    })()
    .is_some();
    if !webgl2_available {
        web_sys::console::error_1(
            &"bevy_tui_texture wasm_demo: WebGL2 is not available in this browser \
              (unsupported, or blocked - e.g. by Brave's fingerprinting shields). \
              The demo cannot run; exiting."
                .into(),
        );
        return;
    }

    suppress_canvas_escape_key();
    clamp_canvas_to_safe_texture_size();
    retro_crt::main();
}

// retro_crt.rs's `Window { canvas: "#bevy", fit_canvas_to_parent: true, .. }`
// resizes the canvas to its parent's CSS size. `WgpuSettingsPriority::WebGL2`
// (also in retro_crt.rs) caps `max_texture_dimension_2d` at 2048 - on a
// HiDPI/Retina display (e.g. Apple Silicon Macs) even a modest ~1160
// CSS-px-wide browser window already produces a >2048 PHYSICAL-pixel
// surface, which fails `Surface::configure`'s validation and - per bevy
// 0.19's fatal render-error policy - silently quits the app to a black
// screen (observed on macOS Brave: "Requested was (2312, 810), maximum
// extent for either dimension is 2048").
//
// An earlier version of this function tried to fix that by overriding
// `window.devicePixelRatio` to always read `1.0` (same DOM-spoofing trick as
// `suppress_canvas_escape_key` below). That does NOT work: winit's actual
// resize/scale detection (see
// `winit::platform_impl::web::web_sys::resize_scaling::ResizeScaleInternal`)
// watches the canvas with a `ResizeObserver` requesting
// `ResizeObserverBoxOptions::DevicePixelContentBox` wherever the browser
// supports it (Chromium/Brave and modern Firefox both do - only Safali
// lacks it) - that API reports the browser's own device-pixel measurement
// of the canvas's box directly from the compositor, which is NOT the same
// thing as the JS-visible `devicePixelRatio` property and can't be spoofed
// by redefining it (verified: overriding the property to `2` on a real
// devicePixelRatio-1 display left the canvas's actual backing-buffer width
// completely unaffected).
//
// The only thing that actually changes what `ResizeObserver` reports is the
// canvas's own CSS layout box, so this instead reads the REAL (unspoofed)
// `devicePixelRatio` and clamps the canvas's CSS size with `max-width`/
// `max-height` such that `css_size * devicePixelRatio` never exceeds 2048 -
// CSS `max-width` wins over whatever width winit's `fit_canvas_to_parent`
// sets, so this is a hard, permanent ceiling regardless of the parent's
// size. retro_crt.rs is shared with the native binary, so - same reasoning
// as `suppress_canvas_escape_key` - this stays purely in this wasm-only
// file rather than adding a wasm32 branch there.
#[cfg(target_arch = "wasm32")]
fn clamp_canvas_to_safe_texture_size() {
    const MAX_PHYSICAL_PX: f64 = 2048.0;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(canvas) = window
        .document()
        .and_then(|d| d.get_element_by_id("bevy"))
    else {
        return;
    };
    let dpr = window.device_pixel_ratio();
    let max_css_px = (MAX_PHYSICAL_PX / dpr).floor();
    let Ok(style) = canvas.dyn_into::<web_sys::HtmlElement>().map(|e| e.style()) else {
        return;
    };
    let _ = style.set_property("max-width", &format!("{max_css_px}px"));
    let _ = style.set_property("max-height", &format!("{max_css_px}px"));
}

// retro_crt.rs's `handle_input` unconditionally sends `AppExit` on Escape
// (its doc comment claims "native only", but the system isn't actually
// cfg-gated) - that file is shared with the native binary, so editing it to
// add a wasm32 cfg would touch shared/native-tested code just for a
// wasm-only quirk. Instead, disable it purely at the DOM level, from this
// wasm-only file: winit registers its own "keydown" listener on the `#bevy`
// canvas lazily, inside `retro_crt::main()`'s `App::run()` (WinitPlugin's
// window creation). Registering OUR "keydown" listener on that same canvas
// element *before* calling `retro_crt::main()` guarantees ours runs first -
// per the DOM spec, listeners on the event's own target (not an ancestor)
// fire in REGISTRATION ORDER regardless of the capture flag - so calling
// `stop_immediate_propagation()` here for Escape means winit's listener
// never sees that keydown at all, `ButtonInput<KeyCode>` never marks
// `KeyCode::Escape` pressed, and `handle_input`'s `AppExit` branch never
// fires. Native is untouched: this function is wasm32-only and the canvas
// doesn't exist there.
#[cfg(target_arch = "wasm32")]
fn suppress_canvas_escape_key() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("bevy"))
    else {
        return;
    };

    // Leaked deliberately (`forget`): this listener must outlive the
    // function and live for the rest of the page's life, exactly like
    // wasm-bindgen's own generated glue does for its callbacks.
    let closure = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
        |event: web_sys::KeyboardEvent| {
            if event.key() == "Escape" {
                event.stop_immediate_propagation();
                event.prevent_default();
            }
        },
    );
    let _ = canvas.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
    closure.forget();
}

// Always defined (not cfg-gated): rustc requires a crate-level `main` for
// every target, wasm32 included - `wasm32-unknown-unknown` has no runtime
// that calls it, though, so on wasm it's simply never invoked (the JS glue
// calls the exported `__wbindgen_start`, i.e. `wasm_main` above, instead).
fn main() {
    retro_crt::main();
}
