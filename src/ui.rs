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
use crate::scan::{format_bytes, parse_filter_option, FolderStats};

// ================================================================
// ENUM UNTUK OPSI FILTER UKURAN FILE
// ================================================================
#[derive(Debug, Clone, Copy)]
enum OpsiFilterUkuran {
    SeratusMB,
    LimaRatusMB,
    SatuGB,
    LimaGB,
    Custom,
}

impl OpsiFilterUkuran {
    /// Konversi enum ke string untuk ditampilkan di UI
    fn ke_string(&self) -> &'static str {
        match self {
            OpsiFilterUkuran::SeratusMB => "100 MB",
            OpsiFilterUkuran::LimaRatusMB => "500 MB",
            OpsiFilterUkuran::SatuGB => "1 GB",
            OpsiFilterUkuran::LimaGB => "5 GB",
            OpsiFilterUkuran::Custom => "Custom",
        }
    }

    /// Dapatkan semua opsi filter yang tersedia
    fn semua_opsi() -> Vec<OpsiFilterUkuran> {
        vec![
            OpsiFilterUkuran::SeratusMB,
            OpsiFilterUkuran::LimaRatusMB,
            OpsiFilterUkuran::SatuGB,
            OpsiFilterUkuran::LimaGB,
            OpsiFilterUkuran::Custom,
        ]
    }

    /// Parse string menjadi enum OpsiFilterUkuran
    fn dari_string(text: &str) -> Option<OpsiFilterUkuran> {
        OpsiFilterUkuran::semua_opsi()
            .into_iter()
            .find(|opsi| opsi.ke_string() == text)
    }

    /// Cek apakah opsi adalah Custom
    fn adalah_custom(&self) -> bool {
        matches!(self, OpsiFilterUkuran::Custom)
    }

    /// Hitung minimum bytes berdasarkan enum.
    /// Untuk opsi bawaan (100MB, 500MB, dst) langsung dihitung dari enum.
    /// Untuk Custom, nilai diambil dari input user dan diparse oleh parse_filter_option.
    fn ke_minimum_bytes(&self, custom_text: Option<&str>) -> u64 {
        match self {
            OpsiFilterUkuran::SeratusMB => 100 * 1024 * 1024,
            OpsiFilterUkuran::LimaRatusMB => 500 * 1024 * 1024,
            OpsiFilterUkuran::SatuGB => 1 * 1024 * 1024 * 1024,
            OpsiFilterUkuran::LimaGB => 5 * 1024 * 1024 * 1024,
            OpsiFilterUkuran::Custom => {
                let text = custom_text.unwrap_or("").trim();
                // Reuse fungsi parse_filter_option dari crate::scan
                parse_filter_option("Custom", Some(text))
            }
        }
    }
}

// ================================================================
// ENUM UNTUK STATUS VALIDASI PATH
// ================================================================
#[derive(Debug)]
enum StatusValidasiPath {
    Valid(PathBuf),
    Kosong,
    TidakValid,
}

impl StatusValidasiPath {
    /// Validasi path dari string input
    fn validasi(text_path: &str) -> StatusValidasiPath {
        let path_bersih = text_path.trim();

        match path_bersih.is_empty() {
            true => StatusValidasiPath::Kosong,
            false => {
                let path_buffer = PathBuf::from(path_bersih);
                match path_buffer.exists() && path_buffer.is_dir() {
                    true => StatusValidasiPath::Valid(path_buffer),
                    false => StatusValidasiPath::TidakValid,
                }
            }
        }
    }

    /// Ekstrak PathBuf jika valid
    fn ambil_path(self) -> Option<PathBuf> {
        match self {
            StatusValidasiPath::Valid(path) => Some(path),
            _ => None,
        }
    }

    /// Dapatkan pesan error untuk status tidak valid
    fn pesan_error(&self) -> Option<&'static str> {
        match self {
            StatusValidasiPath::Kosong => Some("(masukkan path dulu)"),
            StatusValidasiPath::TidakValid => Some("(path tidak valid)"),
            StatusValidasiPath::Valid(_) => None,
        }
    }
}

// ================================================================
// STRUCT UNTUK UI COMPONENTS
// ================================================================
struct KomponenUI {
    window: ApplicationWindow,
    entry_path: Entry,
    filter_combo: ComboBoxText,
    custom_entry: Entry,
    calc_btn: Button,  // ✅ DITAMBAHKAN
    spinner: Spinner,
    total_label: Label,
    count_label: Label,
    ext_list: ListBox,
    file_list: ListBox,
}

// ================================================================
// HELPER: Ambil semua child dari ListBox
// ================================================================
fn ambil_semua_child_listbox(list_box: &ListBox) -> Vec<Widget> {
    std::iter::successors(list_box.first_child(), |widget| widget.next_sibling()).collect()
}

// ================================================================
// MEMBANGUN UI UTAMA APLIKASI
// ================================================================
pub fn build_ui(app: &Application) {
    let window = buat_window_utama(app);
    let komponen = buat_komponen_ui(&window);

    setup_event_handlers(&komponen);

    komponen.window.show();
}

// ================================================================
// FUNGSI UNTUK MEMBUAT WINDOW UTAMA
// ================================================================
fn buat_window_utama(app: &Application) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);
    window.set_title(Some("fscan - Folder Stats"));
    window.set_default_size(1000, 700);

    terapkan_css_styling();

    window
}

// ================================================================
// FUNGSI UNTUK MENERAPKAN CSS STYLING
// ================================================================
fn terapkan_css_styling() {
    let provider = CssProvider::new();
    provider.load_from_data(
        r#"
        window { background-color: #fafafa; }
        box.card { background-color: #fff; border-radius: 8px; padding: 8px; }
        button.suggested-action { background-color: #f97316; color: #fff; }
        "#,
    );

    gdk::Display::default().map(|display| {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        )
    });
}

// ================================================================
// FUNGSI UNTUK MEMBUAT SEMUA KOMPONEN UI
// ================================================================
fn buat_komponen_ui(window: &ApplicationWindow) -> KomponenUI {
    // Buat header bar dengan tombol fullscreen
    let header = buat_header_bar(window);
    window.set_titlebar(Some(&header));

    // Buat root container
    let root = buat_root_container();

    // Buat komponen control row
    let (entry_path, choose_btn, filter_combo, custom_entry, calc_btn, spinner) =
        buat_control_row();

    // Buat info bar
    let (total_label, count_label) = buat_info_bar();

    // Buat split panel dengan extension list dan file list
    let (ext_list, file_list) = buat_split_panel();

    // Rakit semua komponen ke root
    root.append(&buat_row_horizontal_box(
        vec![
            entry_path.clone().upcast(),
            choose_btn.clone().upcast(),  // ✅ DITAMBAHKAN .clone()
            filter_combo.clone().upcast(),
            custom_entry.clone().upcast(),
            calc_btn.clone().upcast(),
            spinner.clone().upcast(),
        ],
        8,
    ));

    root.append(&buat_row_horizontal_box(
        vec![total_label.clone().upcast(), count_label.clone().upcast()],
        12,
    ));

    root.append(&buat_paned_dengan_lists(&ext_list, &file_list));

    window.set_child(Some(&root));

    // Setup event untuk file chooser
    setup_file_chooser(&choose_btn, &entry_path);

    // Setup event untuk custom entry enable/disable
    setup_custom_entry_toggle(&filter_combo, &custom_entry);

    KomponenUI {
        window: window.clone(),
        entry_path,
        filter_combo,
        custom_entry,
        calc_btn,  // ✅ DITAMBAHKAN
        spinner,
        total_label,
        count_label,
        ext_list,
        file_list,
    }
}

// ================================================================
// FUNGSI UNTUK MEMBUAT HEADER BAR
// ================================================================
fn buat_header_bar(window: &ApplicationWindow) -> HeaderBar {
    let header = HeaderBar::new();
    header.set_show_title_buttons(true);

    let header_title = Label::new(Some("fscan - Folder Stats"));
    header.set_title_widget(Some(&header_title));

    let fullscreen_btn = ToggleButton::with_label("Fullscreen");
    header.pack_end(&fullscreen_btn);

    // Setup toggle fullscreen
    let window_clone = window.clone();
    fullscreen_btn.connect_toggled(move |tombol| {
        match tombol.is_active() {
            true => window_clone.fullscreen(),
            false => window_clone.unfullscreen(),
        }
    });

    header
}

// ================================================================
// FUNGSI UNTUK MEMBUAT ROOT CONTAINER
// ================================================================
fn buat_root_container() -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_vexpand(true);
    root
}

// ================================================================
// FUNGSI UNTUK MEMBUAT CONTROL ROW
// ================================================================
fn buat_control_row() -> (Entry, Button, ComboBoxText, Entry, Button, Spinner) {
    let entry_path = Entry::new();
    entry_path.set_placeholder_text(Some("Masukkan path folder atau pilih..."));
    entry_path.set_hexpand(true);

    let choose_btn = Button::with_label("Pilih Folder");

    let filter_combo = buat_combo_box_filter();

    let custom_entry = Entry::new();
    custom_entry.set_placeholder_text(Some("Mis. 150 MB (untuk Custom)"));
    custom_entry.set_sensitive(false);

    let calc_btn = Button::with_label("Hitung");
    calc_btn.add_css_class("suggested-action");

    let spinner = Spinner::new();
    spinner.set_visible(false);

    (entry_path, choose_btn, filter_combo, custom_entry, calc_btn, spinner)
}

// ================================================================
// FUNGSI UNTUK MEMBUAT COMBO BOX FILTER MENGGUNAKAN ENUM
// ================================================================
fn buat_combo_box_filter() -> ComboBoxText {
    let combo = ComboBoxText::new();

    // Populate combo box menggunakan enum
    OpsiFilterUkuran::semua_opsi()
        .iter()
        .for_each(|opsi| combo.append_text(opsi.ke_string()));

    combo.set_active(Some(0));
    combo
}

// ================================================================
// FUNGSI UNTUK MEMBUAT INFO BAR
// ================================================================
fn buat_info_bar() -> (Label, Label) {
    let total_label = Label::new(Some("Total size: -"));
    let count_label = Label::new(Some("Total files: -"));
    (total_label, count_label)
}

// ================================================================
// FUNGSI UNTUK MEMBUAT HORIZONTAL BOX DENGAN CHILDREN
// ================================================================
fn buat_row_horizontal_box(children: Vec<Widget>, spacing: i32) -> GtkBox {
    let box_horizontal = GtkBox::new(Orientation::Horizontal, spacing);
    children.iter().for_each(|child| box_horizontal.append(child));
    box_horizontal
}

// ================================================================
// FUNGSI UNTUK MEMBUAT SPLIT PANEL
// ================================================================
fn buat_split_panel() -> (ListBox, ListBox) {
    let ext_list = ListBox::new();
    ext_list.set_selection_mode(SelectionMode::None);

    let file_list = ListBox::new();
    file_list.set_selection_mode(SelectionMode::None);

    (ext_list, file_list)
}

// ================================================================
// FUNGSI UNTUK MEMBUAT PANED DENGAN LISTS
// ================================================================
fn buat_paned_dengan_lists(ext_list: &ListBox, file_list: &ListBox) -> Paned {
    let split = Paned::new(Orientation::Horizontal);
    split.set_vexpand(true);

    // Extension box
    let ext_box = buat_list_box_container("File extensions (by count):", ext_list, 260, 380);

    // File list box
    let file_box = buat_list_box_container("Files passing filter:", file_list, 640, 380);

    split.set_start_child(Some(&ext_box));
    split.set_end_child(Some(&file_box));

    split
}

// ================================================================
// FUNGSI HELPER UNTUK MEMBUAT LIST BOX CONTAINER
// ================================================================
fn buat_list_box_container(
    judul: &str,
    list_box: &ListBox,
    min_width: i32,
    min_height: i32,
) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 6);
    container.add_css_class("card");

    let title_label = Label::new(Some(judul));
    container.append(&title_label);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(list_box));
    scroll.set_min_content_width(min_width);
    scroll.set_min_content_height(min_height);

    container.append(&scroll);
    container
}

// ================================================================
// SETUP FILE CHOOSER EVENT
// ================================================================
fn setup_file_chooser(choose_btn: &Button, entry_path: &Entry) {
    let entry_clone = entry_path.clone();

    choose_btn.connect_clicked(move |_| {
        let file_chooser = FileChooserNative::new(
            Some("Pilih folder"),
            None::<&gtk4::Window>,
            FileChooserAction::SelectFolder,
            Some("Pilih"),
            Some("Batal"),
        );

        let entry_inner = entry_clone.clone();

        file_chooser.connect_response(move |dialog, response| {
            handle_file_chooser_response(dialog, response, &entry_inner);
            dialog.destroy();
        });

        file_chooser.show();
    });
}

// ================================================================
// HANDLE FILE CHOOSER RESPONSE
// ================================================================
fn handle_file_chooser_response(
    dialog: &FileChooserNative,
    response: gtk4::ResponseType,
    entry: &Entry,
) {
    (response == gtk4::ResponseType::Accept)
        .then(|| dialog.file())
        .flatten()
        .and_then(|file| file.path())
        .map(|path| entry.set_text(&path.to_string_lossy()));
}

// ================================================================
// SETUP CUSTOM ENTRY TOGGLE
// ================================================================
fn setup_custom_entry_toggle(filter_combo: &ComboBoxText, custom_entry: &Entry) {
    let custom_entry_clone = custom_entry.clone();

    filter_combo.connect_changed(move |combo| {
        let opsi_aktif = combo
            .active_text()
            .and_then(|text| OpsiFilterUkuran::dari_string(&text));

        let is_custom = opsi_aktif
            .map(|opsi| opsi.adalah_custom())
            .unwrap_or(false);

        custom_entry_clone.set_sensitive(is_custom);
    });
}

// ================================================================
// SETUP EVENT HANDLERS UTAMA
// ================================================================
fn setup_event_handlers(komponen: &KomponenUI) {
    let (pengirim_channel, penerima_channel) = mpsc::channel::<Result<FolderStats, String>>();

    // Setup polling untuk menerima hasil dari worker
    setup_result_polling(komponen, penerima_channel);

    // Setup button hitung untuk spawn worker
    setup_button_hitung(komponen, pengirim_channel);
}

// ================================================================
// SETUP POLLING UNTUK MENERIMA HASIL DARI WORKER
// ================================================================
fn setup_result_polling(
    komponen: &KomponenUI,
    penerima_channel: mpsc::Receiver<Result<FolderStats, String>>,
) {
    let total_label = komponen.total_label.clone();
    let count_label = komponen.count_label.clone();
    let ext_list = komponen.ext_list.clone();
    let file_list = komponen.file_list.clone();
    let spinner = komponen.spinner.clone();

    glib::source::timeout_add_local(Duration::from_millis(100), move || {
        handle_channel_message(
            &penerima_channel,
            &spinner,
            &total_label,
            &count_label,
            &ext_list,
            &file_list,
        )
    });
}

// ================================================================
// HANDLE MESSAGE DARI CHANNEL
// ================================================================
fn handle_channel_message(
    penerima: &mpsc::Receiver<Result<FolderStats, String>>,
    spinner: &Spinner,
    total_label: &Label,
    count_label: &Label,
    ext_list: &ListBox,
    file_list: &ListBox,
) -> Continue {
    match penerima.try_recv() {
        Ok(hasil) => {
            hentikan_spinner(spinner);
            tampilkan_hasil(hasil, total_label, count_label, ext_list, file_list);
            Continue(true)
        }
        Err(TryRecvError::Empty) => Continue(true),
        Err(TryRecvError::Disconnected) => {
            hentikan_spinner(spinner);
            count_label.set_text("Error: worker disconnected");
            Continue(false)
        }
    }
}

// ================================================================
// HENTIKAN SPINNER
// ================================================================
fn hentikan_spinner(spinner: &Spinner) {
    spinner.stop();
    spinner.set_visible(false);
}

// ================================================================
// TAMPILKAN HASIL SCAN
// ================================================================
fn tampilkan_hasil(
    hasil: Result<FolderStats, String>,
    total_label: &Label,
    count_label: &Label,
    ext_list: &ListBox,
    file_list: &ListBox,
) {
    match hasil {
        Ok(stats) => tampilkan_stats_berhasil(stats, total_label, count_label, ext_list, file_list),
        Err(pesan_error) => tampilkan_error(pesan_error, total_label, count_label),
    }
}

// ================================================================
// TAMPILKAN STATS JIKA BERHASIL
// ================================================================
fn tampilkan_stats_berhasil(
    stats: FolderStats,
    total_label: &Label,
    count_label: &Label,
    ext_list: &ListBox,
    file_list: &ListBox,
) {
    // Update label
    total_label.set_text(&format!("Total size: {}", format_bytes(stats.total_size)));
    count_label.set_text(&format!("Total files: {}", stats.total_files));

    // Clear dan isi extension list
    bersihkan_list_box(ext_list);
    populate_extension_list(ext_list, stats.extension_count);

    // Clear dan isi file list
    bersihkan_list_box(file_list);
    populate_file_list(file_list, stats.filtered_files);
}

// ================================================================
// TAMPILKAN ERROR
// ================================================================
fn tampilkan_error(pesan_error: String, total_label: &Label, count_label: &Label) {
    total_label.set_text("Total size: -");
    count_label.set_text(&format!("Error: {}", pesan_error));
}

// ================================================================
// BERSIHKAN LIST BOX
// ================================================================
fn bersihkan_list_box(list_box: &ListBox) {
    ambil_semua_child_listbox(list_box)
        .iter()
        .for_each(|child| list_box.remove(child));
}

// ================================================================
// POPULATE EXTENSION LIST
// ================================================================
fn populate_extension_list(ext_list: &ListBox, extension_count: Vec<(String, usize)>) {
    extension_count.into_iter().for_each(|(ekstensi, jumlah)| {
        let row = buat_list_row(&format!("{} : {} file", ekstensi, jumlah));
        ext_list.append(&row);
    });
}

// ================================================================
// POPULATE FILE LIST
// ================================================================
fn populate_file_list(
    file_list: &ListBox,
    filtered_files: Vec<crate::scan::FileEntry>,  // ✅ DIPERBAIKI: Tambahkan <
) {
    let files_terurut = urutkan_files_by_size(filtered_files);

    for file_entry in files_terurut {
        let row = buat_list_row(&format!(
            "{} ({})",
            file_entry.path,
            format_bytes(file_entry.size)
        ));
        file_list.append(&row);
    }
}

// ================================================================
// URUTKAN FILES BERDASARKAN SIZE (DESCENDING)
// ================================================================
fn urutkan_files_by_size(
    mut files: Vec<crate::scan::FileEntry>,  // ✅ DIPERBAIKI: Tambahkan <
) -> Vec<crate::scan::FileEntry> {          // ✅ DIPERBAIKI: Tambahkan <
    files.sort_by(|a, b| b.size.cmp(&a.size));
    files
}

// ================================================================
// BUAT LIST ROW
// ================================================================
fn buat_list_row(text: &str) -> ListBoxRow {
    let row = ListBoxRow::new();
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

// ================================================================
// SETUP BUTTON HITUNG
// ================================================================
fn setup_button_hitung(
    komponen: &KomponenUI,
    pengirim_channel: mpsc::Sender<Result<FolderStats, String>>,
) {
    let entry_path = komponen.entry_path.clone();
    let filter_combo = komponen.filter_combo.clone();
    let custom_entry = komponen.custom_entry.clone();
    let spinner = komponen.spinner.clone();
    let total_label = komponen.total_label.clone();
    let count_label = komponen.count_label.clone();

    // ✅ GUNAKAN calc_btn dari komponen langsung
    komponen.calc_btn.connect_clicked(move |_| {
        handle_button_hitung_click(
            &entry_path,
            &filter_combo,
            &custom_entry,
            &spinner,
            &total_label,
            &count_label,
            &pengirim_channel,
        );
    });
}

// ================================================================
// HANDLE BUTTON HITUNG CLICK
// ================================================================
fn handle_button_hitung_click(
    entry_path: &Entry,
    filter_combo: &ComboBoxText,
    custom_entry: &Entry,
    spinner: &Spinner,
    total_label: &Label,
    count_label: &Label,
    pengirim: &mpsc::Sender<Result<FolderStats, String>>,
) {
    let text_path = entry_path.text().to_string();

    match StatusValidasiPath::validasi(&text_path) {
        StatusValidasiPath::Valid(path_valid) => {
            jalankan_worker_scan(
                path_valid,
                filter_combo,
                custom_entry,
                spinner,
                total_label,
                count_label,
                pengirim,
            );
        }
        status_invalid => {
            tampilkan_pesan_validasi_error(status_invalid, total_label, count_label);
        }
    }
}

// ================================================================
// TAMPILKAN PESAN ERROR VALIDASI
// ================================================================
fn tampilkan_pesan_validasi_error(
    status: StatusValidasiPath,
    total_label: &Label,
    count_label: &Label,
) {
    total_label.set_text("Total size: -");

    status.pesan_error().map(|pesan| {
        count_label.set_text(&format!("Total files: - {}", pesan))
    });
}

// ================================================================
// JALANKAN WORKER SCAN
// ================================================================
fn jalankan_worker_scan(
    path_folder: PathBuf,
    filter_combo: &ComboBoxText,
    custom_entry: &Entry,
    spinner: &Spinner,
    total_label: &Label,
    count_label: &Label,
    pengirim: &mpsc::Sender<Result<FolderStats, String>>,
) {
    let ukuran_minimum = hitung_ukuran_minimum_bytes(filter_combo, custom_entry);

    // Mulai spinner
    spinner.start();
    spinner.set_visible(true);

    // Update label
    total_label.set_text("Menghitung...");
    count_label.set_text("Menghitung...");

    // Spawn worker thread
    spawn_worker_thread(path_folder, ukuran_minimum, pengirim.clone());
}

// ================================================================
// HITUNG UKURAN MINIMUM BYTES (ENUM-BASED)
// ================================================================
fn hitung_ukuran_minimum_bytes(
    filter_combo: &ComboBoxText,
    custom_entry: &Entry,
) -> u64 {
    // Ambil text aktif dari combo; kalau tidak ada, fallback ke "100 MB"
    let text_aktif = filter_combo
        .active_text()
        .unwrap_or_else(|| "100 MB".into());

    // Ubah text menjadi enum; kalau gagal parse, fallback ke SeratusMB
    let opsi = OpsiFilterUkuran::dari_string(&text_aktif)
        .unwrap_or(OpsiFilterUkuran::SeratusMB);

    // Ambil teks custom (kalau user isi)
    let teks_custom = custom_entry.text().to_string();

    // Semua keputusan logika sekarang lewat enum
    opsi.ke_minimum_bytes(Some(teks_custom.as_str()))
}

// ================================================================
// SPAWN WORKER THREAD
// ================================================================
fn spawn_worker_thread(
    path_folder: PathBuf,
    ukuran_minimum: u64,
    pengirim: mpsc::Sender<Result<FolderStats, String>>,
) {
    let path_executable = current_exe().expect("Tidak dapat mengambil path executable");
    let folder_string = path_folder.to_string_lossy().to_string();

    thread::spawn(move || {
        let hasil_scan = ipc::run_worker_scan(&path_executable, &folder_string, ukuran_minimum);
        let _ = pengirim.send(hasil_scan);
    });
}