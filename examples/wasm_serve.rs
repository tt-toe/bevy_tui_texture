//! WASM Build & Serve - Complete One-Command Solution
//!
//! This example builds the WASM demo and serves it locally in one command.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example wasm_serve
//! ```
//!
//! This will:
//! 1. Check if required WASM tools are installed
//! 2. Build the WASM target with optimizations
//! 3. Process with wasm-bindgen, wasm-opt, and wasm-strip
//! 4. Start a local web server at http://127.0.0.1:8080
//! 5. Open your browser to view the demo

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode};
use tiny_http::{Header, Response, Server};

fn main() -> ExitCode {
    println!("🚀 WASM Build & Serve");
    println!("===================");
    println!();

    // Step 1: Check if required tools are available
    if !check_wasm_tools() {
        return ExitCode::FAILURE;
    }

    // Step 2: Build WASM if needed
    if !check_wasm_exists() || should_rebuild() {
        if !build_wasm() {
            return ExitCode::FAILURE;
        }
    } else {
        println!("✅ WASM files up to date");
        println!();
    }

    // Step 3: Start web server
    start_server()
}

/// Check if a command is available on the system
fn check_command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn check_wasm_tools() -> bool {
    let required_tools = ["wasm-bindgen", "wasm-opt", "wasm-strip"];
    let mut all_available = true;

    for tool in &required_tools {
        if check_command_available(tool) {
            println!("✅ {} available", tool);
        } else {
            println!("❌ {} not found in PATH", tool);
            println!("   Install with: cargo install wasm-bindgen-cli");
            println!("   And install wasm-opt, wasm-strip from binaryen");
            all_available = false;
        }
    }

    if all_available {
        println!();
    } else {
        println!();
        println!("Please install missing tools and try again.");
    }

    all_available
}

fn check_wasm_exists() -> bool {
    let wasm_file = "examples/web/wasm_demo_bg.wasm";
    let js_file = "examples/web/wasm_demo.js";
    let index_file = "examples/web/index.html";

    Path::new(wasm_file).exists() && Path::new(js_file).exists() && Path::new(index_file).exists()
}

fn should_rebuild() -> bool {
    // Simple check: if source is newer than output
    let source_file = "examples/wasm_demo.rs";
    let output_file = "examples/web/wasm_demo_bg.wasm";

    if let (Ok(source_meta), Ok(output_meta)) =
        (fs::metadata(source_file), fs::metadata(output_file))
    {
        if let (Ok(source_time), Ok(output_time)) = (source_meta.modified(), output_meta.modified())
        {
            return source_time > output_time;
        }
    }

    false
}

fn build_wasm() -> bool {
    println!("🔨 Building WASM...");

    // Create output directory
    if let Err(e) = fs::create_dir_all("examples/web") {
        eprintln!("Failed to create output directory: {}", e);
        return false;
    }

    // Step 1: Build WASM
    println!("   1/4 Compiling to WASM...");
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--profile",
            "wasm-release",
            "--example",
            "wasm_demo",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("❌ Failed to build WASM");
        return false;
    }

    // Step 2: wasm-bindgen
    println!("   2/4 Running wasm-bindgen...");
    let status = Command::new("wasm-bindgen")
        .args([
            "target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm",
            "--out-dir",
            "examples/web",
            "--target",
            "web",
            "--no-typescript",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("❌ Failed to run wasm-bindgen");
        return false;
    }

    // Step 3: wasm-opt
    println!("   3/4 Optimizing with wasm-opt...");
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
            "-o",
            "examples/web/wasm_demo_bg.wasm",
        ])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("❌ Failed to run wasm-opt");
        return false;
    }

    // Step 4: wasm-strip
    println!("   4/4 Stripping with wasm-strip...");
    let status = Command::new("wasm-strip")
        .args(["examples/web/wasm_demo_bg.wasm"])
        .status();

    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("❌ Failed to run wasm-strip");
        return false;
    }

    // Create index.html if it doesn't exist
    create_index_html();

    println!("✅ WASM build complete!");
    println!();

    true
}

fn create_index_html() {
    let index_path = "examples/web/index.html";
    if !Path::new(index_path).exists() {
        let html_content = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>bevy_tui_texture WASM Demo</title>
    <style>
        body {
            margin: 0;
            padding: 0;
            background: #1a1a1a;
            color: white;
            font-family: monospace;
            display: flex;
            flex-direction: column;
            height: 100vh;
        }
        .header {
            background: #2d2d2d;
            padding: 10px 20px;
            border-bottom: 2px solid #4a4a4a;
        }
        .content {
            flex: 1;
            display: flex;
            justify-content: center;
            align-items: center;
        }
        canvas {
            background: #000;
        }
        .loading {
            text-align: center;
        }
        .error {
            color: #ff6b6b;
            text-align: center;
        }
    </style>
</head>
<body>
    <div class="header">
        <h1>🖥️ bevy_tui_texture - WASM Demo</h1>
        <p>Terminal UI rendering in the browser with Bevy + ratatui + WGPU</p>
    </div>
    <div class="content">
        <div class="loading" id="loading">
            <p>Loading WASM module...</p>
            <p style="font-size: 12px;">This may take a moment on first load</p>
        </div>
        <div class="error" id="error" style="display: none;">
            <p>Failed to load WASM module</p>
            <p style="font-size: 12px;">Check browser console for details</p>
        </div>
    </div>

    <script type="module">
        import init from './wasm_demo.js';

        async function run() {
            try {
                await init();
                document.getElementById('loading').style.display = 'none';
                console.log('WASM module loaded successfully');
            } catch (e) {
                console.error('Failed to load WASM:', e);
                document.getElementById('loading').style.display = 'none';
                document.getElementById('error').style.display = 'block';
            }
        }

        run();
    </script>
</body>
</html>"#;

        if let Err(e) = fs::write(index_path, html_content) {
            eprintln!("Warning: Failed to create index.html: {}", e);
        }
    }
}

fn start_server() -> ExitCode {
    let server = match Server::http("127.0.0.1:8080") {
        Ok(server) => server,
        Err(e) => {
            eprintln!("Failed to start server: {}", e);
            return ExitCode::FAILURE;
        }
    };

    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 🚀 bevy_tui_texture - WASM Demo Server                      │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│ 📡 Server: http://127.0.0.1:8080                            │");
    println!("│ 📁 Files:  examples/web/                                    │");
    println!("│ 🎮 Demo:   Terminal UI in your browser!                     │");
    println!("│                                                             │");
    println!("│ Press Ctrl+C to stop                                        │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!();

    // Try to open browser
    let _ = open_browser("http://127.0.0.1:8080");

    for request in server.incoming_requests() {
        let url = request.url();
        let url_path = url.split('?').next().unwrap_or(url);
        print!("📥 {} {} ", request.method(), url_path);
        io::stdout().flush().ok();

        let file_path = if url_path == "/" || url_path.is_empty() {
            "examples/web/index.html".to_string()
        } else {
            format!("examples/web{}", url_path)
        };

        match fs::read(&file_path) {
            Ok(content) => {
                let content_type = get_content_type(&file_path);
                let mut response = Response::from_data(content);

                // Add headers
                if let Ok(header) = Header::from_bytes(b"Content-Type", content_type.as_bytes()) {
                    response = response.with_header(header);
                }
                if let Ok(header) =
                    Header::from_bytes(b"Cross-Origin-Opener-Policy", b"same-origin")
                {
                    response = response.with_header(header);
                }
                if let Ok(header) =
                    Header::from_bytes(b"Cross-Origin-Embedder-Policy", b"require-corp")
                {
                    response = response.with_header(header);
                }

                request.respond(response).ok();
                println!("✅");
            }
            Err(_) => {
                let response = Response::from_string("404 Not Found").with_status_code(404);
                request.respond(response).ok();
                println!("❌");
            }
        }
    }

    ExitCode::SUCCESS
}

fn get_content_type(path: &str) -> String {
    let path = Path::new(path);
    match path.extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn open_browser(url: &str) {
    let _ = match std::env::consts::OS {
        "windows" => Command::new("cmd").args(["/c", "start", url]).status(),
        "macos" => Command::new("open").arg(url).status(),
        _ => Command::new("xdg-open").arg(url).status(),
    };
}
