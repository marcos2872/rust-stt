//! Binário `app` — janela nativa do pipeline rust-stt.
//!
//! # Uso
//! ```sh
//! cargo run --bin app
//! ```

fn main() {
    // Limpa sessão anterior (caso o app tenha crashado)
    let temp_dir = std::env::temp_dir().join("rust_stt_ui");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("falha ao criar temp dir");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rust-stt")
            .with_inner_size([820.0, 620.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "rust-stt",
        options,
        Box::new(|cc| Ok(Box::new(rust_stt::ui::App::new(cc, temp_dir)))),
    )
    .expect("falha ao iniciar a janela");
}
