use crate::ipc::{Request, Requirements, Settings, Status, handle_game_connection};
use arc_swap::ArcSwapOption;
use eframe::egui::{self, Color32, Layout};
use humantime::format_duration;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::Relaxed},
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};

static GOOD_REQUIREMENT: Color32 = Color32::from_rgb(178, 239, 155);
static BAD_REQUIREMENT: Color32 = Color32::from_rgb(237, 106, 90);
static BAD_NOT_REQUIRED: Color32 = Color32::from_rgb(245, 183, 0);
static GEM_COLORS: [Color32; 7] = [
    Color32::from_rgb(255, 132, 255),
    Color32::from_rgb(191, 100, 100),
    Color32::from_rgb(255, 255, 80),
    Color32::from_rgb(86, 255, 86),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(64, 64, 255),
    Color32::from_rgb(87, 255, 255),
];

pub struct Ironrunner {
    pub requirements: Arc<ArcSwapOption<Requirements>>,
    pub status: Arc<ArcSwapOption<Status>>,
    pub connected: Arc<AtomicBool>,
    request_sender: Sender<Request>,
    connection_attempted: Option<Instant>,
    last_update: Option<Instant>,
    settings: Settings,
    initial_status: Option<(Status, Instant)>,
}

impl eframe::App for Ironrunner {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ctx.request_repaint_after(Duration::from_millis(500));
            if self.connected.load(Relaxed) {
                let status = self.status.load();
                let requirements = self.requirements.load();
                self.handle_updates(status.as_ref());

                if let Some(status) = status.as_ref()
                    && status.enabled
                {
                    let initial = self.initial_status.unwrap_or_else(|| {
                        let init = (**status, Instant::now());
                        self.initial_status = Some(init);
                        init
                    });
                    show_status(ui, status, &initial);
                } else if let Some(requirements) = requirements.as_ref() {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    show_requirements(ui, requirements);
                }
                ui.with_layout(Layout::bottom_up(egui::Align::Min), |ui| {
                    self.start_stop_button(ui, status.as_ref(), requirements.as_ref());
                    ui.add_space(5.0);
                    self.settings(ui);
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
                self.connect_to_game(ui);
            }
        });
    }
}

impl Ironrunner {
    pub fn new(sender: Sender<Request>) -> Self {
        Self {
            requirements: Arc::new(ArcSwapOption::from_pointee(None)),
            status: Arc::new(ArcSwapOption::from_pointee(None)),
            connected: Arc::new(AtomicBool::default()),
            request_sender: sender,
            connection_attempted: None,
            last_update: None,
            settings: Settings::default(),
            initial_status: None,
        }
    }

    fn connect_to_game(&mut self, ui: &mut egui::Ui) {
        let connection_attempted_too_recently = if let Some(attempt) = self.connection_attempted {
            if attempt.elapsed() > Duration::from_secs(5) {
                self.connection_attempted = None;
                false
            } else {
                true
            }
        } else {
            false
        };
        ui.allocate_ui_with_layout(
            [ui.available_width(), ui.available_height() / 2.0].into(),
            Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(20.0);
                ui.label("Low Grade Edition only");
                ui.add_space(5.0);
                ui.label("If MHFZ was run as admin this program must be run as admin as well.");
                ui.take_available_space();
            },
        );
        ui.with_layout(Layout::bottom_up(egui::Align::Center), |ui| {
            if connection_attempted_too_recently {
                ui.disable();
            }
            if ui
                .add_sized(
                    [ui.available_width(), ui.available_height()],
                    egui::Button::new("Connect to MHFZ"),
                )
                .clicked()
            {
                let (tx, rx) = mpsc::channel();
                self.request_sender = tx;
                let requirements = Arc::clone(&self.requirements);
                let status = Arc::clone(&self.status);
                let connected = Arc::clone(&self.connected);
                thread::spawn(move || {
                    let _ = handle_game_connection(rx, requirements, status, connected.clone());
                    connected.store(false, Relaxed);
                });
                let _ = self.request_sender.send(Request::Settings(self.settings));
                self.connection_attempted = Some(Instant::now());
            }
        });
    }

    fn handle_updates(&mut self, status: Option<&Arc<Status>>) {
        let should_update = if let Some(update) = self.last_update {
            update.elapsed() > Duration::from_millis(200)
        } else {
            true
        };
        if should_update {
            self.last_update = Some(Instant::now());
            if let Some(status) = status
                && status.enabled
            {
                let _ = self.request_sender.send(Request::Status);
            } else {
                let _ = self.request_sender.send(Request::FullUpdate);
            };
        }
    }

    fn start_stop_button(
        &mut self,
        ui: &mut egui::Ui,
        status: Option<&Arc<Status>>,
        requirements: Option<&Arc<Requirements>>,
    ) {
        if let Some(status) = status
            && status.enabled
        {
            if ui
                .add_sized([ui.available_width(), 40.0], egui::Button::new("Stop"))
                .clicked()
            {
                let _ = self.request_sender.send(Request::Stop);
            }
        } else {
            let enabled = if let Some(requirements) = requirements {
                requirements.in_plaza && requirements.portable
            } else {
                false
            };
            ui.add_enabled_ui(enabled, |ui| {
                if ui
                    .add_sized([ui.available_width(), 40.0], egui::Button::new("Start"))
                    .clicked()
                {
                    self.initial_status = None;
                    let _ = self.request_sender.send(Request::Start);
                }
            });
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let checkboxes = [
            ui.checkbox(&mut self.settings.stop_at_box, "Open box after stopping"),
            ui.checkbox(
                &mut self.settings.stop_if_not_rainbow,
                "Stop if not rainbow",
            ),
            ui.checkbox(&mut self.settings.stop_at_color_cap, "Stop if capped"),
        ];
        if checkboxes.iter().any(|resp| resp.clicked()) {
            let _ = self.request_sender.send(Request::Settings(self.settings));
        }
    }
}

fn show_requirements(ui: &mut egui::Ui, requirements: &Requirements) {
    if requirements.capped {
        ui.colored_label(BAD_NOT_REQUIRED, "Colors capped");
    } else {
        ui.colored_label(GOOD_REQUIREMENT, "Colors not capped");
    }
    if requirements.in_plaza {
        ui.colored_label(GOOD_REQUIREMENT, "In mezeporta quest area");
    } else {
        ui.colored_label(BAD_REQUIREMENT, "Outside mezeporta quest area");
    }
    if requirements.rainbow {
        ui.colored_label(GOOD_REQUIREMENT, "Gem color is rainbow");
    } else {
        ui.colored_label(BAD_NOT_REQUIRED, "Gem color is not rainbow");
    }
    if requirements.iron_in_set {
        ui.colored_label(GOOD_REQUIREMENT, "Iron in one of 4 first item sets");
    } else {
        ui.colored_label(BAD_NOT_REQUIRED, "No iron in 4 first item sets");
    }
    if requirements.iron_in_set {
        if requirements.iron_in_box {
            ui.colored_label(GOOD_REQUIREMENT, "Some iron in box");
        } else {
            ui.colored_label(BAD_NOT_REQUIRED, "Not enough iron in box");
        }
    }
    if requirements.portable {
        ui.colored_label(GOOD_REQUIREMENT, "Portable control scheme");
    } else {
        ui.colored_label(BAD_REQUIREMENT, "Classic control scheme");
    }
}

fn show_status(ui: &mut egui::Ui, status: &Status, initial: &(Status, Instant)) {
    static COLOR_CAPS: [u16; 9] = [259, 962, 1739, 2516, 3330, 4181, 5296, 7047, 8484];
    let colors = status.colors.inner();
    let cap = COLOR_CAPS[status.gem_level as usize];
    ui.horizontal(|ui| {
        egui::Grid::new("Colors").show(ui, |ui| {
            ui.colored_label(GEM_COLORS[0], colors[0].to_string());
            ui.colored_label(GEM_COLORS[4], colors[4].to_string());
            ui.end_row();
            ui.colored_label(GEM_COLORS[1], colors[1].to_string());
            ui.colored_label(GEM_COLORS[5], colors[5].to_string());
            ui.end_row();
            ui.colored_label(GEM_COLORS[2], colors[2].to_string());
            ui.colored_label(GEM_COLORS[6], colors[6].to_string());
            ui.end_row();
            ui.colored_label(GEM_COLORS[3], colors[3].to_string());
        });
        let cap_display = format!("Cap: {}", cap);
        ui.label(cap_display);
    });
    ui.separator();
    let elapsed_rounded = Duration::from_secs(initial.1.elapsed().as_secs());
    ui.label(format_duration(elapsed_rounded).to_string());
    let hours_elapsed = elapsed_rounded.as_secs_f64() / 60.0 / 60.0;
    let completions_per_hour = status.completions as f64 / hours_elapsed;
    let completions_display = format!(
        "Completions/h: {:.2} ({} total)",
        completions_per_hour, status.completions
    );
    ui.label(completions_display);
    let color_gained = colors
        .iter()
        .zip(initial.0.colors.inner())
        .map(|(new_color, old_color)| new_color - old_color)
        .max()
        .unwrap();
    let color_per_hour = color_gained as f64 / hours_elapsed;
    let color_display = format!(
        "Single color/h: {:.2} ({} total)",
        color_per_hour, color_gained
    );
    ui.label(color_display);
    let mut iron_display = format!("Iron left: {}", status.iron_left);
    if completions_per_hour > 0.0 {
        let time_to_no_iron = Duration::from_mins(
            ((status.iron_left as f64 / (3.0 * completions_per_hour)) * 60.0) as u64,
        );
        iron_display.push_str(&format!(" ({})", format_duration(time_to_no_iron)));
    }
    ui.label(iron_display);
    if color_per_hour > 0.0 {
        let remaining_color = if status.gem_color == 9 {
            colors.iter().map(|color| cap - *color).max().unwrap()
        } else {
            cap - colors[(status.gem_color - 1) as usize]
        };
        let time_to_cap =
            Duration::from_mins(((remaining_color as f64 / color_per_hour) * 60.0) as u64);
        let time_to_cap_display = format!("To color cap: {}", format_duration(time_to_cap));
        ui.label(time_to_cap_display);
    }
}

impl Drop for Ironrunner {
    fn drop(&mut self) {
        if self.connected.load(Relaxed) {
            let _ = self.request_sender.send(Request::Exit);
        }
    }
}
