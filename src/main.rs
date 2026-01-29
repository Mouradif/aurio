use aurio::{AurioApp, spawn_engine};

fn main() {
    let engine = spawn_engine();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Aurio DAW"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Aurio",
        options,
        Box::new(|_cc| Ok(Box::new(AurioApp::new(engine)))),
    );
}
