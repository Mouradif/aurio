use aurio::{AurioApp, spawn_engine};

fn main() {
    let engine = spawn_engine();

    let icon_image = image::open("assets/icon.png")
        .expect("Failed to open icon path")
        .into_rgba8();

    let (width, height) = icon_image.dimensions();
    let rgba_raw = icon_image.into_raw();

    let icon_data = egui::IconData {
        rgba: rgba_raw,
        width,
        height,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Aurio DAW")
            .with_icon(icon_data),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Aurio",
        options,
        Box::new(|_cc| Ok(Box::new(AurioApp::new(engine)))),
    );
}
