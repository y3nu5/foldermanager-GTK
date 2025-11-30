// src/main.rs
mod scan;
mod ipc;
mod ui;

use gtk4::prelude::*;
use gtk4::Application;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Worker mode: --worker <folder> <min_bytes>
    if args.len() > 1 && args[1] == "--worker" {
        run_worker(&args);
        return;
    }

    // GUI mode
    let app = Application::new(Some("com.example.fscan_gui_stats"), Default::default());
    app.connect_activate(ui::build_ui);
    app.run();
}

fn run_worker(args: &[String]) {
    use crate::scan::scan_folder;
    use serde_json::to_string;

    if args.len() < 4 {
        eprintln!("Usage: --worker <folder_path> <min_size_bytes>");
        std::process::exit(1);
    }

    let folder = std::path::PathBuf::from(&args[2]);
    let min_bytes = args[3].parse::<u64>().unwrap_or(0);

    match scan_folder(&folder, min_bytes) {
        Ok(stats) => {
            match to_string(&stats) {
                Ok(json) => {
                    println!("{}", json);
                }
                Err(e) => {
                    eprintln!("serialization error: {}", e);
                    std::process::exit(2);
                }
            }
        }
        Err(err) => {
            eprintln!("scan error: {}", err);
            std::process::exit(3);
        }
    }
}
