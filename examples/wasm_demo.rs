// WASM Browser Demo - a thin wasm-bindgen shim around the retro CRT scene.
//
// Reuses examples/retro_crt.rs's code directly (`#[path = "retro_crt.rs"]
// mod retro_crt;` below) instead of duplicating it - the scene, its wasm32/
// WebGL2-specific branches (canvas config, OIT skipped, tonemapping without
// LUTs), and `pub fn main()` all live in that one file. This file only adds
// what's specific to being loaded as a browser module:
// - `#[wasm_bindgen(start)]` entry + `console_error_panic_hook`,
// - a WebGL2 availability probe (see docs/index.html for the matching JS
//   probe, which runs first and avoids fetching the wasm at all if it fails),
// - boot-progress reporting to the page's loading overlay (`boot_status`
//   below) - registered on top of `retro_crt::build_app()` before `run()`.
//
// Build the browser-ready site into examples/web/ (see examples/web/README.md
// for local preview instructions):
//   cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
//   wasm-bindgen --target web --no-typescript --out-dir examples/web \
//     target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm
//   wasm-opt -Oz --strip-debug --strip-producers --enable-nontrapping-float-to-int \
//     --enable-bulk-memory --enable-sign-ext --enable-mutable-globals \
//     --enable-simd --enable-reference-types \
//     -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm

#[path = "retro_crt.rs"]
mod retro_crt;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

// WASM entry point - invoked by the JS glue's `init()` (see docs/index.html).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    // Panics: log to the console (console_error_panic_hook) AND forward to
    // the loading overlay. On mobile Safari there is no reachable console
    // without a tethered desktop inspector, so the overlay is often the
    // only place a panic message can be seen at all - without this, a
    // panic just freezes the overlay on whatever milestone was last sent.
    // The heap size distinguishes a logic panic from one triggered at the
    // edge of the browser's memory cap. (An out-of-memory ABORT - a failed
    // memory.grow - never runs this hook at all: it traps straight to a
    // JS-side "Unreachable code should not be executed" RuntimeError, which
    // index.html's error listener annotates as probable OOM.)
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        demo_status(&format!("panicked (heap {} MB): {info}", wasm_heap_mb()));
    }));

    // WebGL2 availability probe, on a THROWAWAY canvas - probing #bevy
    // itself would take its context and break wgpu's own getContext later
    // ("canvas already in use"). Without this guard, an unsupported/blocked
    // WebGL2 (e.g. Brave with aggressive fingerprinting shields, software-
    // rendering-only environments) panics deep inside wgpu surface
    // creation; per the demo's contract we instead report and exit.
    // (examples/web/index.html performs the same probe before even
    // fetching the wasm - this is the defense for hosts that serve the
    // module differently.)
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
    // The asset root, as a URL path relative to the hosting page - see
    // `build_app`'s doc comment. examples/web/index.html is served one
    // directory below examples/assets/ (httpd rooted at `examples/`; see
    // examples/web/README.md), hence "../assets".
    let mut app = retro_crt::build_app("../assets");
    clamp_initial_window_resolution(&mut app);
    boot_status::register(&mut app);
    app.run();
}

/// Current wasm linear-memory size in MB (`memory_size` counts 64KiB
/// pages). This is the number iOS Safari's per-tab memory cap judges:
/// wasm memory only ever grows, so a climbing figure in the loading
/// heartbeat that ends in an "unreachable" RuntimeError is the signature
/// of an out-of-memory abort.
#[cfg(target_arch = "wasm32")]
fn wasm_heap_mb() -> usize {
    core::arch::wasm32::memory_size(0) * 64 / 1024
}

/// Forwards a milestone to `window.__demoStatus`. If the hook is absent
/// (a host serving the module without the overlay), silently no-ops.
/// File-scope (not inside `boot_status`) because the panic hook in
/// `wasm_main` forwards panic messages through it too.
#[cfg(target_arch = "wasm32")]
fn demo_status(msg: &str) {
    use wasm_bindgen::{JsCast, JsValue};
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(hook) = js_sys::Reflect::get(&window, &"__demoStatus".into()) else {
        return;
    };
    let Ok(hook) = hook.dyn_into::<js_sys::Function>() else {
        return;
    };
    let _ = hook.call1(&JsValue::NULL, &msg.into());
}

/// Boot-progress reporting to the page's loading overlay.
/// examples/web/index.html keeps the overlay up long past wasm-bindgen's
/// `init()` - the winit control-flow exception only means the event loop is
/// REGISTERED; renderer init, the synchronous WebGL2 shader compiles, and
/// all asset loads come after, on a still-black canvas - and waits for the
/// milestone strings sent from here via `window.__demoStatus(msg)`. The
/// special string `"ready"` makes it remove the overlay.
///
/// Lives here rather than in retro_crt.rs because it is pure browser
/// plumbing: the scene only exposes `build_app()` (so `register` can add
/// these systems before `run()`) and the `CrtMaterial` type.
#[cfg(target_arch = "wasm32")]
mod boot_status {
    use bevy::prelude::*;
    use bevy::world_serialization::WorldAssetRoot;

    use super::demo_status;
    use crate::retro_crt::{CrtMaterial, GltfAsset};

    /// Registers the milestone reporter.
    pub fn register(app: &mut App) {
        app.add_systems(Update, report_boot_status);
    }

    /// Once-per-second progress tick for a stage that is WAITING on
    /// something: `Some("<label> … 12s / heap 340 MB")` when the whole-second
    /// count advances (and at least a few seconds have passed - a fast boot
    /// never sees it), `None` otherwise. A climbing counter proves the main
    /// loop is alive (just slow); a frozen one means the tab stalled. The
    /// heap figure is for diagnosing out-of-memory aborts - see
    /// [`wasm_heap_mb`](super::wasm_heap_mb).
    fn heartbeat(label: &str, elapsed_secs: &mut f32, time: &Time) -> Option<String> {
        let before = *elapsed_secs as u32;
        *elapsed_secs += time.delta_secs();
        let now = *elapsed_secs as u32;
        (now > before && now >= 3).then(|| {
            format!("{label} … {now}s / heap {} MB", super::wasm_heap_mb())
        })
    }

    /// Drives the loading overlay's staged status text. Timing note: on wasm
    /// the main schedule and the render (where WebGL2 compiles shaders
    /// synchronously, freezing the tab) run inside the same rAF callback, so
    /// a message sent from an `Update` only paints AFTER that frame's
    /// compile stall - each stage's text describes what comes NEXT, and the
    /// text visible DURING a stall is whatever was set the frame before.
    ///
    /// - Stage 0: first `Update` ever - the base-pipeline compile stall is
    ///   this same frame's render; announce the asset loading that follows.
    /// - Stage 1: the glTF scene got spawned (`WorldAssetRoot` appeared).
    /// - Stage 2: the CRT material landed on the monitor mesh. Landing does
    ///   NOT mean the mesh renders yet: bevy skips a mesh whose specialized
    ///   pipeline isn't compiled, and `CrtMaterial`'s pipeline can't even
    ///   start compiling until `crt_extended.wgsl` finishes loading over
    ///   HTTP.
    /// - Stage 3: hold for a fixed `GRACE_PERIOD_SECONDS` after the material
    ///   landed, then send "ready" - index.html removes the overlay.
    ///
    ///   An earlier version of this stage waited for a render-world pipeline
    ///   cache counter to read zero instead of a fixed delay, to close the
    ///   overlay exactly when the CRT screen's own pipeline finished
    ///   compiling rather than guessing. That counted the ENTIRE
    ///   `PipelineCache`, though, not just the CRT screen's pipeline - and
    ///   in this scene (auto-orbit camera, moving shadows) something else
    ///   keeps getting re-specialized every frame, so the global count
    ///   never reliably reached zero. Confirmed in testing: the CRT screen
    ///   was already rendering correctly while the overlay lingered for the
    ///   full fallback timeout regardless. A plain fixed delay is both
    ///   simpler and - per that same testing - actually more accurate here.
    fn report_boot_status(
        scene_spawned: Query<(), Added<WorldAssetRoot>>,
        crt_material_added: Query<(), Added<MeshMaterial3d<CrtMaterial>>>,
        gltf_model: Query<&GltfAsset>,
        asset_server: Res<AssetServer>,
        time: Res<Time>,
        mut stage: Local<u8>,
        mut stage_elapsed_secs: Local<f32>,
        mut stage3_elapsed_secs: Local<f32>,
    ) {
        /// The CRT screen's own compile-and-first-render stall measured
        /// under 1.5s total (as `requestAnimationFrame` "Violation" log
        /// entries) on a fast native GPU in testing; tripled for headroom
        /// on slower devices/GPUs.
        const GRACE_PERIOD_SECONDS: f32 = 4.5;

        match *stage {
            0 => {
                demo_status("loading 3D model (2.5 MB) …");
                *stage = 1;
            }
            1 => {
                if !scene_spawned.is_empty() {
                    demo_status("compiling CRT shaders …");
                    *stage = 2;
                    *stage_elapsed_secs = 0.0;
                } else if let Ok(gltf) = gltf_model.single()
                    && let bevy::asset::LoadState::Failed(err) =
                        asset_server.load_state(&gltf.0)
                {
                    // A failed .glb fetch/parse otherwise stalls the overlay
                    // on "loading 3D model" forever with the reason visible
                    // only in the console - unreachable on mobile Safari
                    // without a tethered desktop inspector.
                    demo_status(&format!("failed to load 3D model: {err}"));
                    *stage = u8::MAX;
                } else if let Some(msg) =
                    heartbeat("loading 3D model (2.5 MB)", &mut stage_elapsed_secs, &time)
                {
                    demo_status(&msg);
                }
            }
            2 => {
                if !crt_material_added.is_empty() {
                    demo_status("waiting for material …");
                    *stage = 3;
                    *stage3_elapsed_secs = 0.0;
                } else if let Some(msg) =
                    heartbeat("compiling CRT shaders", &mut stage_elapsed_secs, &time)
                {
                    demo_status(&msg);
                }
            }
            3 => {
                *stage3_elapsed_secs += time.delta_secs();
                if *stage3_elapsed_secs >= GRACE_PERIOD_SECONDS {
                    demo_status("ready");
                    *stage = 4;
                }
            }
            _ => {}
        }
    }
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
// Deliberately below the real 2048 WebGL2-priority ceiling: the exact
// clamp (floor(2048/dpr) CSS px) leaves only a couple physical pixels
// of headroom (682*3 = 2046), and on browsers without
// devicePixelContentBox (Safari, iOS in particular) winit measures the
// physical size itself as css*devicePixelRatio with its own rounding -
// a fractional layout box (flex centering) or rounding-up there can
// tip the surface past 2048, whose Surface::configure validation
// failure leaves the surface unconfigured and the next
// get_current_texture panics ("Surface is not configured for
// presentation" - observed on iPhone Safari). 16 physical px of margin
// costs nothing visible and absorbs any such off-by-a-few measurement.
// Shared by both clamps below: the CSS ceiling on the live canvas and the
// pre-`run()` clamp of bevy's initial `WindowResolution`.
#[cfg(target_arch = "wasm32")]
const MAX_PHYSICAL_PX: f64 = 2032.0;

#[cfg(target_arch = "wasm32")]
fn clamp_canvas_to_safe_texture_size() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(canvas) = window.document().and_then(|d| d.get_element_by_id("bevy")) else {
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

// Even with `clamp_canvas_to_safe_texture_size`'s CSS ceiling in place, the
// FIRST frame's surface still blows past the texture limit on high-DPR
// phones: the CSS clamp only takes effect once winit's ResizeObserver
// reports a (clamped) canvas size, but bevy configures the initial surface
// from its own `WindowResolution` record before any such report arrives
// (winit's web backend never measures the canvas at creation - its cached
// size starts at 0x0 until the observer fires). `bevy_winit` hands
// retro_crt.rs's `WindowResolution::new(1024, 768)` to winit as a *logical*
// size (bevy_winit-0.19.0/src/winit_windows.rs:114-119 - no scale-factor
// override set, and the stored scale factor is still 1.0 at creation), the
// browser multiplies by the real devicePixelRatio, and the first
// `Surface::configure` requests 1024x768 x DPR - on an iPhone (DPR 3)
// that's 3072x2304, over the WebGL2-priority 2048 ceiling. The configure
// fails (bevy 0.19 catches it as a render error), the surface stays
// unconfigured, and the very next `get_current_texture` panics - all
// before `fit_canvas_to_parent` or the CSS clamp above ever get to correct
// the size. (DPR-2 desktop displays squeaked through by luck: 1024 x 2 =
// 2048, exactly at the limit.)
//
// So ALSO clamp bevy's own initial resolution by the real devicePixelRatio
// before `run()`, keeping every first-frame surface dimension within
// `MAX_PHYSICAL_PX`. The clamped value only lives for a frame or two -
// `fit_canvas_to_parent` plus the CSS ceiling take over as soon as the
// ResizeObserver reports - so the distorted aspect ratio is never visible.
// Same reasoning as the other browser-side fixes in this file for why this
// lives here and not in the shared retro_crt.rs: `WindowPlugin` has already
// spawned the `PrimaryWindow` entity by the time `build_app` returns, so
// its `Window` component can be rewritten from here before `run()`.
#[cfg(target_arch = "wasm32")]
fn clamp_initial_window_resolution(app: &mut bevy::app::App) {
    use bevy::prelude::With;
    use bevy::window::{PrimaryWindow, Window};

    let Some(window) = web_sys::window() else {
        return;
    };
    let dpr = window.device_pixel_ratio();
    if !(dpr > 0.0) {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_logical_px = (MAX_PHYSICAL_PX / dpr).floor() as u32;

    let world = app.world_mut();
    let mut primary = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    let Ok(mut bevy_window) = primary.single_mut(world) else {
        return;
    };
    // The resolution's scale factor is still 1.0 here (winit hasn't created
    // the window yet), so "physical" == the logical CSS size bevy_winit
    // will request from winit.
    let width = bevy_window.resolution.physical_width().min(max_logical_px);
    let height = bevy_window.resolution.physical_height().min(max_logical_px);
    bevy_window.resolution.set_physical_resolution(width, height);
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
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("bevy"))
    else {
        return;
    };

    // Leaked deliberately (`forget`): this listener must outlive the
    // function and live for the rest of the page's life, exactly like
    // wasm-bindgen's own generated glue does for its callbacks.
    let closure =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|event: web_sys::KeyboardEvent| {
            if event.key() == "Escape" {
                event.stop_immediate_propagation();
                event.prevent_default();
            }
        });
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
