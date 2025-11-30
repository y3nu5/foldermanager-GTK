// src/ui.rs
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, CssProvider, Entry,
    FileChooserAction, FileChooserNative, HeaderBar, Label, ListBox, ListBoxRow, Orientation,
    Paned, ScrolledWindow, SelectionMode, Spinner, ToggleButton, Widget,
};

use glib::Continue;
use std::env::current_exe;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Duration;

use crate::ipc;
use crate::scan::{FolderStats, format_bytes, parse_filter_option};

// --------------------------
// Helper: ambil semua child listbox
// --------------------------
fn listbox_children(lb: &ListBox) -> Vec<Widget> {
    let mut out = Vec::new();
    let mut current = lb.first_child();

    while let Some(w) = current {
        out.push(w.clone());
        current = w.next_sibling();
    }
    out
}

// --------------------------
// Membangun UI utama aplikasi
// --------------------------
pub fn build_ui(app: &Application) {
    // ============ WINDOW ===============
    let window = ApplicationWindow::new(app);
    window.set_title(Some("fscan - Folder Stats"));
    window.set_default_size(1000, 700);

    // CSS sederhana
    let provider = CssProvider::new();
    provider.load_from_data(
        r#"
    window { background-color: #fafafa; }
    box.card { background-color: #fff; border-radius: 8px; padding: 8px; }
    button.suggested-action { background-color: #f97316; color: #fff; }
"#,
    );

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // ============ HEADER BAR ============
    let header = HeaderBar::new();
    header.set_show_title_buttons(true);

    let header_title = Label::new(Some("fscan - Folder Stats"));
    header.set_title_widget(Some(&header_title));

    let fullscreen_btn = ToggleButton::with_label("Fullscreen");
    header.pack_end(&fullscreen_btn);

    window.set_titlebar(Some(&header));

    // fullscreen toggle
    let win_clone_fs = window.clone();
    fullscreen_btn.connect_toggled(move |b| {
        if b.is_active() {
            win_clone_fs.fullscreen();
        } else {
            win_clone_fs.unfullscreen();
        }
    });

    // ============ ROOT CONTAINER ============
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_vexpand(true);

    // ============ ROW CONTROL ============
    let row = GtkBox::new(Orientation::Horizontal, 8);

    let entry = Entry::new();
    entry.set_placeholder_text(Some("Masukkan path folder atau pilih..."));
    entry.set_hexpand(true);

    let choose_btn = Button::with_label("Pilih Folder");

    let filter_combo = ComboBoxText::new();
    filter_combo.append_text("100 MB");
    filter_combo.append_text("500 MB");
    filter_combo.append_text("1 GB");
    filter_combo.append_text("5 GB");
    filter_combo.append_text("Custom");
    filter_combo.set_active(Some(0));

    let custom_entry = Entry::new();
    custom_entry.set_placeholder_text(Some("Mis. 150 MB (untuk Custom)"));
    custom_entry.set_sensitive(false);

    let calc_btn = Button::with_label("Hitung");
    calc_btn.add_css_class("suggested-action");

    let spinner = Spinner::new();
    spinner.set_visible(false);

    // Masukkan ke row
    row.append(&entry);
    row.append(&choose_btn);
    row.append(&filter_combo);
    row.append(&custom_entry);
    row.append(&calc_btn);
    row.append(&spinner);

    // ============ INFO BAR ============
    let info_box = GtkBox::new(Orientation::Horizontal, 12);

    let total_label = Label::new(Some("Total size: -"));
    let count_label = Label::new(Some("Total files: -"));

    info_box.append(&total_label);
    info_box.append(&count_label);

    // ============ SPLIT PANEL ============
    let split = Paned::new(Orientation::Horizontal);
    split.set_vexpand(true);

    // ----- Extension Box -----
    let ext_box = GtkBox::new(Orientation::Vertical, 6);
    ext_box.add_css_class("card");

    let ext_title = Label::new(Some("File extensions (by count):"));
    ext_box.append(&ext_title);

    let ext_list = ListBox::new();
    ext_list.set_selection_mode(SelectionMode::None);

    let ext_scroll = ScrolledWindow::new();
    ext_scroll.set_child(Some(&ext_list));
    ext_scroll.set_min_content_width(260);
    ext_scroll.set_min_content_height(380);

    ext_box.append(&ext_scroll);

    // ----- File List Box -----
    let file_box = GtkBox::new(Orientation::Vertical, 6);
    file_box.add_css_class("card");

    let file_title = Label::new(Some("Files passing filter:"));
    file_box.append(&file_title);

    let file_list = ListBox::new();
    file_list.set_selection_mode(SelectionMode::None);

    let file_scroll = ScrolledWindow::new();
    file_scroll.set_child(Some(&file_list));
    file_scroll.set_min_content_width(640);
    file_scroll.set_min_content_height(380);

    file_box.append(&file_scroll);

    // set ke paned
    split.set_start_child(Some(&ext_box));
    split.set_end_child(Some(&file_box));

    // root
    root.append(&row);
    root.append(&info_box);
    root.append(&split);

    window.set_child(Some(&root));
    window.show();

    // ================================================================
    // FILE CHOOSER
    // ================================================================
    let entry_clone = entry.clone();
    choose_btn.connect_clicked(move |_| {
        let fc = FileChooserNative::new(
            Some("Pilih folder"),
            None::<&gtk4::Window>,
            FileChooserAction::SelectFolder,
            Some("Pilih"),
            Some("Batal"),
        );

        let entry_inner = entry_clone.clone();

        fc.connect_response(move |dlg, resp| {
            if resp == gtk4::ResponseType::Accept {
                if let Some(f) = dlg.file() {
                    if let Some(pb) = f.path() {
                        entry_inner.set_text(pb.to_string_lossy().as_ref());
                    }
                }
            }
            dlg.destroy();
        });

        fc.show();
    });

    // ================================================================
    // CUSTOM INPUT ENABLE
    // ================================================================
    let custom_for_combo = custom_entry.clone();
    filter_combo.connect_changed(move |combo| {
        let active = combo
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_default();

        custom_for_combo.set_sensitive(active == "Custom");
    });

    // ================================================================
    // CHANNEL UNTUK RESULT WORKER
    // ================================================================
    let (tx, rx) = mpsc::channel::<Result<FolderStats, String>>();

    // clone untuk polling
    let total_label_clone = total_label.clone();
    let count_label_clone = count_label.clone();
    let ext_list_clone = ext_list.clone();
    let file_list_clone = file_list.clone();
    let spinner_clone = spinner.clone();

    // polling setiap 100ms
    glib::source::timeout_add_local(Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok(res) => {
                spinner_clone.stop();
                spinner_clone.set_visible(false);

                match res {
                    Ok(stats) => {
                        total_label_clone
                            .set_text(&format!("Total size: {}", format_bytes(stats.total_size)));
                        count_label_clone.set_text(&format!("Total files: {}", stats.total_files));

                        // clear ext list
                        for child in listbox_children(&ext_list_clone) {
                            ext_list_clone.remove(&child);
                        }

                        // clear file list
                        for child in listbox_children(&file_list_clone) {
                            file_list_clone.remove(&child);
                        }

                        // isi extension
                        for (ext, cnt) in stats.extension_count.into_iter() {
                            let row = ListBoxRow::new();
                            let label = Label::new(Some(&format!("{} : {} file", ext, cnt)));
                            label.set_xalign(0.0);

                            row.set_child(Some(&label));
                            ext_list_clone.append(&row);
                        }

                        // isi file list
                        let mut files = stats.filtered_files;
                        files.sort_by(|a, b| b.size.cmp(&a.size));

                        for fe in files.into_iter() {
                            let row = ListBoxRow::new();
                            let label = Label::new(Some(&format!(
                                "{} ({})",
                                fe.path,
                                format_bytes(fe.size)
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
            }

            Err(TryRecvError::Empty) => Continue(true),

            Err(TryRecvError::Disconnected) => {
                spinner_clone.stop();
                spinner_clone.set_visible(false);
                count_label_clone.set_text("Error: worker disconnected");
                Continue(false)
            }
        }
    });

    // ================================================================
    // BUTTON HITUNG (SPAWN WORKER PROCESS)

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

        // filter
        let active = filter_combo_clone
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "100 MB".to_string());

        let custom_text = custom_entry_clone.text().to_string();
        let min_bytes = parse_filter_option(&active, Some(custom_text.as_str()));

        // spinner
        spinner_calc.start();
        spinner_calc.set_visible(true);

        total_label_calc.set_text("Menghitung...");
        count_label_calc.set_text("Menghitung...");

        // Spawn worker in background thread (multiprocessing)
        let tx_bg = tx_clone.clone();
        let exe = current_exe().expect("cannot get exe path");
        let folder_arg = pb.to_string_lossy().to_string();

        thread::spawn(move || {
            let res = ipc::run_worker_scan(&exe, &folder_arg, min_bytes);
            let _ = tx_bg.send(res);
        });
    });
}
