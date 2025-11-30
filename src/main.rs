use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, Entry, Label, ListBox,
    ListBoxRow, Orientation, ScrolledWindow, SelectionMode, Spinner, FileChooserAction,
    FileChooserNative, ResponseType, Widget,
};
use glib::{MainContext, Continue, PRIORITY_DEFAULT};
use std::path::PathBuf;
use std::thread;
use walkdir::WalkDir;
use rayon::prelude::*;
use std::fs;
use humansize::{FileSize, file_size_opts as options};
use std::collections::HashMap;

/// Struktur data hasil scan (immutable fields)
#[derive(Clone)]
struct FolderStats {
    total_size: u64,
    total_files: usize,
    extension_count: Vec<(String, usize)>, // (ext, count)
    filtered_files: Vec<(PathBuf, u64)>,    // (path, size)
}

/// Helper: konversi pilihan teks ke bytes
fn parse_filter_option(opt: &str, custom_text: Option<&str>) -> u64 {
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
fn parse_human_input_to_bytes(s: &str) -> Option<u64> {
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
    } else {
        if let Ok(value) = parts[0].parse::<f64>() {
            let unit = parts[1];
            return match unit {
                "B" => Some(value as u64),
                "KB" => Some((value * 1024.0) as u64),
                "MB" => Some((value * 1024.0 * 1024.0) as u64),
                "GB" => Some((value * 1024.0 * 1024.0 * 1024.0) as u64),
                _ => None,
            };
        }
    }
    None
}

/// Scan folder: tanpa variabel mutable yang terlihat di logic utama (menggunakan iterator + rayon)
fn scan_folder(path: &PathBuf, min_size_bytes: u64) -> Result<FolderStats, String> {
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

fn format_bytes(bytes: u64) -> String {
    bytes
        .file_size(options::CONVENTIONAL)
        .unwrap_or_else(|_| format!("{} B", bytes))
}

/// Helper GTK4: ambil semua child dari ListBox (GTK4 tidak punya .children())
fn listbox_children(lb: &ListBox) -> Vec<Widget> {
    let mut out = Vec::new();
    let mut current = lb.first_child();
    while let Some(widget) = current {
        out.push(widget.clone());
        current = widget.next_sibling();
    }
    out
}

fn main() {
    let app = Application::new(Some("com.example.fscan_gui_stats"), Default::default());
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    // Window
    let window = ApplicationWindow::new(app);
    window.set_title(Some("fscan - Folder Stats"));
    window.set_default_size(800, 600);

    // Root container
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    // Row: entry + choose + filter + calc
    let row = GtkBox::new(Orientation::Horizontal, 8);

    let entry = Entry::new();
    entry.set_placeholder_text(Some("Masukkan path folder atau pilih..."));
    entry.set_hexpand(true);

    let choose_btn = Button::with_label("Pilih Folder");

    let filter_combo = ComboBoxText::new();
    filter_combo.append_text("> 100 MB");
    filter_combo.append_text("> 500 MB");
    filter_combo.append_text("> 1 GB");
    filter_combo.append_text("> 5 GB");
    filter_combo.append_text("Custom");
    filter_combo.set_active(Some(0));

    let custom_entry = Entry::new();
    custom_entry.set_placeholder_text(Some("Mis. 150 MB (untuk Custom)"));

    let calc_btn = Button::with_label("Hitung");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    row.append(&entry);
    row.append(&choose_btn);
    row.append(&filter_combo);
    row.append(&custom_entry);
    row.append(&calc_btn);
    row.append(&spinner);

    // Info labels
    let info_box = GtkBox::new(Orientation::Horizontal, 12);
    let total_label = Label::new(Some("Total size: -"));
    let count_label = Label::new(Some("Total files: -"));
    info_box.append(&total_label);
    info_box.append(&count_label);

    // Split area: left=extensions, right=filtered files
    let split = GtkBox::new(Orientation::Horizontal, 12);

    // Extension list
    let ext_box = GtkBox::new(Orientation::Vertical, 6);
    ext_box.append(&Label::new(Some("File extensions (by count):")));
    let ext_list = ListBox::new();
    ext_list.set_selection_mode(SelectionMode::None);
    let ext_scroll = ScrolledWindow::new();
    ext_scroll.set_child(Some(&ext_list));
    ext_scroll.set_min_content_width(200);
    ext_scroll.set_min_content_height(300);
    ext_box.append(&ext_scroll);

    // Filtered files list
    let file_box = GtkBox::new(Orientation::Vertical, 6);
    file_box.append(&Label::new(Some("Files passing filter:")));
    let file_list = ListBox::new();
    file_list.set_selection_mode(SelectionMode::None);
    let file_scroll = ScrolledWindow::new();
    file_scroll.set_child(Some(&file_list));
    file_scroll.set_min_content_width(480);
    file_scroll.set_min_content_height(300);
    file_box.append(&file_scroll);

    split.append(&ext_box);
    split.append(&file_box);

    // Add to root
    root.append(&row);
    root.append(&info_box);
    root.append(&split);

    window.set_child(Some(&root));
    window.show();

    // File chooser action
    let entry_clone = entry.clone();
    choose_btn.connect_clicked(move |_| {
        let file_chooser = FileChooserNative::new(
            Some("Pilih folder"),
            None::<&gtk4::Window>,
            FileChooserAction::SelectFolder,
            Some("Pilih"),
            Some("Batal"),
        );
        let entry_inner = entry_clone.clone();
        file_chooser.connect_response(move |dlg, resp| {
            if resp == ResponseType::Accept {
                if let Some(f) = dlg.file() {
                    if let Some(pb) = f.path() {
                        entry_inner.set_text(pb.to_string_lossy().as_ref());
                    }
                }
            }
            dlg.destroy();
        });
        file_chooser.show();
    });

    // Channel untuk menerima hasil scan
    let (tx, rx) = MainContext::channel(PRIORITY_DEFAULT);

    // Receiver: update UI dengan FolderStats
    let total_label_clone = total_label.clone();
    let count_label_clone = count_label.clone();
    let ext_list_clone = ext_list.clone();
    let file_list_clone = file_list.clone();
    let spinner_clone = spinner.clone();

    rx.attach(None, move |res: Result<FolderStats, String>| {
        spinner_clone.stop();
        spinner_clone.set_visible(false);

        match res {
            Ok(stats) => {
                total_label_clone
                    .set_text(&format!("Total size: {}", format_bytes(stats.total_size)));
                count_label_clone
                    .set_text(&format!("Total files: {}", stats.total_files));

                // Bersihkan ext_list & file_list (GTK4 tidak punya .children())
                for child in listbox_children(&ext_list_clone) {
                    ext_list_clone.remove(&child);
                }
                for child in listbox_children(&file_list_clone) {
                    file_list_clone.remove(&child);
                }

                // Tambahkan ekstensi
                for (ext, cnt) in stats.extension_count.into_iter() {
                    let row = ListBoxRow::new();
                    let label = Label::new(Some(&format!("{} : {} file", ext, cnt)));
                    label.set_xalign(0.0);
                    row.set_child(Some(&label));
                    ext_list_clone.append(&row);
                }

                // Tambahkan file yang lolos filter (path + human size)
                // Urutkan descending by size untuk tampilan yang berguna
                let mut files = stats.filtered_files;
                files.sort_by(|a, b| b.1.cmp(&a.1));
                for (path, sz) in files.into_iter() {
                    let row = ListBoxRow::new();
                    let label = Label::new(Some(&format!(
                        "{} ({})",
                        path.to_string_lossy(),
                        format_bytes(sz)
                    )));
                    label.set_xalign(0.0);
                    row.set_child(Some(&label));
                    file_list_clone.append(&row);
                }
            }
            Err(err) => {
                total_label_clone.set_text("Total size: -");
                count_label_clone.set_text(&format!("Error: {}", err));
            }
        }

        Continue(true)
    });

    // Kalkulasi ketika tombol ditekan
    let tx_clone = tx.clone();
    let filter_combo_clone = filter_combo.clone();
    let custom_entry_clone = custom_entry.clone();
    let entry_for_thread = entry.clone();

    calc_btn.connect_clicked(move |_| {
        let text = entry_for_thread.text().to_string();
        if text.trim().is_empty() {
            total_label.set_text("Total size: -");
            count_label.set_text("Total files: - (masukkan path dulu)");
            return;
        }

        let pb = PathBuf::from(text);
        if !pb.exists() || !pb.is_dir() {
            total_label.set_text("Total size: -");
            count_label.set_text("Total files: - (path tidak valid)");
            return;
        }

        // baca pilihan filter
        let active = filter_combo_clone
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "> 100 MB".to_string());
        let custom_text = custom_entry_clone.text().to_string();
        let min_bytes = parse_filter_option(&active, Some(custom_text.as_str()));

        // show spinner
        spinner.start();
        spinner.set_visible(true);
        total_label.set_text("Menghitung...");
        count_label.set_text("Menghitung...");

        // spawn background thread (non-UI), gunakan rayon di dalam scan_folder
        let tx_bg = tx_clone.clone();
        thread::spawn(move || {
            let res = scan_folder(&pb, min_bytes);
            let _ = tx_bg.send(res.map_err(|e| e));
        });
    });
}
