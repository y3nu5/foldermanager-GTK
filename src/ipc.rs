// src/ipc.rs
use crate::scan::FolderStats;
use serde_json;
use std::process::Command;

/// Spawn worker process (same exe) with args: --worker <path> <min_bytes>
/// Returns parsed FolderStats or error message
pub fn run_worker_scan(
    exe_path: &std::path::PathBuf,
    folder: &str,
    min_bytes: u64,
) -> Result<FolderStats, String> {
    // Jalankan proses worker dan tangkap outputnya
    let output = spawn_worker_process(exe_path, folder, min_bytes)?;
    
    // Validasi apakah proses berhasil
    validate_worker_success(&output)?;
    
    // Parse output JSON menjadi FolderStats
    parse_worker_output(&output.stdout)
}

/// Menjalankan proses worker sebagai child process
fn spawn_worker_process(
    exe_path: &std::path::PathBuf,
    folder: &str,
    min_bytes: u64,
) -> Result<std::process::Output, String> {
    Command::new(exe_path)
        .arg("--worker")
        .arg(folder)
        .arg(min_bytes.to_string())
        .output()
        .map_err(|error| format!("Gagal menjalankan worker process: {}", error))
}

/// Memvalidasi apakah worker process berhasil dijalankan
fn validate_worker_success(output: &std::process::Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        let error_message = String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string();
        Err(format!("Worker process gagal: {}", error_message))
    }
}

/// Mengparse output JSON dari worker menjadi FolderStats
fn parse_worker_output(stdout: &[u8]) -> Result<FolderStats, String> {
    let json_string = String::from_utf8_lossy(stdout);
    
    serde_json::from_str::<FolderStats>(&json_string)
        .map_err(|error| format!("Output JSON tidak valid dari worker: {}", error))
}