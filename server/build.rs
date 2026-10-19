//! Guarantees that `rust-embed` always has a folder to embed.
//!
//! The dashboard assets are produced by `server-ui` (`vite build` writes
//! straight into `server/assets`). A Rust-only checkout would otherwise fail
//! to compile, so a placeholder page is materialised on first build. Running
//! the UI build replaces the whole directory, placeholder included.

use std::fs;
use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>Orbien dashboard</title>
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center;
         font: 15px/1.6 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; }
  main { max-width: 34rem; padding: 2rem; }
  code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  p { opacity: .75; }
</style>
</head>
<body>
<main>
  <h1>Orbien dashboard</h1>
  <p>The API is running, but this build has no compiled UI bundle.</p>
  <p>Build it with <code>cd server-ui &amp;&amp; npm ci &amp;&amp; npm run build</code>, then rebuild the server.</p>
</main>
</body>
</html>
"#;

fn main() {
    // Re-run when the asset directory appears, disappears or changes, so a
    // deleted `assets/` is recreated instead of breaking the next compile.
    println!("cargo:rerun-if-changed=assets");

    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let assets = Path::new(&manifest_dir).join("assets");
    let index = assets.join("index.html");

    if index.is_file() {
        return;
    }
    if let Err(err) = fs::create_dir_all(&assets).and_then(|_| fs::write(&index, PLACEHOLDER)) {
        println!("cargo:warning=could not create dashboard placeholder: {err}");
    }
}
