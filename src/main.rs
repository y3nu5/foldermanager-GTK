mod scan;
mod ui;

use gtk4::prelude::*;
use gtk4::Application;

fn main() {
    let app = Application::new(
        Some("com.example.fscan_gui_stats"),
        Default::default(),
    );
    app.connect_activate(ui::build_ui);
    app.run();
}
