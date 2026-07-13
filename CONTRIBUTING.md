# Contributing to bevy_tui_texture

Thank you for your interest in contributing to bevy_tui_texture! This document provides guidelines and information for contributors.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Code Style](#code-style)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Project Structure](#project-structure)
- [Performance Guidelines](#performance-guidelines)

## Getting Started

1. **Fork the Repository** - Create your own fork of the project
2. **Clone Your Fork** - `git clone https://github.com/YOUR_USERNAME/bevy_tui_texture.git`
3. **Create a Branch** - `git checkout -b feature/your-feature-name`
4. **Make Changes** - Implement your feature or fix
5. **Test** - Run tests and examples to verify your changes
6. **Submit PR** - Push to your fork and create a pull request

## Development Setup

### Prerequisites

- Rust 1.96 or later (2024 edition)
- A GPU with WGPU support
- Basic familiarity with Bevy, ratatui, and WGPU

### Building the Project

```bash
# Clone the repository
git clone https://github.com/tt-toe/bevy_tui_texture.git
cd bevy_tui_texture

# Build the project
cargo build

# Run tests
cargo test

# Run an example
cargo run --example helloworld
```

### Running Examples

Examples are the best way to test your changes:

```bash
# Basic examples
cargo run --example helloworld
cargo run --example widget_catalog_2d
cargo run --example multiple_terminals

# 3D examples
cargo run --example widget_catalog_3d
cargo run --example shader_mesh
cargo run --example retro_crt

# Performance testing
cargo run --release --example benchmark
cargo run --release --example benchmark_partial
```

See README.md's "Examples" section for the full list, one per feature area.

## Code Style

### Rust Code Style

- Follow the [Rust Style Guide](https://doc.rust-lang.org/style-guide/)
- Use `cargo fmt` to format code before committing
- Use `cargo clippy` to catch common mistakes
- Maximum line length: 100 characters (soft limit)

### Documentation

- All public APIs must have documentation comments (`///`)
- Module-level documentation should explain purpose and architecture
- Examples should be included for non-trivial functions
- Update README.md if adding new features

Example (matching the actual style used in `src/setup.rs`):

```rust
/// Requests a resize to `cols` × `rows`. A no-op if the terminal is
/// already that size. No GPU work at the call site - the destination
/// `Image` is recreated in place, and the sibling `TerminalDimensions`
/// component (and, for a `TuiKind::WorldQuad`, the mesh's aspect ratio)
/// update automatically on the next `gpu_flush_system` pass.
///
/// # Example
///
/// ```no_run
/// # use bevy_tui_texture::prelude::*;
/// # fn resize(mut tui: bevy::prelude::Mut<Tui>) {
/// tui.request_resize(80, 25);
/// # }
/// ```
pub fn request_resize(&mut self, cols: u16, rows: u16) {
    // ...
}
```

## Testing

### Running Tests

```bash
# Run all tests
cargo test --all-features

# Run one test by name (works across every #[cfg(test)] module)
cargo test resize_to_the_current_size_is_a_no_op

# Run every test in one module
cargo test tui_flush_tests::

# Run tests with logging
RUST_LOG=debug cargo test --all-features
```

### Adding Tests

- There is **no dedicated `tests/` directory** - add unit tests as an
  inline `#[cfg(test)] mod ...` next to the code they cover (see
  `src/setup.rs`, `src/input/mod.rs`, `src/fonts.rs` for the existing
  pattern - often several test submodules per file, one per concern)
- Prefer testing pure-CPU logic directly (coordinate mapping, dirty
  tracking, font fallback order, hit-region decoding, ...) - almost
  everything in this crate can be tested with no GPU adapter at all
- If a test genuinely needs a GPU (a `RenderDevice`/`RenderQueue`), make it
  skip gracefully when no adapter is available rather than failing CI on
  headless runners - see `flush_renders_drawn_content_synchronously` in
  `src/setup.rs` for the pattern
- Test both success and error cases, including edge cases and boundary
  conditions

Example (matching the actual style used in `src/input/mod.rs`):

```rust
#[cfg(test)]
mod pixel_to_cell_tests {
    use super::super::pixel_to_cell;

    #[test]
    fn origin_maps_to_first_cell() {
        assert_eq!(pixel_to_cell(0.0, 0.0, 8.0, 16.0, 80, 24), (0, 0));
    }

    #[test]
    fn out_of_bounds_pixels_clamp_instead_of_wrapping_or_panicking() {
        assert_eq!(
            pixel_to_cell(-50.0, -50.0, 8.0, 16.0, 80, 24),
            (0, 0),
            "negative input must clamp to the first cell"
        );
    }
}
```

## Submitting Changes

### Pull Request Process

1. **Update Documentation** - Ensure all code changes are documented
2. **Add Tests** - Include tests for new features or bug fixes
3. **Run CI Checks** - Ensure `cargo test`, `cargo clippy`, and `cargo fmt` pass
4. **Update CHANGELOG** - Add entry describing your changes
5. **Create PR** - Provide clear description of changes and motivation

### PR Description Template

```markdown
## Description

Brief description of what this PR does.

## Motivation

Why is this change needed? What problem does it solve?

## Changes

- List of specific changes made
- Breaking changes (if any)

## Testing

- How was this tested?
- Which examples were run?

## Checklist

- [ ] Code follows project style guidelines
- [ ] Documentation updated
- [ ] Tests added/updated
- [ ] All tests pass
- [ ] Examples run successfully
```

## Project Structure

```
bevy_tui_texture/
├── src/
│   ├── lib.rs              # Crate entry point, prelude, public re-exports
│   ├── bevy_plugin.rs      # TerminalPlugin, TerminalSystemSet, render-world GPU dispatch
│   ├── setup.rs            # Tui, TerminalTexture, TuiRequest, AttachTerminal - the spawn/draw API
│   ├── fonts.rs            # Font loading, Fonts fallback slots, font identity
│   ├── colors.rs           # ratatui <-> RGB color conversion
│   ├── backend/
│   │   ├── mod.rs          # Backend module entry, GPU pipelines, texture atlas plumbing
│   │   ├── bevy_backend.rs # BevyTerminalBackend (the ratatui::backend::Backend impl)
│   │   ├── rasterize.rs    # Glyph rasterization (rustybuzz + raqote)
│   │   └── programmatic_glyphs/ # Box-drawing, braille, block elements, powerline
│   ├── input/
│   │   ├── mod.rs          # Input event handling, focus, hit-testing dispatch
│   │   └── ray.rs          # 3D mouse raycasting (feature `3d`)
│   └── utils/
│       ├── mod.rs          # Shared small utilities (outline geometry, ...)
│       ├── text_atlas.rs   # GPU glyph cache (LRU-evicted)
│       └── plan_cache.rs   # Text shaping cache
└── examples/               # Example applications, one per feature area (see README.md)
    ├── assets/             # Fonts, glTF model, shaders (shared by native + wasm)
    └── web/                # WASM demo site (index.html, generated wasm/js)
```

## Performance Guidelines

### General Principles

- `tui.draw(...)` is designed to be called every frame regardless of
  whether content changed - a byte-identical redraw is a free no-op
  (see CLAUDE.md's "Terminal Rendering" section), so don't build
  change-detection around it yourself
- Share one `Fonts` (`Arc<Fonts>`) across every terminal that uses the
  same font - they then also share one glyph atlas and rasterize each
  glyph only once, instead of paying for it per terminal
- Avoid creating new fonts or textures in hot loops
- Use `Arc` to share immutable data instead of cloning

### Profiling

Two benchmark examples target different workload shapes - see each
one's module doc comment for details:

```bash
# Full-frame throughput (every cell redrawn every frame)
cargo run --release --example benchmark

# Isolates the cost of unchanged-frame / partial-row redraws
BENCH_MODE=static cargo run --release --example benchmark_partial
BENCH_MODE=partial cargo run --release --example benchmark_partial

# Profile with perf (Linux)
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --example benchmark
perf record --call-graph dwarf ./target/release/examples/benchmark
perf report
```

Always compare `--release` numbers against `--release` numbers - debug
builds are not representative and must not be compared across runs.

## Areas for Contribution

Known incomplete spots, from the `TODO`s currently in the code:

- [ ] `src/backend/programmatic_glyphs/box_drawing.rs` - remaining
      box-drawing glyphs (U+250C-U+257F)
- [ ] `src/backend/programmatic_glyphs/block_elements.rs` - proper dark
      shade block pattern (currently an approximation)
- [ ] `src/backend/bevy_backend.rs` - text extraction from the backend's
      cell buffer (for accessibility / copy-paste support)

Beyond those:

- [ ] Additional examples demonstrating advanced features
- [ ] Better error messages and debugging tools
- [ ] Documentation improvements
- [ ] Alternative font backends

## Questions or Issues?

- Open an issue for bugs or feature requests
- Start a discussion for questions or ideas
- Check existing issues before creating new ones

## License

By contributing to bevy_tui_texture, you agree that your contributions will be licensed under the same terms as the project (MIT).

---

Thank you for contributing to bevy_tui_texture!
