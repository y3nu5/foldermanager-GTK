use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, CssProvider, Entry,
    EventControllerKey, HeaderBar, Label, ListBox, ListBoxRow, Orientation, Paned, ScrolledWindow,
    SelectionMode, Spinner, StyleContext, ToggleButton, FileChooserAction, FileChooserNative,
    ResponseType, Widget,
};
use gtk4::gdk;

use glib::{ControlFlow, Propagation};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;
use rayon::prelude::*;
use humansize::{FileSize, file_size_opts as options};

/// Struktur data hasil scan (immutable fields)
#[derive(Clone)]
struct FolderStats {
    total_size: u64,
    total_files: usize,
    extension_count: Vec<(String, usize)>, // (ext, count)
    filtered_files: Vec<(PathBuf, u64)>,   // (path, size)
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
    window.set_default_size(1100, 700);

    // ===== CSS: gaya modern sederhana =====
    let provider = CssProvider::new();
    #[allow(deprecated)]
    provider.load_from_data(
        r#"
        window {
            background-color: #f9fafb; /* putih lembut */
        }

        headerbar, .titlebar {
            background: linear-gradient(90deg, #ffffff, #f3f4f6);
            color: #111827;
            border-bottom: 1px solid #e5e7eb;
        }

        box.content {
            background: transparent;
        }

        box.card {
            background-color: #ffffff;
            border-radius: 10px;
            border: 1px solid #e5e7eb;
            padding: 8px 10px;
        }

        box.status-bar {
            margin-top: 4px;
            margin-bottom: 4px;
        }

        box.status-bar label {
            color: #374151;
            font-size: 11px;
        }

        label.title-1 {
            font-weight: 600;
            font-size: 19px;
            color: #111827;
        }

        label.dim-label {
            color: #6b7280;
            font-size: 11px;
        }

        label.heading {
            font-weight: 600;
            font-size: 12px;
            color: #111827;
        }

        button {
            border-radius: 6px;
            padding: 4px 10px;
            background-color: #ffffff;
            color: #111827;
            border: 1px solid #d1d5db;
        }

        button.flat {
            background: transparent;
            border-color: transparent;
            color: #111827;
        }

        /* Oranye sebagai tombol utama */
        button.suggested-action {
            background-color: #f97316;  /* oranye */
            color: #ffffff;
            border-color: #ea580c;
        }

        button.suggested-action:hover {
            background-color: #ea580c;
        }

        entry, combobox, spinbutton {
            border-radius: 6px;
            border: 1px solid #d1d5db;
            background-color: #ffffff;
            color: #111827;
        }

        entry:focus, combobox:focus, spinbutton:focus {
            border-color: #f97316;
            box-shadow: 0 0 0 1px rgba(249, 115, 22, 0.25);
        }

        listbox.rich-list {
            background: transparent;
        }

        listbox.rich-list row {
            padding: 4px 8px;
            background: #ffffff;
            color: #111827;
        }

        listbox.rich-list row:nth-child(even) {
            background: #f9fafb;
        }

        listbox.rich-list row:hover {
            background-color: #fffbeb;  /* putih kekuningan lembut */
        }

        scrolledwindow > viewport > * {
            background-color: transparent;
        }

        /* garis pemisah Paned juga pakai oranye tipis */
        paned > separator {
            min-width: 2px;
            background-color: rgba(249, 115, 22, 0.5);
        }
        "#,
    );


    if let Some(display) = gdk::Display::default() {
        #[allow(deprecated)]
        StyleContext::add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // ===== HeaderBar (titlebar custom + tombol fullscreen) =====
    let header = HeaderBar::new();
    header.set_show_title_buttons(true);

    let header_title = Label::new(Some("fscan - Folder Stats"));
    header_title.add_css_class("title");
    header.set_title_widget(Some(&header_title));

    let fullscreen_btn = ToggleButton::with_label("Fullscreen");
    fullscreen_btn.add_css_class("flat");
    header.pack_end(&fullscreen_btn);

    window.set_titlebar(Some(&header));

    // Toggle fullscreen / windowed
    let win_clone = window.clone();
    fullscreen_btn.connect_toggled(move |btn| {
        if btn.is_active() {
            win_clone.fullscreen();
        } else {
            win_clone.unfullscreen();
        }
    });

    // ESC => keluar dari fullscreen
    let win_for_esc = window.clone();
    let fs_btn_for_esc = fullscreen_btn.clone();
    let key_controller = EventControllerKey::new();

    key_controller.connect_key_pressed(move |_, key, _keycode, _state| {
        if key == gdk::Key::Escape {
            win_for_esc.unfullscreen();
            // sinkronkan toggle button
            if fs_btn_for_esc.is_active() {
                fs_btn_for_esc.set_active(false);
            }
        }
        // lanjutkan event ke handler lain
        Propagation::Proceed
    });

    window.add_controller(key_controller);

    // Root container
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_vexpand(true);
    root.add_css_class("content");

    // ===== Header / Judul di dalam content =====
    let title_label = Label::new(Some("Folder Scanner & Statistics"));
    title_label.add_css_class("title-1"); // pakai style bawaan + CSS
    title_label.set_halign(Align::Start);
    title_label.set_margin_bottom(4);

    let subtitle_label = Label::new(Some(
        "Pilih folder, atur batas ukuran file, lalu klik \"Hitung\" untuk melihat statistik.",
    ));
    subtitle_label.add_css_class("dim-label");
    subtitle_label.set_halign(Align::Start);
    subtitle_label.set_wrap(true);
    subtitle_label.set_margin_bottom(8);

    root.append(&title_label);
    root.append(&subtitle_label);

    // ===== Row: label + entry + choose + filter + calc =====
    let row = GtkBox::new(Orientation::Horizontal, 8);

    let path_label = Label::new(Some("Folder:"));
    path_label.set_halign(Align::Start);
    path_label.set_margin_end(4);

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
    custom_entry.set_width_chars(10);
    custom_entry.set_sensitive(false); // hanya aktif saat "Custom"

    let calc_btn = Button::with_label("Hitung");
    calc_btn.add_css_class("suggested-action");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    row.append(&path_label);
    row.append(&entry);
    row.append(&choose_btn);
    row.append(&filter_combo);
    row.append(&custom_entry);
    row.append(&calc_btn);
    row.append(&spinner);

    // ===== Info labels =====
    let info_box = GtkBox::new(Orientation::Horizontal, 12);
    info_box.add_css_class("status-bar");
    let total_label = Label::new(Some("Total size: -"));
    let count_label = Label::new(Some("Total files: -"));

    total_label.set_halign(Align::Start);
    count_label.set_halign(Align::Start);

    info_box.append(&total_label);
    info_box.append(&count_label);

    // ===== Split area pakai Paned (kiri-kanan bisa di-resize) =====
    let split = Paned::new(Orientation::Horizontal);
    split.set_vexpand(true);
    split.set_hexpand(true);
    split.set_wide_handle(true);

    // Extension list
    let ext_box = GtkBox::new(Orientation::Vertical, 6);
    ext_box.add_css_class("card");
    let ext_title = Label::new(Some("File extensions (by count):"));
    ext_title.add_css_class("heading");
    ext_title.set_halign(Align::Start);
    ext_box.append(&ext_title);

    let ext_list = ListBox::new();
    ext_list.set_selection_mode(SelectionMode::None);
    ext_list.add_css_class("rich-list");

    let ext_scroll = ScrolledWindow::new();
    ext_scroll.set_child(Some(&ext_list));
    ext_scroll.set_min_content_width(220);
    ext_scroll.set_min_content_height(320);
    ext_scroll.set_vexpand(true);
    ext_box.append(&ext_scroll);

    // Filtered files list
    let file_box = GtkBox::new(Orientation::Vertical, 6);
    file_box.add_css_class("card");
    let file_title = Label::new(Some("Files passing filter:"));
    file_title.add_css_class("heading");
    file_title.set_halign(Align::Start);
    file_box.append(&file_title);

    let file_list = ListBox::new();
    file_list.set_selection_mode(SelectionMode::None);
    file_list.add_css_class("rich-list");

    let file_scroll = ScrolledWindow::new();
    file_scroll.set_child(Some(&file_list));
    file_scroll.set_min_content_width(520);
    file_scroll.set_min_content_height(320);
    file_scroll.set_vexpand(true);
    file_box.append(&file_scroll);

    // Paned: kiri = ext_box, kanan = file_box
    split.set_start_child(Some(&ext_box));
    split.set_resize_start_child(true);
    split.set_shrink_start_child(false);

    split.set_end_child(Some(&file_box));
    split.set_resize_end_child(true);
    split.set_shrink_end_child(false);

    // ===== Susun ke root =====
    root.append(&row);
    root.append(&info_box);
    root.append(&split);

    window.set_child(Some(&root));
    window.show();

    // ===== File chooser action =====
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

    // ===== Aktif/nonaktifkan custom_entry sesuai pilihan combo =====
    let custom_entry_for_combo = custom_entry.clone();
    filter_combo.connect_changed(move |combo| {
        let active = combo
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_default();
        custom_entry_for_combo.set_sensitive(active == "Custom");
    });

    // ===== Channel untuk menerima hasil scan (pakai std::mpsc) =====
    let (tx, rx) = mpsc::channel::<Result<FolderStats, String>>();

    let total_label_clone = total_label.clone();
    let count_label_clone = count_label.clone();
    let ext_list_clone = ext_list.clone();
    let file_list_clone = file_list.clone();
    let spinner_clone = spinner.clone();

    // Poll hasil dari thread background tiap 100 ms di main loop
    glib::source::timeout_add_local(Duration::from_millis(100), move || {
    match rx.try_recv() {
        Ok(res) => {
            spinner_clone.stop();
            spinner_clone.set_visible(false);

            match res {
                Ok(stats) => {
                    total_label_clone
                        .set_text(&format!("Total size: {}", format_bytes(stats.total_size)));
                    count_label_clone
                        .set_text(&format!("Total files: {}", stats.total_files));

                    // Bersihkan list
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

                    // Tambahkan file yang lolos filter
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

            // Tetap lanjut polling untuk scan berikutnya
            ControlFlow::Continue
        }
        Err(TryRecvError::Empty) => {
            // Belum ada hasil, lanjut polling
            ControlFlow::Continue
        }
        Err(TryRecvError::Disconnected) => {
            spinner_clone.stop();
            spinner_clone.set_visible(false);
            count_label_clone.set_text("Error: worker thread disconnected");
            ControlFlow::Break
        }
    }
});


    // ===== Kalkulasi ketika tombol ditekan =====
    let tx_clone = tx.clone();
    let filter_combo_clone = filter_combo.clone();
    let custom_entry_clone = custom_entry.clone();
    let entry_for_thread = entry.clone();
    let total_label_calc = total_label.clone();
    let count_label_calc = count_label.clone();
    let spinner_calc = spinner.clone();

    calc_btn.connect_clicked(move |_| {
        let text = entry_for_thread.text().to_string();
        if text.trim().is_empty() {
            total_label_calc.set_text("Total size: -");
            count_label_calc.set_text("Total files: - (masukkan path dulu)");
            return;
        }

        let pb = PathBuf::from(text);
        if !pb.exists() || !pb.is_dir() {
            total_label_calc.set_text("Total size: -");
            count_label_calc.set_text("Total files: - (path tidak valid)");
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
        spinner_calc.start();
        spinner_calc.set_visible(true);
        total_label_calc.set_text("Menghitung...");
        count_label_calc.set_text("Menghitung...");

        // spawn background thread
        let tx_bg = tx_clone.clone();
        thread::spawn(move || {
            let res = scan_folder(&pb, min_bytes);
            let _ = tx_bg.send(res);
        });
    });
}
