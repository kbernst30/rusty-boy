use crate::interrupts::*;
use crate::mmu::*;
use crate::utils::*;

pub struct Timer {
    divider_counter: usize,
    timer_counter: usize,
}

impl Timer {

    pub fn new() -> Timer {
        Timer {
            divider_counter: 0,
            timer_counter: 0
        }
    }

    pub fn update(&mut self, mmu: &mut Mmu, cycles: u8) {
        self.update_divider_register(mmu, cycles);

        let freq = self.get_timer_frequency(mmu);

        if mmu.is_timer_frequency_changed() {
            self.timer_counter = 0;
            mmu.update_timer_frequency_changed(false);
        }

        // If Timer is enabled, update it
        if self.is_timer_enabled(mmu) {

            self.timer_counter += cycles as usize;
            while self.timer_counter >= freq {
                // If we have counted enough cycles, increment timer
                self.timer_counter -= freq;
                mmu.increment_timer_register();

                // If the Timer overflows (i.e. rolled around to 0) then
                // Request a Timer interrupt and set the timer to the value
                // in the Timer Modulo register (i.e. 0xFF06)
                if mmu.read_byte(TIMER_ADDR) == 0 {
                    request_interrupt(mmu, Interrupt::TIMER);
                    mmu.write_byte(TIMER_ADDR, mmu.read_byte(TIMER_MODULATOR_ADDR));
                }
            }
        }
    }

    fn is_timer_enabled(&mut self, mmu: &Mmu) -> bool {
        // Bit 2 of Timer Control Register denotes if the Timer is enabled
        is_bit_set(&mmu.read_byte(TIMER_CONTROL_ADDR), 2)
    }

    fn get_timer_frequency(&mut self, mmu: &Mmu) -> usize {
        // Bits 0 and 1 of Timer Control denote the current timer frequency
        let freq_compare_val = mmu.read_byte(TIMER_CONTROL_ADDR) & 0x3;

        // These values are taken from the Pan Docs
        match freq_compare_val {
            0 => CLOCK_SPEED / 4096,
            1 => CLOCK_SPEED / 262144,
            2 => CLOCK_SPEED / 65536,
            _ => CLOCK_SPEED / 16384,
        }
    }

    fn update_divider_register(&mut self, mmu: &mut Mmu, cycles: u8) {
        self.divider_counter += cycles as usize;
        if self.divider_counter >= CYCLES_PER_DIVIDER_INCREMENT {
            self.divider_counter -= CYCLES_PER_DIVIDER_INCREMENT;
            mmu.increment_divider_register();
        }
    }

}