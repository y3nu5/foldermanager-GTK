// src/scan.rs
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;
use humansize::{file_size_opts as options, FileSize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderStats {
    pub total_size: u64,
    pub total_files: usize,
    pub extension_count: Vec<(String, usize)>,
    pub filtered_files: Vec<FileEntry>,
}

/// parsing filter text -> bytes
pub fn parse_filter_option(opt: &str, custom_text: Option<&str>) -> u64 {
    match opt {
        "100 MB" => 100 * 1024 * 1024,
        "500 MB" => 500 * 1024 * 1024,
        "1 GB" => 1 * 1024 * 1024 * 1024,
        "5 GB" => 5 * 1024 * 1024 * 1024,
        "Custom" => {
            if let Some(s) = custom_text {
                parse_human_input_to_bytes(s).unwrap_or(0)
            } else {
                0
            }
        }
        _ => 0,
    }
}

pub fn parse_human_input_to_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 1 {
        if let Ok(n) = parts[0].parse::<f64>() {
            return Some((n * 1024.0 * 1024.0) as u64); // assume MB
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

/// scan_folder: returns FolderStats
/// - uses parallel iterators (rayon)
/// - minimal mutable: local fold usage (safe)
pub fn scan_folder(path: &PathBuf, min_size_bytes: u64) -> Result<FolderStats, String> {
    let paths: Vec<PathBuf> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    let total_size: u64 = paths
        .par_iter()
        .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

    let total_files = paths.len();

    // count extensions via parallel fold + reduce
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

    let mut extension_count: Vec<(String, usize)> = ext_map.into_iter().collect();
    extension_count.sort_by(|a, b| b.1.cmp(&a.1));

    // filtered files -> FileEntry
    let filtered_files: Vec<FileEntry> = paths
        .par_iter()
        .filter_map(|p| fs::metadata(p).ok().map(|m| (p.clone(), m.len())))
        .filter(|(_, sz)| *sz >= min_size_bytes)
        .map(|(p, sz)| FileEntry {
            path: p.to_string_lossy().into_owned(),
            size: sz,
        })
        .collect();

    Ok(FolderStats {
        total_size,
        total_files,
        extension_count,
        filtered_files,
    })
}

/// helper format human readable
pub fn format_bytes(bytes: u64) -> String {
    bytes
        .file_size(options::CONVENTIONAL)
        .unwrap_or_else(|_| format!("{} B", bytes))
}
