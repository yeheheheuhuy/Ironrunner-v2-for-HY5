use glam::Vec3;
use serde::Serialize;

use crate::address::Addresses;

static IRON_ORE: u16 = 0xD8;
static COLOR_CAPS: [u16; 9] = [259, 962, 1739, 2516, 3330, 4181, 5296, 7047, 8484];

// #[repr(C)]
// #[derive(Clone, Copy)]
// pub struct ItemSet([ItemSetItem; 20]);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Item {
    id: u16,
    amount: u16,
    padding: [u8; 4],
}

pub fn contains_iron(items: &[Item]) -> bool {
    items
        .iter()
        .any(|item| item.id == IRON_ORE && item.amount > 2)
}

pub fn count_iron(items: &[Item]) -> u32 {
    items.iter().fold(0, |state, item| {
        if item.id == IRON_ORE {
            state + item.amount as u32
        } else {
            state
        }
    })
}

// impl ItemSet {
//     pub fn contains_iron(&self) -> bool {
//         self.0
//             .iter()
//             .any(|item| item.id == IRON_ORE && item.amount > 2)
//     }
// }

#[repr(C)]
#[derive(Clone, Copy, Serialize, Debug)]
pub struct Colors([u16; 7]);

impl Colors {
    pub fn capped(&self, gem_level: usize) -> bool {
        let cap = COLOR_CAPS[gem_level];
        self.0.iter().all(|color| *color == cap)
    }

    pub fn get(addresses: &Addresses) -> Self {
        unsafe { (addresses.colors as *const Colors).read() }
    }
}

pub fn in_plaza(addresses: &Addresses) -> bool {
    let player_coords = unsafe { (addresses.player_coordinates() as *const Vec3).read() };
    let current_stage = unsafe { (addresses.current_stage() as *const u16).read() };
    current_stage == 0xC8
        && (8100.0..11650.0).contains(&player_coords.x)
        && (1350.0..5400.0).contains(&player_coords.z)
}

pub fn rainbow_gem_active(addresses: &Addresses) -> bool {
    let gem_color = unsafe { (addresses.gem_color as *const u8).read() };
    gem_color == 9
}

pub fn find_iron_item_set(addresses: &Addresses) -> Option<usize> {
    let sets_addr = addresses.item_sets as *const [Item; 20];
    let sets = unsafe {
        [
            sets_addr.read(),
            sets_addr.wrapping_byte_add(0xF4).read(),
            sets_addr.wrapping_byte_add(0xF4 * 2).read(),
            sets_addr.wrapping_byte_add(0xF4 * 3).read(),
        ]
    };
    sets.iter().position(|set| contains_iron(set))
}

pub fn box_count_iron(addresses: &Addresses) -> u32 {
    let item_box = unsafe { (addresses.item_box as *const [Item; 6300]).read() };
    count_iron(&item_box)
}

pub fn pouch_count_iron(addresses: &Addresses) -> u32 {
    let item_pouch = unsafe { (addresses.item_pouch() as *const [Item; 20]).read() };
    count_iron(&item_pouch)
}

pub fn portable_controls(addresses: &Addresses) -> bool {
    unsafe { (addresses.portable_controls as *const u8).read() == 1 }
}
