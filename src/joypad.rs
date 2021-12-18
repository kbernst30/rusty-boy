use crate::utils::*;

#[derive(Debug)]
pub struct Joypad {
    state: [u8; 8]
}

impl Joypad {
    pub fn new() -> Joypad {
        Joypad {
            // Hold internal state of joypad, 1 for unpressed and 0 for pressed
            state: [1; 8]
        }
    }

    pub fn get_button_state(&self, button: usize) -> u8 {
        self.state[button]
    }

    pub fn set_button_state(&mut self, button: usize) {
        self.state[button] = 0;
    }

    pub fn reset_button_state(&mut self, button: usize) {
        self.state[button] = 1;
    }

    pub fn get_buttons_for_mode(&mut self, mode: JoypadMode) -> u8 {
        // Returns the lower nibble for the Joypad register based on the Joypad mode
        match mode {
            JoypadMode::DIRECTION => {
                let down = self.state[DOWN_BUTTON];
                let up = self.state[UP_BUTTON];
                let left = self.state[LEFT_BUTTON];
                let right = self.state[RIGHT_BUTTON];

                (down << 3) | (up << 2) | (left << 1) | right
            },
            JoypadMode::ACTION => {
                let start = self.state[START_BUTTON];
                let select = self.state[SELECT_BUTTON];
                let b = self.state[B_BUTTON];
                let a = self.state[A_BUTTON];

                (start << 3) | (select << 2) | (b << 1) | a
            }
        }
    }
}