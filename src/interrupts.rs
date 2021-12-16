use crate::mmu::*;
use crate::utils::*;

pub const AVAILABLE_INTERRUPTS: [Interrupt; 5] = [
    Interrupt::V_BLANK,
    Interrupt::LCD_STAT,
    Interrupt::TIMER,
    Interrupt::SERIAL,
    Interrupt::JOYPAD
];

pub fn get_servicable_interrupt(mmu: &Mmu) -> Option<Interrupt> {
    for i in 0..AVAILABLE_INTERRUPTS.len() {
        if is_interrupt_enabled(mmu, i) && is_interrupt_requested(mmu, i) {
            return Some(AVAILABLE_INTERRUPTS[i]);
        }
    }

    None
}

fn is_interrupt_enabled(mmu: &Mmu, interrupt_idx: usize) -> bool {
    let interrupts_enabled = mmu.read_byte(INTERRUPT_ENABLE_ADDR);
    is_bit_set(&interrupts_enabled, interrupt_idx)
}

fn is_interrupt_requested(mmu: &Mmu, interrupt_idx: usize) -> bool {
    let interrupts_requested = mmu.read_byte(INTERRUPT_FLAG_ADDR);
    is_bit_set(&interrupts_requested, interrupt_idx)
}
