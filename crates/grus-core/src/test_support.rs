//! Lookups shared by all test modules: PDK files and repository test designs.
//!
//! Missing inputs panic with the path they were looked up at; tests never skip.

use std::path::{Path, PathBuf};

/// `$PDK_ROOT`, else `$HOME/.ciel`.
pub fn pdk_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PDK_ROOT") {
        return PathBuf::from(root);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".ciel"),
        None => panic!("neither PDK_ROOT nor HOME is set; cannot locate the PDK (set PDK_ROOT)"),
    }
}

/// The sky130 `sky130_fd_sc_hd` library directory under the PDK root.
pub fn sky130_hd() -> PathBuf {
    pdk_root().join("sky130A/libs.ref/sky130_fd_sc_hd")
}

/// `tests/designs/<name>` in this repository.
pub fn design_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/designs")
        .join(name)
}

/// Returns `path` if it exists; otherwise panics naming the path and where inputs come from.
pub fn require(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if !path.exists() {
        panic!(
            "required test input missing: {} (PDK files come from PDK_ROOT, design inputs from tests/designs)",
            path.display()
        );
    }
    path.to_path_buf()
}
