use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use humansize::{file_size_opts as options, FileSize};
use rayon::prelude::*;
use walkdir::WalkDir;

/// Struktur data hasil scan (immutable fields)
#[derive(Clone)]
pub struct FolderStats {
    pub total_size: u64,
    pub total_files: usize,
    pub extension_count: Vec<(String, usize)>, // (ext, count)
    pub filtered_files: Vec<(PathBuf, u64)>,   // (path, size)
}

/// Helper: konversi pilihan teks ke bytes
pub fn parse_filter_option(opt: &str, custom_text: Option<&str>) -> u64 {
    match opt {
        "> 100 MB" => 100 * 1024 * 1024,
        "> 500 MB" => 500 * 1024 * 1024,
        "> 1 GB" => 1 * 1024 * 1024 * 1024,
        "> 5 GB" => 5 * 1024 * 1024 * 1024,
        "Custom" => {
            if let Some(s) = custom_text {
                // expect format like "150 MB" or "1.5 GB" or just number in MB
                parse_human_input_to_bytes(s).unwrap_or(0)
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Parse simple human input (very forgiving): "150", "150 MB", "1.2 GB"
pub fn parse_human_input_to_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 1 {
        // maybe "150" assume MB
        if let Ok(n) = parts[0].parse::<f64>() {
            return Some((n * 1024.0 * 1024.0) as u64);
        }
    } else if let Ok(value) = parts[0].parse::<f64>() {
        let unit = parts[1];
        return match unit {
            "B" => Some(value as u64),
            "KB" => Some((value * 1024.0) as u64),
            "MB" => Some((value * 1024.0 * 1024.0) as u64),
            "GB" => Some((value * 1024.0 * 1024.0 * 1024.0) as u64),
            _ => None,
        };
    }
    None
}

/// Scan folder: tanpa variabel mutable yang terlihat di logic utama (menggunakan iterator + rayon)
pub fn scan_folder(path: &PathBuf, min_size_bytes: u64) -> Result<FolderStats, String> {
    // Kumpulkan semua file paths (ignores errors for individual entries)
    let paths: Vec<PathBuf> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    // Total size: parallel sum
    let total_size: u64 = paths
        .par_iter()
        .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

    // Total files
    let total_files = paths.len();

    // Count extensions: parallel folding into HashMap then merge
    let ext_map: HashMap<String, usize> = paths
        .par_iter()
        .map(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_else(|| "unknown".to_string())
        })
        .fold(
            || HashMap::new(),
            |mut acc: HashMap<String, usize>, ext| {
                *acc.entry(ext).or_insert(0) += 1;
                acc
            },
        )
        .reduce(
            || HashMap::new(),
            |mut a: HashMap<String, usize>, b: HashMap<String, usize>| {
                for (k, v) in b {
                    *a.entry(k).or_insert(0) += v;
                }
                a
            },
        );

    // Convert ext_map to sorted Vec<(ext, count)>
    let mut extension_count: Vec<(String, usize)> = ext_map.into_iter().collect();
    extension_count.sort_by(|a, b| b.1.cmp(&a.1)); // descending by count

    // Filter files by size (>= min_size_bytes)
    let filtered_files: Vec<(PathBuf, u64)> = paths
        .par_iter()
        .filter_map(|p| fs::metadata(p).ok().map(|m| (p.clone(), m.len())))
        .filter(|(_, sz)| *sz >= min_size_bytes)
        .collect();

    Ok(FolderStats {
        total_size,
        total_files,
        extension_count,
        filtered_files,
    })
}

pub fn format_bytes(bytes: u64) -> String {
    bytes
        .file_size(options::CONVENTIONAL)
        .unwrap_or_else(|_| format!("{} B", bytes))
}
