//! FaveLists entry point. The data folder defaults to `FaveListsData/` next
//! to this executable (see `store`), so the base dir handed to the app is
//! the executable's directory.

// Windows GUI subsystem: without this, launching the app on Windows opens a
// console window alongside the GUI (and closing that console kills the app).
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    let base_dir = favelists::store::exe_dir();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FaveLists")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "FaveLists",
        options,
        Box::new(|_cc| Ok(Box::new(favelists::app::FaveApp::new(base_dir)))),
    )
}
