//! vettr: agent-first review IDE (pronounced "vetter").

use vettr::ui::app::VettrApp;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("vettr")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([640.0, 400.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!(
        "../branding/icons/vettr-app-icon-512.png"
    )) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "vettr",
        options,
        Box::new(|cc| Ok(Box::new(VettrApp::new(cc)))),
    )
}
