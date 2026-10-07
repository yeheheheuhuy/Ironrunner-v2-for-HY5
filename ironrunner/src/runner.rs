use std::fmt::Display;
use std::time::{Duration, Instant};

use crate::address::Addresses;
use crate::gamedata::{
    Colors, Item, box_count_iron, contains_iron, find_iron_item_set, pouch_count_iron,
    rainbow_gem_active,
};
use crate::input::{Input, InputSet};
use anyhow::{Result, anyhow, bail};
use glam::{Vec2, Vec3, Vec3Swizzles};
use serde::Deserialize;
use tracing::{error, info};

pub static mut ENABLED: bool = false;
pub static mut CURRENT_STATE: Option<State> = None;
pub static mut SETTINGS: Settings = Settings::new();

#[derive(Clone, Copy, Deserialize, Debug)]
pub struct Settings {
    stop_at_color_cap: bool,
    stop_if_not_rainbow: bool,
    stop_at_box: bool,
}

impl Settings {
    const fn new() -> Self {
        Self {
            stop_at_color_cap: false,
            stop_if_not_rainbow: false,
            stop_at_box: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct State {
    step: Step,
    substep: usize,
    input: InputSet,
    frame_done: bool,
    item_set: Option<usize>,
    pub box_iron_left: u32,
    pub pouch_iron_left: u32,
    pub completions: u32,
    fallback_timer: Option<Instant>,
}

impl State {
    fn new(addresses: &Addresses) -> Result<Self> {
        let binds = unsafe { (addresses.binds as *const [u32; 24]).read() };
        Ok(Self {
            step: Step::Startup,
            substep: 0,
            input: InputSet::new(binds),
            frame_done: false,
            item_set: find_iron_item_set(addresses),
            box_iron_left: box_count_iron(addresses),
            pouch_iron_left: pouch_count_iron(addresses),
            completions: 0,
            fallback_timer: None,
        })
    }

    fn end_frame(&mut self, addresses: &Addresses) {
        self.input.apply(addresses.controller_values);
        self.frame_done = false;
    }

    fn advance(&mut self, addresses: &Addresses) -> Result<()> {
        while !self.frame_done {
            self.frame_done = true;
            match self.step {
                Step::Startup => startup(addresses, self)?,
                Step::Settings => change_settings(addresses, self)?,
                Step::RemoveQuest => remove_quest(addresses, self)?,
                Step::PreQuest => pre_quest(addresses, self)?,
                Step::AcceptQuest => accept_quest(addresses, self)?,
                Step::Restock => restock(addresses, self)?,
                Step::QuestDepart => quest_depart(addresses, self)?,
                Step::InQuest => in_quest(addresses, self)?,
                Step::Rewards => rewards(addresses, self)?,
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Step {
    Startup,
    Settings,
    RemoveQuest,
    PreQuest,
    AcceptQuest,
    Restock,
    QuestDepart,
    InQuest,
    Rewards,
}

pub fn run(addresses: Addresses) {
    unsafe {
        if ENABLED {
            let mut state = if let Some(state) = CURRENT_STATE {
                state
            } else {
                match State::new(&addresses) {
                    Ok(state) => {
                        addresses.kbm_disable.disable();
                        state
                    }
                    Err(e) => {
                        error!("Error: {e}");
                        ENABLED = false;
                        return;
                    }
                }
            };
            if let Err(e) = state.advance(&addresses) {
                error!("Error: {e}");
                ENABLED = false;
                return;
            }
            state.end_frame(&addresses);
            CURRENT_STATE = Some(state);
        } else {
            let state = CURRENT_STATE;
            if state.is_some() {
                addresses.kbm_disable.restore();
                CURRENT_STATE = None;
            }
        }
    }
}

fn startup(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        match state.substep {
            0 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 0 {
                    state.substep += 1;
                } else {
                    state.input.add_if_not_held(Input::X);
                }
            }
            1 => {
                let accepted_quest = (addresses.accepted_quest as *const u16).read();
                if accepted_quest != 0
                    && accepted_quest != 0x524E
                    && state.input.add_if_not_held(Input::Start)
                {
                    state.substep = 0;
                    state.step = Step::RemoveQuest;
                } else {
                    state.substep += 1;
                    state.frame_done = false;
                }
            }
            2 => {
                let lobby_movement = (addresses.settings_lobby_movement() as *const u8).read();
                let player_interaction =
                    (addresses.settings_player_interaction() as *const u8).read();
                if lobby_movement != 2 && player_interaction == 1 {
                    state.substep += 1;
                    state.frame_done = false;
                } else if state.input.add_if_not_held(Input::Start) {
                    state.substep = 0;
                    state.step = Step::Settings;
                }
            }
            3 => {
                let player_coords = (addresses.player_coordinates() as *const Vec3).read().xz();
                if player_coords.y < 3100.0 {
                    startup_done(addresses, state);
                    return Ok(());
                }
                if !(9890.0..10120.0).contains(&player_coords.x) {
                    let target_y = if player_coords.y > 3900.0 {
                        player_coords.y
                    } else {
                        3000.0
                    };
                    move_towards(
                        addresses,
                        state,
                        Vec2 {
                            x: 10000.0,
                            y: target_y,
                        },
                        true,
                    );
                } else {
                    move_towards(
                        addresses,
                        state,
                        Vec2 {
                            x: 10000.0,
                            y: 3000.0,
                        },
                        true,
                    );
                }
            }
            _ => bail!("Undefined startup substep"),
        }
    }
    Ok(())
}

fn startup_done(addresses: &Addresses, state: &mut State) {
    let accepted_quest = unsafe { (addresses.accepted_quest as *const u16).read() };
    state.substep = 0;
    if accepted_quest == 0x524E {
        state.step = Step::QuestDepart;
    } else {
        state.step = Step::PreQuest;
    }
}

fn change_settings(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        match state.substep {
            0 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 11 {
                    state.substep += 1;
                }
            }
            1 => {
                let start_idx = (addresses.start_menu_idx as *const u8).read();
                if start_idx == 8 {
                    if state.input.add_if_not_held(Input::O) {
                        state.substep += 1;
                    }
                } else {
                    state.input.add_if_not_held(Input::PadUp);
                }
            }
            2 => {
                let settings_idx = (addresses.settings_idx() as *const u8).read();
                if settings_idx == 2 {
                    if state.input.add_if_not_held(Input::O) {
                        state.substep += 1;
                    }
                } else {
                    state.input.add_if_not_held(Input::PadDown);
                }
            }
            3 => {
                let submenu_page = (addresses.settings_submenu_page() as *const u8).read();
                let submenu_idx = (addresses.settings_submenu_idx() as *const u8).read();
                if submenu_page != 1 {
                    state.input.add_if_not_held(Input::R2);
                } else if submenu_idx != 10 {
                    state.input.add_if_not_held(Input::PadUp);
                } else {
                    state.substep += 1;
                }
            }
            4 => {
                let player_interaction =
                    (addresses.settings_player_interaction() as *const u8).read();
                if player_interaction == 1 {
                    if state.input.add_if_not_held(Input::PadUp) {
                        state.substep += 1;
                    }
                } else {
                    state.input.add_if_not_held(Input::PadRight);
                }
            }
            5 => {
                let lobby_movement = (addresses.settings_lobby_movement() as *const u8).read();
                if lobby_movement != 2 {
                    state.substep = 0;
                    state.frame_done = false;
                    state.step = Step::Startup;
                } else {
                    state.input.add_if_not_held(Input::PadRight);
                }
            }
            _ => bail!("Undefined change_settings substep"),
        }
    }
    Ok(())
}

fn remove_quest(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        match state.substep {
            0 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 11 {
                    state.substep += 1;
                }
            }
            1 => {
                let start_idx = (addresses.start_menu_idx as *const u8).read();
                if start_idx == 11 {
                    state.substep += 1;
                    state.frame_done = false;
                } else {
                    state.input.add_if_not_held(Input::PadUp);
                }
            }
            2 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 0 {
                    state.substep = 0;
                    state.frame_done = false;
                    state.step = Step::Startup;
                } else {
                    state.input.add_if_not_held(Input::O);
                }
            }
            _ => bail!("Undefined remove_quest substep"),
        }
    }
    Ok(())
}

fn pre_quest(addresses: &Addresses, state: &mut State) -> Result<()> {
    state.fallback_timer = Some(Instant::now());
    let settings = unsafe { SETTINGS };
    if should_stop(addresses, settings) {
        end(addresses, state, settings.stop_at_box);
        return Ok(());
    }
    let npc_coords = Vec2::new(9444.0, 3635.0);
    move_towards(addresses, state, npc_coords, true);
    if let Some(ptr) = addresses.target() {
        let target_type = unsafe { ((ptr + 0x1B) as *const u8).read() };
        if target_type == 0xA4 && state.input.add_if_not_held(Input::O) {
            state.step = Step::AcceptQuest;
        }
    }
    Ok(())
}

fn should_stop(addresses: &Addresses, settings: Settings) -> bool {
    if settings.stop_at_color_cap {
        let colors = Colors::get(addresses);
        let gem_level = unsafe { (addresses.gem_level as *const u8).read() };
        if colors.capped(gem_level as usize) {
            return true;
        }
    }
    if settings.stop_if_not_rainbow && !rainbow_gem_active(addresses) {
        return true;
    }
    false
}

fn end(addresses: &Addresses, state: &mut State, at_box: bool) {
    if at_box {
        let target = unsafe { (addresses.target_interaction() as *const usize).read() };
        let box_addr = addresses.small_box;
        let box_coords = Vec2::new(10430.0, 2156.0);
        move_towards(addresses, state, box_coords, true);
        if !(target == box_addr && state.input.add_if_not_held(Input::O)) {
            return;
        }
    }
    unsafe {
        CURRENT_STATE = None;
        ENABLED = false;
    }
}

fn accept_quest(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        if handle_fallback(state) {
            return Ok(());
        }
        match state.substep {
            0 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                let dialogue_done = (addresses.npc_dialogue_finished as *const u8).read();
                if interaction_state == 4
                    && dialogue_done == 0
                    && state.input.add_if_not_held(Input::O)
                {
                    state.fallback_timer = Some(Instant::now());
                    state.substep += 1;
                }
            }
            1 => {
                let dialogue_done = (addresses.npc_dialogue_finished as *const u8).read();
                if dialogue_done == 1 && state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            2 => {
                let check = (addresses.menu_state_identifier_1 as *const u8).read();
                if check == 0xFF {
                    state.substep += 1;
                }
            }
            3 => {
                let check = (addresses.menu_state_identifier_1 as *const u8).read();
                if check == 0 {
                    state.fallback_timer = Some(Instant::now());
                    let submenu_check = (addresses.quest_submenu_open as *const u8).read();
                    if submenu_check == 0 {
                        state.substep += 1;
                        state.frame_done = false;
                    } else if state.input.add_if_not_held(Input::X) {
                        state.substep += 1;
                    }
                }
            }
            4 => {
                let previous_quest = (addresses.previous_quest as *const u16).read();
                let quest_list_open = if previous_quest == 0x524E {
                    find_quest_history(addresses, state)
                } else {
                    find_normal_quest_list(addresses, state)
                };
                if quest_list_open {
                    state.substep += 1;
                }
            }
            5 => {
                let check = (addresses.menu_state_identifier_1 as *const u8).read();
                if check == 1 {
                    state.substep += 1;
                    state.frame_done = false;
                }
            }
            6 => {
                let selected_quest = (addresses.selected_quest_id as *const u16).read();
                if selected_quest == 21070 {
                    if state.input.add_if_not_held(Input::O) {
                        state.substep += 1;
                    }
                } else {
                    state.input.add_if_not_held(Input::PadDown);
                }
            }
            7 => {
                let check = (addresses.menu_state_identifier_2 as *const u8).read();
                if check == 0xFF && state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            8 => {
                let check = (addresses.menu_state_identifier_2 as *const u8).read();
                if check == 0 {
                    let player_count = (addresses.quest_player_count_selection as *const u8).read();
                    if player_count != 0 {
                        state.input.add_if_not_held(Input::PadRight);
                    } else {
                        state.substep += 1;
                    }
                }
            }
            9 => {
                let menu_idx = (addresses.quest_accept_menu_idx as *const u8).read();
                if menu_idx != 8 {
                    state.input.add_if_not_held(Input::PadUp);
                } else if state.input.add_if_not_held(Input::O) {
                    state.fallback_timer = Some(Instant::now());
                    state.substep += 1;
                }
            }
            10 => {
                let check = (addresses.menu_state_identifier_2 as *const u8).read();
                if check == 3 && state.input.add_if_not_held(Input::O) {
                    state.fallback_timer = Some(Instant::now());
                    state.substep = 0;
                    state.step = if need_restock(addresses) {
                        Step::Restock
                    } else {
                        Step::QuestDepart
                    };
                }
            }
            _ => bail!("Undefined accept_quest substep"),
        }
    }
    Ok(())
}

fn find_normal_quest_list(addresses: &Addresses, state: &mut State) -> bool {
    unsafe {
        if let Some(idx) = addresses.hunter_quest_idx() {
            let submenu_open = (addresses.quest_submenu_open as *const u8).read();
            if submenu_open == 0 {
                let menu_idx = (addresses.menu_idx as *const u8).read();
                if menu_idx as usize == idx {
                    state.input.add_if_not_held(Input::O);
                } else {
                    state.input.add_if_not_held(Input::PadDown);
                }
            } else if let Some(idx) = addresses.one_star_idx() {
                let submenu_idx = (addresses.sub_menu_idx() as *const u8).read();
                if submenu_idx as usize == idx {
                    return state.input.add_if_not_held(Input::O);
                } else {
                    state.input.add_if_not_held(Input::PadDown);
                }
            }
        }
        false
    }
}

fn find_quest_history(addresses: &Addresses, state: &mut State) -> bool {
    unsafe {
        if let Some(idx) = addresses.favorite_idx() {
            let menu_idx = (addresses.menu_idx as *const u8).read();
            if menu_idx as usize == idx {
                return state.input.add_if_not_held(Input::O);
            } else {
                state.input.add_if_not_held(Input::PadDown);
            }
        }
    }
    false
}

fn need_restock(addresses: &Addresses) -> bool {
    let item_pouch = unsafe { (addresses.item_pouch() as *const [Item; 20]).read() };
    !contains_iron(&item_pouch)
}

fn restock(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        if handle_fallback(state) {
            return Ok(());
        }
        match state.substep {
            0 => {
                let target = (addresses.target_interaction() as *const usize).read();
                let box_addr = addresses.small_box;
                let box_coords = Vec2::new(10430.0, 2156.0);
                move_towards(addresses, state, box_coords, true);
                if target == box_addr && state.input.add_if_not_held(Input::O) {
                    state.fallback_timer = None;
                    state.substep += 1;
                }
            }
            1 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 98 {
                    if pouch_count_iron(addresses) + box_count_iron(addresses) < 3 {
                        return Err(anyhow!("Not enough iron ore left"));
                    } else {
                        state.substep += 1;
                    }
                }
            }
            2 => {
                // This might lose a frame because it's not timed to the earliest possible input but restocks don't happen that often so that's fine

                if need_restock(addresses) {
                    let set_input = if let Some(item_set) = state.item_set {
                        match item_set {
                            0 => Input::L1,
                            1 => Input::L2,
                            2 => Input::R1,
                            3 => Input::R2,
                            _ => bail!("Impossible item set in state"),
                        }
                    } else {
                        return Err(anyhow!("No item set with iron to restock with."));
                    };
                    state.input.add_if_not_held(set_input);
                } else if state.input.add_if_not_held(Input::X) {
                    state.box_iron_left = box_count_iron(addresses);
                    state.pouch_iron_left = pouch_count_iron(addresses);
                    state.substep = 0;
                    state.step = Step::QuestDepart;
                }
            }
            _ => bail!("Undefined restock substep"),
        }
    }
    Ok(())
}

fn quest_depart(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        if handle_fallback(state) {
            return Ok(());
        }
        match state.substep {
            0 => {
                let target = (addresses.target_interaction() as *const usize).read();
                let depart_addr = addresses.departure;
                let depart_coords = Vec2::new(10000.0, 1400.0);
                move_towards(addresses, state, depart_coords, true);
                if target == depart_addr && state.input.add_if_not_held(Input::O) {
                    state.fallback_timer = Some(Instant::now());
                    state.substep += 1;
                }
            }
            1 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 42 {
                    state.fallback_timer = Some(Instant::now());
                    state.substep += 1;
                    // 1 frame delay before input
                }
            }
            2 => {
                if state.input.add_if_not_held(Input::O) {
                    state.fallback_timer = Some(Instant::now());
                    state.substep += 1;
                }
            }
            3 => {
                let interaction_state = (addresses.interaction_state as *const u8).read();
                if interaction_state == 7 {
                    state.fallback_timer = None;
                    state.substep += 1;
                }
            }
            4 => {
                let current_stage = (addresses.current_stage() as *const u16).read();
                if current_stage == 0x2D {
                    state.substep = 0;
                    state.frame_done = false;
                    state.step = Step::InQuest;
                }
            }
            _ => bail!("Undefined quest_depart substep"),
        }
    }
    Ok(())
}

fn in_quest(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        match state.substep {
            0 => {
                let target = (addresses.target_interaction() as *const usize).read();
                let red_box_addr = addresses.red_box;
                let box_coords = Vec2::new(8477.0, 13553.0);
                move_towards(addresses, state, box_coords, false);
                if target == red_box_addr && state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            1 => {
                // Box interaction fails if we let go of movement on the same frame
                let box_coords = Vec2::new(8477.0, 13553.0);
                move_towards(addresses, state, box_coords, false);
                let quest_state = (addresses.in_quest_state as *const u8).read();
                if quest_state == 0 {
                    state.substep += 1;
                    // 1 frame delay before input
                }
            }
            2 => {
                if state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            3 => {
                let box_state = (addresses.in_quest_box_state() as *const u8).read();
                if box_state == 1 && state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            4 => {
                let box_state = (addresses.in_quest_box_state() as *const u8).read();
                if box_state == 2 {
                    let idx = (addresses.in_quest_deposit_idx() as *const u8).read();
                    if idx == 0 {
                        if state.input.add_if_not_held(Input::O) {
                            state.substep = 0;
                            state.completions += 1;
                            state.pouch_iron_left -= 3;
                            state.step = Step::Rewards;
                        }
                    } else {
                        state.input.add_if_not_held(Input::PadUp);
                    }
                }
            }
            _ => bail!("Undefined in_quest substep"),
        }
    }
    Ok(())
}

fn rewards(addresses: &Addresses, state: &mut State) -> Result<()> {
    unsafe {
        match state.substep {
            0 => {
                let rewards_timer = (addresses.rewards_timer() as *const u16).read();
                if rewards_timer == 7200 {
                    state.substep += 1;
                    // 1 frame delay before input
                }
            }
            1 => {
                let idx = (addresses.rewards_idx() as *const u8).read();
                if idx == 3 {
                    state.substep += 1;
                } else {
                    state.input.add_if_not_held(Input::PadUp);
                }
            }
            2 => {
                // Confirming the choice is locked for a few frames here
                let rewards_timer = (addresses.rewards_timer() as *const u16).read();
                if rewards_timer == 7193 && state.input.add_if_not_held(Input::O) {
                    state.substep += 1;
                }
            }
            3 => {
                let check = (addresses.rewards_state as *const u8).read();
                if check == 1 {
                    state.substep += 1;
                    // 1 frame delay before input
                }
            }
            4 => {
                // For some reason both inputs at once works here. Probably works in some other places too?
                state.input.add(Input::PadLeft);
                state.input.add(Input::O);
                state.substep += 1;
            }
            5 => {
                let end_timer = (addresses.quest_end_timer as *const u16).read();
                if end_timer == 3587 {
                    state.input.add(Input::Start);
                    state.substep += 1;
                }
            }
            6 => {
                let current_stage = (addresses.current_stage() as *const u16).read();
                if current_stage == 0xC8 {
                    state.substep = 0;
                    state.step = Step::PreQuest;
                }
            }
            _ => bail!("Undefined rewards substep"),
        }
    }
    Ok(())
}

fn handle_fallback(state: &mut State) -> bool {
    if let Some(timer) = state.fallback_timer
        && timer.elapsed() > Duration::from_secs(30)
        && state.input.add_if_not_held(Input::O)
    // Connection error requires inputting O to clear
    {
        info!(
            "Fallback triggered in step {} at substep {}",
            state.step, state.substep
        );
        state.substep = 0;
        state.step = Step::Startup;
        state.fallback_timer = None;
        true
    } else {
        false
    }
}

fn move_towards(addresses: &Addresses, state: &mut State, dst: Vec2, run: bool) {
    let player_coords = unsafe { (addresses.player_coordinates() as *const Vec3).read().xz() };
    let camera_coords = unsafe { (addresses.camera_coords as *const Vec3).read().xz() };
    let camera_focus = unsafe { ((addresses.camera_coords + 12) as *const Vec3).read().xz() };
    let dst_direction = (dst - player_coords).normalize();
    let camera_direction = (camera_coords - camera_focus).normalize();
    let angle = camera_direction.angle_to(dst_direction).to_degrees();

    let up_strength = 1.0 - (angle.abs().clamp(90.0, 135.0) - 135.0).abs() / 45.0;
    let up = (255.0 * up_strength).round() as u8;

    let down_strength = 1.0 - (angle.abs().clamp(45.0, 90.0) - 45.0) / 45.0;
    let down = (255.0 * down_strength).round() as u8;

    let side_strength = 1.0 - ((angle.abs() - 90.0).abs().clamp(45.0, 90.0) - 45.0) / 45.0;
    let left = if angle > 0.0 {
        (255.0 * side_strength).round() as u8
    } else {
        0
    };
    let right = if angle < 0.0 {
        (255.0 * side_strength).round() as u8
    } else {
        0
    };

    if run {
        state.input.add(Input::R1);
    }
    state.input.add_with_strength(Input::LUp, up);
    state.input.add_with_strength(Input::LDown, down);
    state.input.add_with_strength(Input::LLeft, left);
    state.input.add_with_strength(Input::LRight, right);
}

impl Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Step::Startup => "startup",
            Step::Settings => "settings",
            Step::RemoveQuest => "remove quest",
            Step::PreQuest => "pre quest",
            Step::AcceptQuest => "accept quest",
            Step::Restock => "restock",
            Step::QuestDepart => "quest depart",
            Step::InQuest => "in quest",
            Step::Rewards => "rewards",
        };
        write!(f, "{}", s)
    }
}
