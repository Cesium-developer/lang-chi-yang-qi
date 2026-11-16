use std::fs;
use std::path::Path;
use std::process::Command;

/// Read a WDL layer from its original raw file, or unpack the compact WDLZ
/// representation through the standard-library-only Python helper.
pub fn load_wdl_layer(dir: &Path, sheep: usize) -> Result<Vec<u8>, String> {
    let raw_path = dir.join(format!("wdl_s{sheep}.bin"));
    if raw_path.is_file() {
        return fs::read(&raw_path).map_err(|e| format!("read {}: {e}", raw_path.display()));
    }

    let packed_path = dir.join(format!("wdl_s{sheep}.wdlz"));
    if !packed_path.is_file() {
        return Err(format!("missing both {} and {}", raw_path.display(), packed_path.display()));
    }
    let helper = dir.join("table_storage.py");
    if !helper.is_file() {
        return Err(format!("missing WDLZ helper {}", helper.display()));
    }
    let temporary = dir.join(format!(".wdl_s{sheep}.{}.tmp.bin", std::process::id()));
    let mut last_error = String::new();
    let mut unpacked = false;
    for python in ["python", "python3", "py"] {
        match Command::new(python)
            .arg(&helper)
            .arg("unpack")
            .arg(&packed_path)
            .arg(&temporary)
            .status()
        {
            Ok(status) if status.success() => {
                unpacked = true;
                break;
            }
            Ok(status) => last_error = format!("{python} exited with {status}"),
            Err(error) => last_error = format!("could not start {python}: {error}"),
        }
    }
    if !unpacked {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not unpack {}: {last_error}", packed_path.display()));
    }
    let result = fs::read(&temporary).map_err(|e| format!("read {}: {e}", temporary.display()));
    let _ = fs::remove_file(&temporary);
    result
}

/// Convert and verify one freshly written .orpre, then remove its redundant
/// raw copy. The helper only uses Python's standard library.
pub fn pack_or_prefix(dir: &Path, sheep: usize) -> Result<(), String> {
    let helper = dir.join("table_storage.py");
    let mut last_error = String::new();
    for python in ["python", "py"] {
        match Command::new(python)
            .arg(&helper)
            .arg("pack-index")
            .arg(sheep.to_string())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => last_error = format!("{python} exited with {status}"),
            Err(error) => last_error = format!("could not start {python}: {error}"),
        }
    }
    Err(format!("could not compact s={sheep} OR prefix: {last_error}"))
}

