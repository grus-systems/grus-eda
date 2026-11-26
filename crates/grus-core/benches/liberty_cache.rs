//! Times a cold Liberty parse against a warm cache load on the sky130 tt library.
//!
//! Prints both timings and asserts nothing. Run with `cargo bench --bench liberty_cache`.

use std::path::PathBuf;
use std::time::Instant;

use grus_eda_core::readers::liberty::{load_liberty, parse_liberty_bytes};

// Deliberately small copy of the lookup. The canonical logic is `test_support`
// (`#[cfg(test)]` in the crate); no public API is added for one benchmark.
fn sky130_tt_lib() -> PathBuf {
    let pdk_root = match (std::env::var_os("PDK_ROOT"), std::env::var_os("HOME")) {
        (Some(root), _) => PathBuf::from(root),
        (None, Some(home)) => PathBuf::from(home).join(".ciel"),
        (None, None) => fail("neither PDK_ROOT nor HOME is set; cannot locate the PDK"),
    };
    let lib = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib");
    if !lib.is_file() {
        fail(&format!("sky130 tt liberty file missing: {}", lib.display()));
    }
    lib
}

fn fail(msg: &str) -> ! {
    eprintln!("liberty_cache: {msg}");
    std::process::exit(1);
}

fn main() {
    let lib = sky130_tt_lib();
    let bytes = std::fs::read(&lib).unwrap_or_else(|e| fail(&format!("read {}: {e}", lib.display())));

    let t0 = Instant::now();
    parse_liberty_bytes(&bytes).unwrap_or_else(|e| fail(&format!("parse: {e}")));
    let cold = t0.elapsed();

    // The first load fills the cache entry (or hits an old one); the second is the warm path.
    load_liberty(&lib).unwrap_or_else(|e| fail(&format!("load: {e}")));
    let t1 = Instant::now();
    load_liberty(&lib).unwrap_or_else(|e| fail(&format!("load: {e}")));
    let warm = t1.elapsed();

    println!("cold parse (parse_liberty_bytes): {cold:?}");
    println!("warm cache load (load_liberty):   {warm:?}");
}
