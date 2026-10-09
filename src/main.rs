//! vettr: agent-first review IDE (pronounced "vetter").

use vettr::cli::{self, Parsed};
use vettr::ui::app::VettrApp;

fn main() -> eframe::Result {
    let cwd = std::env::current_dir().unwrap_or_default();
    let launch = match cli::parse(std::env::args().skip(1), &cwd) {
        Ok(Parsed::Run(launch)) => launch,
        Ok(Parsed::Print(text)) => {
            println!("{text}");
            return Ok(());
        }
        Err(message) => {
            eprintln!("vettr: {message}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    if let Some(wanted) = &launch.profile {
        if let Err(message) = vettr::backend::check_launch_profile(wanted) {
            eprintln!("vettr: {message}");
            std::process::exit(2);
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("vettr")
        .with_app_id("vettr")
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
        Box::new(|cc| Ok(Box::new(VettrApp::with_launch(cc, &launch)))),
    )
}
