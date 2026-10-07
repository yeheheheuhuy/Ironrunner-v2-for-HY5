use core::slice;

#[derive(Clone, Copy)]
pub struct InputSet {
    current: [u8; 32],
    previous: [u8; 32],
    binds: [u32; 24],
}

impl InputSet {
    pub const fn new(binds: [u32; 24]) -> Self {
        Self {
            current: [0; 32],
            previous: [0; 32],
            binds,
        }
    }

    pub fn apply(&mut self, controller_addr: usize) {
        let game_inputs = unsafe { slice::from_raw_parts_mut(controller_addr as *mut u8, 32) };
        game_inputs.copy_from_slice(&self.current);
        self.previous = self.current;
        self.current = [0; 32];
    }

    pub fn add(&mut self, input: Input) {
        let bind = self.binds[input as usize].trailing_zeros() as usize;
        self.current[bind] = 0xFF;
    }

    pub fn add_with_strength(&mut self, input: Input, strength: u8) {
        let bind = self.binds[input as usize].trailing_zeros() as usize;
        self.current[bind] = strength;
    }

    pub fn add_if_not_held(&mut self, input: Input) -> bool {
        let bind = self.binds[input as usize].trailing_zeros() as usize;
        if self.previous[bind] == 0 {
            self.current[bind] = 0xFF;
            true
        } else {
            false
        }
    }

    pub fn add_if_not_held_with_strength(&mut self, input: Input, strength: u8) -> bool {
        let bind = self.binds[input as usize].trailing_zeros() as usize;
        if self.previous[bind] == 0 {
            self.current[bind] = strength;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Input {
    PadUp,
    PadDown,
    PadLeft,
    PadRight,
    X,
    O,
    R2,
    L2,
    Square,
    Triangle,
    R1,
    L1,
    L3,
    R3,
    Select,
    Start,
    LUp,
    LDown,
    LLeft,
    LRight,
    RUp,
    RDown,
    RLeft,
    RRight,
}
