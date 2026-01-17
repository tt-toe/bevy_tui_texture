use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("wasm") => wasm(),
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}");
            eprintln!("Available commands: wasm");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("Usage: cargo xtask <command>");
            eprintln!("Available commands: wasm");
            ExitCode::FAILURE
        }
    }
}

fn wasm() -> ExitCode {
    // Step 1: Build WASM
    println!("Building WASM...");
    let status = Command::new("cargo")
        .args([
            "build",
            "--target", "wasm32-unknown-unknown",
            "--profile", "wasm-release",
            "--bin", "wasm_demo",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("Failed to build WASM");
        return ExitCode::FAILURE;
    }

    // Step 2: wasm-bindgen
    println!("Running wasm-bindgen...");
    let status = Command::new("wasm-bindgen")
        .args([
            "target/wasm32-unknown-unknown/wasm-release/wasm_demo.wasm",
            "--out-dir", "examples/web",
            "--target", "web",
            "--no-typescript",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("Failed to run wasm-bindgen");
        return ExitCode::FAILURE;
    }

    // Step 3: wasm-opt
    println!("Running wasm-opt...");
    let status = Command::new("wasm-opt")
        .args([
            "-Oz",
            "--vacuum",
            "--converge",
            "--strip-debug",
            "--strip-producers",
            "--strip-dwarf",
            "--enable-bulk-memory",
            "--enable-nontrapping-float-to-int",
            "examples/web/wasm_demo_bg.wasm",
            "-o", "examples/web/wasm_demo_bg.wasm",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("Failed to run wasm-opt");
        return ExitCode::FAILURE;
    }

    // Step 4: wasm-strip
    println!("Running wasm-strip...");
    let status = Command::new("wasm-strip")
        .args(["examples/web/wasm_demo_bg.wasm"])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("Failed to run wasm-strip");
        return ExitCode::FAILURE;
    }

    println!("WASM build complete!");
    ExitCode::SUCCESS
}
