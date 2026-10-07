#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    sync::{Arc, atomic::Ordering::Relaxed, mpsc},
    thread,
};

use anyhow::{Result, anyhow};
use eframe::egui;

mod ipc;
mod ui;
use ipc::handle_game_connection;
use ui::Ironrunner;

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let (tx, rx) = mpsc::channel();
    let runner = Ironrunner::new(tx);
    let requirements = Arc::clone(&runner.requirements);
    let status = Arc::clone(&runner.status);
    let connected = Arc::clone(&runner.connected);
    thread::spawn(move || {
        let _ = handle_game_connection(rx, requirements, status, connected.clone());
        connected.store(false, Relaxed);
    });

    let ui_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(egui::viewport::IconData::default())
            .with_inner_size([250.0, 350.0])
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(
        "Ironrunner",
        ui_options,
        Box::new(|ctx| {
            ctx.egui_ctx.set_theme(egui::Theme::Dark);
            Ok(Box::new(runner))
        }),
    )
    .map_err(|e| anyhow!("{e}"))
}
