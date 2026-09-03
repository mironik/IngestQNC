mod app;
mod db;
mod media_probe;
mod model;
mod poster_loader;
mod runtime;
mod scanner;
mod source_identity;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("IngestQNC"),
        ..Default::default()
    };

    eframe::run_native(
        "IngestQNC",
        options,
        Box::new(|cc| Ok(Box::new(app::IngestQncApp::new(cc)))),
    )
}
