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

/// Konstanta untuk konversi ukuran byte
const BYTES_PER_KB: u64 = 1024;
const BYTES_PER_MB: u64 = BYTES_PER_KB * 1024;
const BYTES_PER_GB: u64 = BYTES_PER_MB * 1024;

/// Parsing opsi filter text menjadi bytes
/// Menggunakan pattern matching untuk menghindari if-else
pub fn parse_filter_option(opsi_filter: &str, teks_custom: Option<&str>) -> u64 {
    match opsi_filter {
        "100 MB" => 100 * BYTES_PER_MB,
        "500 MB" => 500 * BYTES_PER_MB,
        "1 GB" => BYTES_PER_GB,
        "5 GB" => 5 * BYTES_PER_GB,
        "Custom" => teks_custom
            .and_then(parse_human_input_to_bytes)
            .unwrap_or(0),
        _ => 0,
    }
}

/// Parse input manusia (contoh: "500 MB", "1.5 GB") menjadi bytes
/// Menggunakan functional style dengan map dan and_then
pub fn parse_human_input_to_bytes(input_string: &str) -> Option<u64> {
    let input_bersih = input_string.trim().to_uppercase();
    
    // Guard clause: return None jika string kosong
    (!input_bersih.is_empty()).then(|| ())?;
    
    let bagian_input: Vec<&str> = input_bersih.split_whitespace().collect();
    
    // Pattern matching berdasarkan jumlah bagian input
    match bagian_input.len() {
        1 => parse_angka_tanpa_unit(&bagian_input[0]),
        2 => parse_angka_dengan_unit(&bagian_input[0], &bagian_input[1]),
        _ => None,
    }
}

/// Parse angka tanpa unit (asumsi: MB)
fn parse_angka_tanpa_unit(angka_string: &str) -> Option<u64> {
    angka_string
        .parse::<f64>()
        .ok()
        .map(|nilai| (nilai * BYTES_PER_MB as f64) as u64)
}

/// Parse angka dengan unit (contoh: "500 MB", "1.5 GB")
fn parse_angka_dengan_unit(angka_string: &str, unit: &str) -> Option<u64> {
    angka_string
        .parse::<f64>()
        .ok()
        .and_then(|nilai| convert_dengan_unit(nilai, unit))
}

/// Konversi nilai dengan unit ke bytes
fn convert_dengan_unit(nilai: f64, unit: &str) -> Option<u64> {
    let pengali = match unit {
        "B" => 1,
        "KB" => BYTES_PER_KB,
        "MB" => BYTES_PER_MB,
        "GB" => BYTES_PER_GB,
        _ => return None,
    };
    
    Some((nilai * pengali as f64) as u64)
}

/// Scan folder dan kembalikan statistik
/// Menggunakan parallel iterators (rayon) untuk performa optimal
pub fn scan_folder(
    path_folder: &PathBuf,
    ukuran_minimum_bytes: u64,
) -> Result<FolderStats, String> {
    // Kumpulkan semua path file dalam folder
    let daftar_path_file = collect_semua_file_paths(path_folder);
    
    // Hitung total ukuran semua file secara paralel
    let total_ukuran = hitung_total_ukuran_file(&daftar_path_file);
    
    // Hitung jumlah file
    let jumlah_total_file = daftar_path_file.len();
    
    // Hitung jumlah file per ekstensi secara paralel
    let jumlah_per_ekstensi = hitung_ekstensi_file(&daftar_path_file);
    
    // Filter file berdasarkan ukuran minimum
    let daftar_file_terfilter = filter_file_berdasarkan_ukuran(
        &daftar_path_file,
        ukuran_minimum_bytes,
    );
    
    Ok(FolderStats {
        total_size: total_ukuran,
        total_files: jumlah_total_file,
        extension_count: jumlah_per_ekstensi,
        filtered_files: daftar_file_terfilter,
    })
}

/// Kumpulkan semua path file dari folder menggunakan WalkDir
fn collect_semua_file_paths(path_folder: &PathBuf) -> Vec<PathBuf> {
    WalkDir::new(path_folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect()
}

/// Hitung total ukuran semua file secara paralel
fn hitung_total_ukuran_file(daftar_path: &[PathBuf]) -> u64 {
    daftar_path
        .par_iter()
        .map(ambil_ukuran_file)
        .sum()
}

/// Ambil ukuran file, return 0 jika gagal
fn ambil_ukuran_file(path_file: &PathBuf) -> u64 {
    fs::metadata(path_file)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

/// Hitung jumlah file per ekstensi menggunakan parallel fold+reduce
/// Hasilnya diurutkan berdasarkan jumlah (descending)
fn hitung_ekstensi_file(daftar_path: &[PathBuf]) -> Vec<(String, usize)> {
    let map_ekstensi = daftar_path
        .par_iter()
        .map(ekstrak_ekstensi_file)
        .fold(HashMap::new, tambahkan_ekstensi_ke_map)
        .reduce(HashMap::new, gabungkan_map_ekstensi);
    
    konversi_dan_urutkan_map_ekstensi(map_ekstensi)
}

/// Ekstrak ekstensi file dari path, return "unknown" jika tidak ada
fn ekstrak_ekstensi_file(path_file: &PathBuf) -> String {
    path_file
        .extension()
        .and_then(|os_str| os_str.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Tambahkan ekstensi ke HashMap (fold operation)
fn tambahkan_ekstensi_ke_map(
    mut map_akumulasi: HashMap<String, usize>,
    ekstensi: String,
) -> HashMap<String, usize> {
    *map_akumulasi.entry(ekstensi).or_insert(0) += 1;
    map_akumulasi
}

/// Gabungkan dua HashMap ekstensi (reduce operation)
fn gabungkan_map_ekstensi(
    mut map_pertama: HashMap<String, usize>,
    map_kedua: HashMap<String, usize>,
) -> HashMap<String, usize> {
    for (ekstensi, jumlah) in map_kedua {
        *map_pertama.entry(ekstensi).or_insert(0) += jumlah;
    }
    map_pertama
}

/// Konversi HashMap ke Vec dan urutkan berdasarkan jumlah (descending)
fn konversi_dan_urutkan_map_ekstensi(
    map_ekstensi: HashMap<String, usize>,
) -> Vec<(String, usize)> {
    let mut hasil_vector: Vec<(String, usize)> = map_ekstensi.into_iter().collect();
    hasil_vector.sort_by(|a, b| b.1.cmp(&a.1));
    hasil_vector
}

/// Filter file berdasarkan ukuran minimum dan konversi ke FileEntry
fn filter_file_berdasarkan_ukuran(
    daftar_path: &[PathBuf],
    ukuran_minimum: u64,
) -> Vec<FileEntry> {
    daftar_path
        .par_iter()
        .filter_map(|path| ambil_path_dan_ukuran(path))
        .filter(|(_, ukuran)| *ukuran >= ukuran_minimum)
        .map(konversi_ke_file_entry)
        .collect()
}

/// Ambil path dan ukuran file, return None jika gagal
fn ambil_path_dan_ukuran(path_file: &PathBuf) -> Option<(PathBuf, u64)> {
    fs::metadata(path_file)
        .ok()
        .map(|metadata| (path_file.clone(), metadata.len()))
}

/// Konversi tuple (PathBuf, u64) menjadi FileEntry
fn konversi_ke_file_entry((path_file, ukuran): (PathBuf, u64)) -> FileEntry {
    FileEntry {
        path: path_file.to_string_lossy().into_owned(),
        size: ukuran,
    }
}

/// Format bytes menjadi human-readable string (contoh: "1.5 GB")
pub fn format_bytes(jumlah_bytes: u64) -> String {
    jumlah_bytes
        .file_size(options::CONVENTIONAL)
        .unwrap_or_else(|_| format!("{} B", jumlah_bytes))
}