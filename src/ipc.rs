// src/ipc.rs
use crate::scan::FolderStats;
use serde_json;
use std::process::Command;

/// Spawn worker process (same exe) with args: --worker <path> <min_bytes>
/// Returns parsed FolderStats or error message
pub fn run_worker_scan(exe_path: &std::path::PathBuf, folder: &str, min_bytes: u64) -> Result<FolderStats, String> {
    let output = Command::new(exe_path)
        .arg("--worker")
        .arg(folder)
        .arg(min_bytes.to_string())
        .output()
        .map_err(|e| format!("failed to spawn worker: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("worker failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    serde_json::from_str::<FolderStats>(&stdout).map_err(|e| format!("invalid JSON from worker: {}", e))
}
