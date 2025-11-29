use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry, Label, Orientation,
    FileChooserAction, FileChooserNative, Spinner, ResponseType,
};
use glib::{MainContext, PRIORITY_DEFAULT};
use std::path::PathBuf;
use std::thread;
use walkdir::WalkDir;
use rayon::prelude::*;
use std::fs;
use humansize::{FileSize, file_size_opts as options};

fn main() {
    // Create GTK application
    let app = Application::new(
        Some("com.example.fscan_simple"),
        Default::default(),
    );

    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    // Window
    let window = ApplicationWindow::new(app);
    window.set_title(Some("fscan - Hitung Total Size Folder"));
    window.set_default_size(600, 120);

    // Vertical box
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    // Horizontal box for entry + choose button
    let hbox = GtkBox::new(Orientation::Horizontal, 6);

    let entry = Entry::new();
    entry.set_placeholder_text(Some("Masukkan path folder atau gunakan tombol 'Pilih Folder'"));

    let choose_btn = Button::with_label("Pilih Folder");
    hbox.append(&entry);
    hbox.append(&choose_btn);

    // Action button + spinner
    let action_box = GtkBox::new(Orientation::Horizontal, 8);
    let calc_btn = Button::with_label("Hitung Total Size");
    let spinner = Spinner::new();
    spinner.set_visible(false);
    action_box.append(&calc_btn);
    action_box.append(&spinner);

    // Result label
    let result_label = Label::new(None);
    result_label.set_wrap(true);
    result_label.set_text("Hasil akan muncul di sini.");

    vbox.append(&hbox);
    vbox.append(&action_box);
    vbox.append(&result_label);

    window.set_child(Some(&vbox));
    window.show();

    // File chooser → isi ke entry
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
                if let Some(path) = dlg.file() {
                    if let Some(pathbuf) = path.path() {
                        entry_inner.set_text(pathbuf.to_string_lossy().as_ref());
                    }
                }
            }
            dlg.destroy();
        });
        file_chooser.show();
    });

    // Setup channel untuk menerima hasil dari thread background
    let (sender, receiver) = MainContext::channel(PRIORITY_DEFAULT);

    // Receiver untuk update UI
    let result_label_clone = result_label.clone();
    let spinner_clone = spinner.clone();
    receiver.attach(None, move |message: Result<u64, String>| {
        spinner_clone.stop();
        spinner_clone.set_visible(false);

        match message {
            Ok(total_bytes) => {
                let human = total_bytes
                    .file_size(options::CONVENTIONAL)
                    .unwrap_or_else(|_| format!("{} B", total_bytes));
                result_label_clone.set_text(&format!(
                    "Total size: {} ({} bytes)",
                    human, total_bytes
                ));
            }
            Err(err) => {
                result_label_clone.set_text(&format!("Error: {}", err));
            }
        }

        glib::Continue(true)
    });

    // Button menghitung
    let sender_clone = sender.clone();
    let entry_for_thread = entry.clone();
    calc_btn.connect_clicked(move |_| {
        let path_text = entry_for_thread.text().to_string();
        if path_text.trim().is_empty() {
            result_label.set_text("Mohon masukkan path folder terlebih dahulu.");
            return;
        }

        let pb = PathBuf::from(path_text);

        if !pb.exists() || !pb.is_dir() {
            result_label.set_text("Path tidak ditemukan atau bukan folder.");
            return;
        }

        spinner.start();
        spinner.set_visible(true);
        result_label.set_text("Menghitung... (proses berjalan di background)");

        let tx = sender_clone.clone();
        thread::spawn(move || {
            match compute_total_size_parallel(&pb) {
                Ok(total) => {
                    let _ = tx.send(Ok(total));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });
    });
}

/// Hitung total size folder menggunakan rayon parallel iterator
fn compute_total_size_parallel(path: &PathBuf) -> Result<u64, String> {
    let walker = WalkDir::new(path).into_iter();

    let mut file_paths: Vec<PathBuf> = Vec::new();
    for entry in walker {
        match entry {
            Ok(e) => {
                if e.file_type().is_file() {
                    file_paths.push(e.into_path());
                }
            }
            Err(err) => {
                eprintln!("Warning: could not read entry: {}", err);
            }
        }
    }

    let total: u64 = file_paths
        .par_iter()
        .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

    Ok(total)
}
