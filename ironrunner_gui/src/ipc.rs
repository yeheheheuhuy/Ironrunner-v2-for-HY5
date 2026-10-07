use anyhow::{Result, anyhow};
use arc_swap::ArcSwapOption;
use dll_syringe::{Syringe, process::OwnedProcess};
use serde::{Deserialize, Serialize};
use std::{
    env::current_exe,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::Relaxed},
        mpsc::Receiver,
    },
    time::{Duration, Instant},
};
use tracing::info;

#[repr(C)]
#[derive(Default, Clone, Copy, Deserialize)]
pub struct Colors([u16; 7]);

impl Colors {
    pub fn inner(&self) -> [u16; 7] {
        self.0
    }
}

#[derive(Deserialize, Default, Clone, Copy)]
pub struct Requirements {
    pub capped: bool,
    pub in_plaza: bool,
    pub rainbow: bool,
    pub iron_in_set: bool,
    pub iron_in_box: bool,
    pub portable: bool,
}

#[derive(Deserialize, Default, Clone, Copy)]
pub struct Status {
    pub enabled: bool,
    pub colors: Colors,
    pub gem_level: u8,
    pub gem_color: u8,
    pub completions: u32,
    pub iron_left: u32,
}

#[derive(Deserialize)]
struct Update {
    status: Status,
    requirements: Requirements,
}

#[derive(Serialize)]
pub enum Request {
    Start,
    Settings(Settings),
    Status,
    FullUpdate,
    Stop,
    Exit,
}

#[derive(Clone, Copy, Serialize, Default)]
pub struct Settings {
    pub stop_at_color_cap: bool,
    pub stop_if_not_rainbow: bool,
    pub stop_at_box: bool,
}

fn inject() -> Result<()> {
    let target_process =
        OwnedProcess::find_first_by_name("HY5.exe").ok_or(anyhow!("MHFZ not running"))?;
    info!("Found target process");

    let syringe = Syringe::for_process(target_process);
    let dll_path = if cfg!(debug_assertions) {
        PathBuf::from("./target/i686-pc-windows-msvc/debug/ironrunner.dll")
    } else {
        let mut path = current_exe()?;
        path.pop();
        path.push("ironrunner.dll");
        path
    };
    let _injected = syringe.find_or_inject(dll_path)?;
    info!("Injected successfully");
    Ok(())
}

fn connect() -> Result<TcpStream> {
    let listener = TcpListener::bind("127.0.0.1:6812")?;
    listener.set_nonblocking(true)?;
    let now = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, addr)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_nonblocking(false)?;
                info!("TCP connection accepted: {}", addr);
                return Ok(stream);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if now.elapsed().as_secs_f32() > 5.0 {
                    return Err(anyhow!("No TCP connection from injected dll"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(anyhow!("{e}")),
        }
    }
}

pub fn handle_game_connection(
    ui_rx: Receiver<Request>,
    ui_requirements: Arc<ArcSwapOption<Requirements>>,
    ui_status: Arc<ArcSwapOption<Status>>,
    connected: Arc<AtomicBool>,
) -> Result<()> {
    inject()?;
    let mut stream = connect()?;
    connected.store(true, Relaxed);
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0; 1024];
    while let Ok(request) = ui_rx.recv() {
        postcard::to_io(&request, &mut stream)?;
        match request {
            Request::Status => {
                if let Ok((status, _)) = postcard::from_io::<Status, _>((&mut stream, &mut buf)) {
                    let new_status = Some(Arc::new(status));
                    ui_status.store(new_status);
                } else {
                    break;
                }
            }
            Request::FullUpdate => {
                if let Ok((update, _)) = postcard::from_io::<Update, _>((&mut stream, &mut buf)) {
                    let new_status = Some(Arc::new(update.status));
                    let new_requirements = Some(Arc::new(update.requirements));
                    ui_status.store(new_status);
                    ui_requirements.store(new_requirements);
                } else {
                    break;
                }
            }
            Request::Exit => break,
            _ => (),
        }
    }
    Ok(())
}
