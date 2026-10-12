# Changelog

## 0.4.0

Dependencies: bevy 0.20, wgpu 30 (ratatui stays at 0.30.2). MSRV is now
Rust 1.97.1 (bevy 0.20's).

### Changed
- **Rendering is one instanced draw instead of two indexed passes.** Each
  glyph is a single 20-byte instance (cell rectangle, atlas slot, foreground
  and background color) expanded to a quad in the vertex shader; the
  fragment shader resolves background and glyph together and writes the
  cell with `REPLACE` blending. Compared to 0.3 that removes the separate
  background pass (every pixel is shaded once, not twice), the index
  buffer, per-fragment color unpacking, and 7/8 of the geometry generated
  and uploaded per glyph (160 bytes of vertices before).
- **The glyph atlas is `R8Unorm` instead of `Rgba8Unorm`.** Glyphs were
  already used as coverage masks only, so this is the same output with a
  quarter of the atlas memory (4 MB instead of 16 MB per font) and upload
  bandwidth.
- **Shaped text is cached.** Shaped runs are kept in an LRU keyed by
  `(font, text)`, so a row whose colors changed, or content that scrolled
  to a different row, is no longer reshaped by rustybuzz. Per-cell font
  fallback is memoized per `(char, bold, italic)`.
- **Atlas lookups no longer maintain an LRU list.** A hit is one hash probe
  plus a last-used stamp; eviction (only when the atlas is full) is a clock
  sweep over the stamps that never takes a glyph the current flush used
  unless nothing else is left.
- The terminal render, its submit and the blocking readback are now ordered
  explicitly. bevy 0.20 only weakly orders the `RenderGraphSystems` sets,
  which left `Tui::read_back_blocking` free to run before the submit (it
  returned an empty texture) and no longer guaranteed same-frame display.
- `TerminalDimensions` carries a column summary tick (bevy 0.20), and a
  terminal with nothing to flush is no longer flagged as changed every
  frame.
- Example shaders are WESL (`.wesl`), as bevy 0.20 requires for shaders
  that import bevy modules.
- Removed the unused `indexmap`, `web-time` and (wasm) `getrandom`
  dependencies; `evictor` 0.7, `tiny-skia` 0.12, `rand` 0.10 (examples).

CPU cost of a dirty frame on a 120x40 terminal (`cargo test --release
--lib cpu_hot_path -- --ignored --nocapture`, Apple M1, microseconds per
frame, includes ratatui's own buffer diff of ~70):

| workload                          | 0.3.5 | 0.4.0 |
|-----------------------------------|------:|------:|
| every cell: scrolling text+colors |  1350 |   290 |
| every cell: colors only           |  1190 |   265 |
| every cell: block glyph gradient  |  1985 |   385 |
| every cell: never-seen text       |  1485 |  1080 |
| one row changes                   |   100 |    75 |
| nothing changes                   |    74 |    69 |

`examples/benchmark` (full-frame redraw, uncapped, same machine) went from
about 250 to about 390 fps.

### Fixed
- Glyphs rasterized in a frame whose draw payload was not extracted that
  same frame were dropped before reaching the GPU atlas and stayed
  invisible for good (e.g. text first printed after startup).
- `gpu_flush_system` is now ordered after `TerminalSystemSet::Render`
  instead of running unordered inside it, so a draw made in that set can
  no longer reach the GPU a frame late.
- Font fallback now works per cell: rows were shaped entirely with the
  font chosen for their first cell, so fonts added via
  `Fonts::add_regular_fonts` were never used past column 0.
- Glyphs from a fallback font wider than the cell width are squeezed
  horizontally to fit instead of having their right side clipped.

