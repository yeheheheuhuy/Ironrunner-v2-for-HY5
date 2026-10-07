use std::{
    ffi::c_void,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::Duration,
};

use anyhow::{Result, anyhow};
use mimalloc::MiMalloc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, level_filters::LevelFilter, warn};
use windows::Win32::{
    Foundation::{CloseHandle, HINSTANCE, HMODULE},
    System::{
        Console::{AllocConsole, FreeConsole},
        LibraryLoader::FreeLibraryAndExitThread,
        SystemServices::DLL_PROCESS_ATTACH,
        Threading::{CreateThread, THREAD_CREATION_FLAGS},
    },
};

mod address;
mod gamedata;
mod hooks;
mod input;
mod runner;
use address::{Addresses, MHFOInfo};
use gamedata::{
    Colors, box_count_iron, find_iron_item_set, in_plaza, portable_controls, rainbow_gem_active,
};
use runner::{CURRENT_STATE, ENABLED, SETTINGS, Settings};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Serialize, Debug)]
struct Requirements {
    capped: bool,
    in_plaza: bool,
    rainbow: bool,
    iron_in_set: bool,
    iron_in_box: bool,
    portable: bool,
}

impl Requirements {
    fn get(addresses: &Addresses) -> Self {
        let colors = Colors::get(addresses);
        let gem_level = unsafe { (addresses.gem_level as *const u8).read() };
        Self {
            capped: colors.capped(gem_level as usize),
            in_plaza: in_plaza(addresses),
            rainbow: rainbow_gem_active(addresses),
            iron_in_set: find_iron_item_set(addresses).is_some(),
            iron_in_box: box_count_iron(addresses) > 2,
            portable: portable_controls(addresses),
        }
    }
}

#[derive(Serialize, Debug)]
struct Status {
    enabled: bool,
    colors: Colors,
    gem_level: u8,
    gem_color: u8,
    completions: u32,
    iron_left: u32,
}

impl Status {
    fn get(addresses: &Addresses) -> Self {
        unsafe {
            let state = CURRENT_STATE;
            let (iron_left, completions) = if let Some(state) = state {
                let iron_left = state.box_iron_left + state.pouch_iron_left;
                (iron_left, state.completions)
            } else {
                (0, 0)
            };
            let enabled = ENABLED;
            let gem_level = (addresses.gem_level as *const u8).read();
            let gem_color = (addresses.gem_color as *const u8).read();
            Self {
                enabled,
                colors: Colors::get(addresses),
                gem_level,
                gem_color,
                completions,
                iron_left,
            }
        }
    }
}

#[derive(Serialize)]
struct Update {
    status: Status,
    requirements: Requirements,
}

#[derive(Deserialize)]
enum Request {
    Start,
    Settings(Settings),
    Status,
    FullUpdate,
    Stop,
    Exit,
}

fn handle_requests(mut stream: TcpStream, addresses: &Addresses) {
    let mut buf = [0u8; 1024];
    while let Ok((request, _)) = postcard::from_io::<Request, _>((&mut stream, &mut buf)) {
        match request {
            Request::Start => {
                info!("Starting");
                unsafe {
                    ENABLED = true;
                }
            }
            Request::Settings(settings) => {
                debug!("Updating settings: {:?}", settings);
                unsafe { SETTINGS = settings };
            }
            Request::Status => {
                let status = Status::get(addresses);
                debug!("Sending status: {:?}", status);
                if postcard::to_io(&status, &mut stream).is_err() {
                    break;
                }
            }
            Request::FullUpdate => {
                let status = Status::get(addresses);
                let requirements = Requirements::get(addresses);
                debug!(
                    "Sending status and requirements: {:?} {:?}",
                    status, requirements
                );
                let update = Update {
                    status,
                    requirements,
                };
                if postcard::to_io(&update, &mut stream).is_err() {
                    break;
                }
            }
            Request::Stop => {
                info!("Stopping");
                unsafe { ENABLED = false }
            }
            Request::Exit => {
                info!("Exiting");
                break;
            }
        }
    }
}

fn fallible() -> Result<()> {
    if cfg!(debug_assertions) {
        unsafe { AllocConsole()? };
        info!("Debug console enabled");
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(LevelFilter::DEBUG)
            .init();
    }
    let addresses = MHFOInfo::find_main_dll()
        .ok_or(anyhow!("Couldn't find main game dll"))?
        .addresses();
    info!("Found dll address: {:X?}", addresses.dll);
    let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6812);
    let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(5))?;
    let _hooks = hooks::init(&addresses)?;
    info!("Started successfully. Listening for requests.");
    handle_requests(stream, &addresses);

    // Hooks are disabled when dropped
    Ok(())
}

extern "system" fn main(lp_parameter: *mut c_void) -> u32 {
    if let Err(e) = fallible() {
        warn!("Something went wrong: {e}");
    }
    unsafe {
        if FreeConsole().is_ok() {
            info!("Cleaned up successfully. Exiting");
            FreeLibraryAndExitThread(HMODULE(lp_parameter), 1);
        } else {
            info!("Failed to clean up, leaving dll loaded to avoid crashing the game");
        }
    }
    1
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(hinst: HINSTANCE, fdw_reason: u32, _lpv_reserved: *mut ()) -> bool {
    if fdw_reason == DLL_PROCESS_ATTACH {
        unsafe {
            if let Ok(handle) = CreateThread(
                None,
                0,
                Some(main),
                Some(hinst.0),
                THREAD_CREATION_FLAGS(0),
                None,
            ) {
                let _ = CloseHandle(handle);
            }
        }
    }
    true
}
