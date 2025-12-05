// src/main.rs
mod scan;
mod ipc;
mod ui;

use gtk4::prelude::*;
use gtk4::Application;
use std::env;

fn main() {
    let arguments = env::args().collect::<Vec<String>>();

    // Tentukan mode aplikasi berdasarkan argumen
    match determine_application_mode(&arguments) {
        ApplicationMode::Worker => run_worker_mode(&arguments),
        ApplicationMode::GUI => run_gui_mode(),
    }
}

/// Enum untuk menentukan mode aplikasi
enum ApplicationMode {
    Worker,
    GUI,
}

/// Menentukan mode aplikasi berdasarkan argumen command line
fn determine_application_mode(arguments: &[String]) -> ApplicationMode {
    arguments
        .get(1)
        .filter(|arg| *arg == "--worker")
        .map(|_| ApplicationMode::Worker)
        .unwrap_or(ApplicationMode::GUI)
}

/// Menjalankan aplikasi dalam mode GUI
fn run_gui_mode() {
    let application = Application::new(
        Some("com.example.fscan_gui_stats"),
        Default::default(),
    );
    
    application.connect_activate(ui::build_ui);
    application.run();
}

/// Menjalankan aplikasi dalam mode worker
fn run_worker_mode(arguments: &[String]) {
    parse_worker_arguments(arguments)
        .and_then(|(folder_path, minimum_bytes)| scan_and_serialize(&folder_path, minimum_bytes))
        .map(|json_output| println!("{}", json_output))
        .unwrap_or_else(|error| handle_worker_error(error));
}

/// Parse argumen untuk mode worker
fn parse_worker_arguments(arguments: &[String]) -> Result<(std::path::PathBuf, u64), WorkerError> {
    // Validasi jumlah argumen
    validate_argument_count(arguments)?;
    
    let folder_path = std::path::PathBuf::from(&arguments[2]);
    let minimum_bytes = parse_minimum_bytes(&arguments[3])?;
    
    Ok((folder_path, minimum_bytes))
}

/// Validasi jumlah argumen yang diberikan
fn validate_argument_count(arguments: &[String]) -> Result<(), WorkerError> {
    (arguments.len() >= 4)
        .then_some(())
        .ok_or(WorkerError::InvalidArguments)
}

/// Parse string menjadi u64 untuk minimum bytes
fn parse_minimum_bytes(bytes_string: &str) -> Result<u64, WorkerError> {
    bytes_string
        .parse::<u64>()
        .map_err(|_| WorkerError::InvalidMinimumBytes)
}

/// Scan folder dan serialize hasilnya menjadi JSON
fn scan_and_serialize(
    folder_path: &std::path::PathBuf,
    minimum_bytes: u64,
) -> Result<String, WorkerError> {
    use crate::scan::scan_folder;
    use serde_json::to_string;
    
    scan_folder(folder_path, minimum_bytes)
        .map_err(WorkerError::ScanError)?
        .pipe(|stats| to_string(&stats))
        .map_err(WorkerError::SerializationError)
}

/// Handle error yang terjadi pada worker mode
fn handle_worker_error(error: WorkerError) {
    let (error_message, exit_code) = match error {
        WorkerError::InvalidArguments => {
            ("Usage: --worker <folder_path> <min_size_bytes>".to_string(), 1)
        }
        WorkerError::InvalidMinimumBytes => {
            ("Error: min_size_bytes harus berupa angka valid".to_string(), 1)
        }
        WorkerError::ScanError(message) => {
            (format!("Error saat scanning folder: {}", message), 3)
        }
        WorkerError::SerializationError(error) => {
            (format!("Error saat serialisasi JSON: {}", error), 2)
        }
    };
    
    eprintln!("{}", error_message);
    std::process::exit(exit_code);
}

/// Enum untuk berbagai jenis error pada worker mode
enum WorkerError {
    InvalidArguments,
    InvalidMinimumBytes,
    ScanError(String),
    SerializationError(serde_json::Error),
}

/// Extension trait untuk pipe pattern (functional programming style)
trait PipeExt {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
        Self: Sized,
    {
        f(self)
    }
}

// Implementasi PipeExt untuk semua tipe
impl<T> PipeExt for T {}