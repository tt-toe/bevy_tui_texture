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

- Rust 1.75 or later (2024 edition)
- A GPU with WGPU support
- Basic familiarity with Bevy, ratatui, and WGPU

### Building the Project

```bash
# Clone the repository
git clone https://github.com/YOUR_USERNAME/bevy_tui_texture.git
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
cargo run --example terminal_texture_3d

# Performance testing
cargo run --example benchmark --release
```

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

Example:

```rust
/// Creates a new terminal with the specified dimensions.
///
/// # Arguments
///
/// * `cols` - Number of columns (characters wide)
/// * `rows` - Number of rows (characters tall)
///
/// # Returns
///
/// Returns `Ok(Terminal)` on success, or an error message on failure.
///
/// # Example
///
/// ```no_run
/// let terminal = create_terminal(80, 25)?;
/// ```
pub fn create_terminal(cols: u16, rows: u16) -> Result<Terminal> {
    // ...
}
```

### Commit Messages

Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
feat: add SimpleTerminal3D support for mesh materials
fix: correct mouse coordinate mapping in 3D terminals
docs: update README with new examples
perf: optimize glyph cache eviction algorithm
refactor: simplify terminal texture creation
test: add tests for keyboard input handling
```

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_terminal_creation

# Run tests with logging
RUST_LOG=debug cargo test
```

### Adding Tests

- Add unit tests in the same file as the code being tested
- Add integration tests in `tests/` directory
- Test both success and error cases
- Include edge cases and boundary conditions

Example:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_dimensions() {
        let terminal = create_terminal(80, 25).unwrap();
        assert_eq!(terminal.cols(), 80);
        assert_eq!(terminal.rows(), 25);
    }

    #[test]
    fn test_invalid_dimensions() {
        let result = create_terminal(0, 0);
        assert!(result.is_err());
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
│   ├── lib.rs              # Main library entry point
│   ├── bevy_plugin.rs      # Bevy plugin and resources
│   ├── setup.rs            # SimpleTerminal2D/3D utilities
│   ├── fonts.rs            # Font loading and management
│   ├── colors.rs           # Color conversion utilities
│   ├── backend/
│   │   ├── mod.rs          # Backend module entry
│   │   ├── bevy_backend.rs # Main ratatui backend
│   │   ├── rasterize.rs    # Glyph rasterization
│   │   └── programmatic_glyphs/ # Box-drawing, braille, etc.
│   ├── input/
│   │   ├── mod.rs          # Input event handling
│   │   └── ray.rs          # 3D mouse raycasting
│   └── utils/
│       ├── text_atlas.rs   # GPU glyph cache
│       └── plan_cache.rs   # Text shaping cache
├── examples/               # Example applications
├── assets/                 # Fonts and resources
└── tests/                  # Integration tests
```

## Performance Guidelines

### General Principles

- Minimize GPU-CPU data transfers
- Batch rendering operations when possible
- Use dirty tracking to avoid unnecessary work
- Cache frequently used data

### Profiling

Use the benchmark example to test performance:

```bash
# Run performance benchmark
cargo run --example benchmark --release

# Profile with perf (Linux)
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --example benchmark
perf record --call-graph dwarf ./target/release/examples/benchmark
perf report
```

### Common Performance Pitfalls

- Don't call `terminal.draw()` every frame if content hasn't changed
- Avoid creating new fonts or textures in hot loops
- Use `Arc` to share immutable data instead of cloning
- Prefer async GPU operations over blocking transfers

## Areas for Contribution

### High Priority

- [ ] WASM support improvements
- [ ] More comprehensive test coverage
- [ ] Performance optimizations for large terminals
- [ ] Additional examples demonstrating advanced features

### Medium Priority

- [ ] Better error messages and debugging tools
- [ ] Documentation improvements
- [ ] CI/CD pipeline enhancements
- [ ] Additional programmatic glyph sets

### Low Priority

- [ ] Alternative font backends
- [ ] Theme/color scheme presets
- [ ] Animation utilities
- [ ] Terminal recording/playback

## Questions or Issues?

- Open an issue for bugs or feature requests
- Start a discussion for questions or ideas
- Check existing issues before creating new ones

## License

By contributing to bevy_tui_texture, you agree that your contributions will be licensed under the same terms as the project (MIT).

---

Thank you for contributing to bevy_tui_texture!
