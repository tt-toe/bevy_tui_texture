//! Web Server for WASM Browser Demo
//!
//! This serves the WebAssembly browser demo built with trunk.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example web_server
//! ```
//!
//! That's it! The server will automatically:
//! 1. Set up the necessary configuration files
//! 2. Build the WASM demo with trunk (if not already built)
//! 3. Start the web server at http://127.0.0.1:8080
//!
//! Open your browser to http://127.0.0.1:8080 to see the demo.
//!

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use tiny_http::{Header, Response, Server};

fn main() {
    // Check if WASM files exist, if not run trunk build
    let wasm_file = "examples/web/wasm_demo_bg.wasm";
    let index_file = "examples/web/index.html";

    if !Path::new(wasm_file).exists() || !Path::new(index_file).exists() {
        println!("📦 WASM files not found, running trunk build...");
        println!("   (Building from root index.html using Trunk.toml)");
        println!();

        let status = Command::new("trunk")
            .arg("build")
            .status();

        match status {
            Ok(exit_status) if exit_status.success() => {
                println!();
                println!("✅ Trunk build completed successfully!");
                println!();
            }
            Ok(exit_status) => {
                eprintln!("❌ Trunk build failed with status: {}", exit_status);
                eprintln!("Please run 'trunk build' manually to see the error.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("❌ Failed to run trunk: {}", e);
                eprintln!("Make sure trunk is installed: cargo install trunk");
                std::process::exit(1);
            }
        }
    }

    let server = Server::http("127.0.0.1:8080").unwrap();

    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ bevy_tui_texture - WASM Demo Server                         │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│ 🚀 Server running at http://127.0.0.1:8080                  │");
    println!("│ 📁 Serving from: examples/web/                              │");
    println!("│                                                             │");
    println!("│ Press Ctrl+C to stop the server                             │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!();

    for request in server.incoming_requests() {
        let url = request.url();
        // Strip query string if present
        let url_path = url.split('?').next().unwrap_or(url);
        println!("📥 {} {}", request.method(), url_path);
        io::stdout().flush().ok();

        // Map URL to file path
        let file_path_owned = if url_path == "/" || url_path.is_empty() {
            "examples/web/index.html".to_string()
        } else {
            format!("examples/web{}", url_path)
        };
        let file_path = file_path_owned.as_str();

        match fs::read(file_path) {
            Ok(content) => {
                let content_type = get_content_type(file_path);

                let mut response = Response::from_data(content);

                // Set content type
                if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
                    response = response.with_header(header);
                }

                // Add CORS headers for WASM
                if let Ok(header) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
                    response = response.with_header(header);
                }
                if let Ok(header) = Header::from_bytes(&b"Cross-Origin-Opener-Policy"[..], &b"same-origin"[..]) {
                    response = response.with_header(header);
                }
                if let Ok(header) = Header::from_bytes(&b"Cross-Origin-Embedder-Policy"[..], &b"require-corp"[..]) {
                    response = response.with_header(header);
                }

                // Cache control
                if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]) {
                    response = response.with_header(header);
                }

                request.respond(response).ok();
                println!("✅ Served: {} ({})", file_path, content_type);
            }
            Err(e) => {
                let error_html = format!(
                    r#"<!DOCTYPE html>
<html>
<head>
    <title>404 Not Found</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #1a1a1a;
            color: #e0e0e0;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
        }}
        .error {{
            text-align: center;
            max-width: 600px;
            padding: 40px;
            background: #252525;
            border-radius: 8px;
            border: 1px solid #333;
        }}
        h1 {{ color: #f44336; }}
        code {{
            background: #2d2d2d;
            padding: 2px 6px;
            border-radius: 3px;
            color: #4CAF50;
        }}
        .commands {{
            background: #0d1117;
            padding: 15px;
            border-radius: 4px;
            margin: 20px 0;
            text-align: left;
        }}
    </style>
</head>
<body>
    <div class="error">
        <h1>404 Not Found</h1>
        <p>File not found: <code>{}</code></p>
        <p style="margin-top: 20px;">Error: {}</p>

        <div class="commands">
            <p><strong>Did you build the WASM?</strong></p>
            <pre><code>trunk build</code></pre>
            <p style="margin-top: 10px;">This will create the files in <code>examples/web/</code></p>
        </div>

        <p style="font-size: 12px; color: #666; margin-top: 20px;">
            Server is looking for files in the <code>examples/web/</code> directory
        </p>
    </div>
</body>
</html>"#,
                    file_path, e
                );

                let response = Response::from_string(error_html)
                    .with_status_code(404)
                    .with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                            .unwrap(),
                    );

                request.respond(response).ok();
                println!("❌ Not found: {} ({})", file_path, e);
            }
        }
    }
}

fn get_content_type(path: &str) -> String {
    let path = Path::new(path);
    match path.extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("ttf") => "font/ttf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
    .to_string()
}
