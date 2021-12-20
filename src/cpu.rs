use std::fs::OpenOptions;
use std::io::prelude::*;

use crate::interrupts::*;
use crate::mmu::*;
use crate::ops::*;
use crate::ppu::*;
use crate::timer::*;
use crate::utils::*;

#[derive(Debug, Copy, Clone)]
struct RegisterPairParts {
    lo: Byte,
    hi: Byte,
}

union RegisterPair {
    val: Word,
    parts: RegisterPairParts,
}

pub struct Cpu {
    // CPU for the Gameboy
    //
    // There are 8 general purpose registers but are often used in pairs. The registers are as follows:
    //     AF	-   A	F	Accumulator & Flags
    //     BC	-   B	C	BC
    //     DE	-   D	E	DE
    //     HL	-   H	L	HL
    //
    // There is a 2-Byte register for the Program counter and a 2-Byte register for the Stack Pointer
    mmu: Mmu,
    timer: Timer,
    ppu: Ppu,
    af: RegisterPair,
    bc: RegisterPair,
    de: RegisterPair,
    hl: RegisterPair,
    program_counter: Word,
    stack_pointer: Word,
    interrupts_enabled: bool,
    will_enable_interrupts: bool,
    will_disable_interrupts: bool,
    halted: bool,
    cycle_tracker: u8,
    last_op: Option<Operation>,
    debug_ctr: usize,
    debug_pc: Word,
    debug_log: bool,
}

impl Cpu {

    pub fn new(mmu: Mmu, timer: Timer, ppu: Ppu) -> Cpu {

        Cpu {
            mmu: mmu,
            timer: timer,
            ppu: ppu,
            af: RegisterPair { val: 0 },
            bc: RegisterPair { val: 0 },
            de: RegisterPair { val: 0 },
            hl: RegisterPair { val: 0 },
            program_counter: 0,
            stack_pointer: 0,
            interrupts_enabled: true,
            will_enable_interrupts: false,
            will_disable_interrupts: false,
            halted: false,
            cycle_tracker: 0,
            last_op: None,
            debug_ctr: 0,
            debug_pc: 0,
            debug_log: false
        }

    }

    pub fn reset(&mut self) {
        self.program_counter = PROGRAM_COUNTER_INIT;
        self.stack_pointer = STACK_POINTER_INIT;

        self.af.val = 0x01B0;
        self.bc.val = 0x0013;
        self.de.val = 0x00D8;
        self.hl.val = 0x014D;

        self.halted = false;
        self.interrupts_enabled = true;
        self.will_disable_interrupts = false;
        self.will_enable_interrupts = false;
    }

    pub fn execute(&mut self) -> u8 {
        // Reset the cycle tracker for mid iteration cycle syncing
        self.cycle_tracker = 0;

        let op = self.read_memory(self.program_counter);
        let opcode = OPCODE_MAP
            .get(&op)
            .expect(&format!("OpCode 0x{:02x} is not recognized", op));

        // if self.program_counter == 0x20a4 {
        //     println!("STUCK");
        // }

        // if self.program_counter == 0x0169 || self.debug_log {
            // self.debug();
            // self.debug_log = true;
            // if self.debug_ctr == 200 {
            //     self.debug_log = false;
            // } else {
            //     self.debug_ctr += 1;
            // }
        // }

        // self.debug_ctr += 1;

        // if (self.debug_ctr >= 3 && self.program_counter == 0x0BF7) || self.debug_log {
        //     self.debug();
        //     self.debug_log = true;

        //     self.debug_ctr += 1;

        //     if self.debug_ctr == 53 {
        //         self.debug_log = false;
        //     }
        // }

        // If in HALT mode, don't execute any instructions and incremeny by 1 T-cycle (4 M-cycles)
        if self.halted {
            self.sync_cycles(4);
            return 4;
        }

        // println!("{:04X} - {}", self.program_counter, self.debug_ctr);

        self.debug_pc = self.program_counter;
        self.program_counter = self.program_counter.wrapping_add(1);

        let cycles = match opcode.operation {
            Operation::ADC => self.do_add(&opcode, true),
            Operation::ADD => self.do_add(&opcode, false),
            Operation::ADD_16_BIT => self.do_add_16_bit(&opcode),
            Operation::AND => self.do_and(&opcode),
            Operation::CALL => self.do_call(&opcode),
            Operation::CCF => self.do_complement_carry(&opcode),
            Operation::CP => self.do_compare(&opcode),
            Operation::CPL => self.do_complement(&opcode),
            Operation::DAA => self.do_daa(&opcode),
            Operation::DEC => self.do_decrement(&opcode),
            Operation::DEC_16_BIT => self.do_decrement_16_bit(&opcode),
            Operation::DI => self.do_disable_interrupts(&opcode),
            Operation::EI => self.do_enable_interrupts(&opcode),
            Operation::HALT => self.do_halt(&opcode),
            Operation::INC => self.do_increment(&opcode),
            Operation::INC_16_BIT => self.do_increment_16_bit(&opcode),
            Operation::JP => self.do_jump(&opcode),
            Operation::JR => self.do_jump_relative(&opcode),
            Operation::LD => self.do_load(&opcode),
            Operation::LDH => self.do_load_h(&opcode),
            Operation::NOP => opcode.cycles,
            Operation::OR => self.do_or(&opcode),
            Operation::POP => self.do_pop(&opcode),
            Operation::PREFIX => self.do_prefix(),
            Operation::PUSH => self.do_push(&opcode),
            Operation::RET => self.do_return(&opcode),
            Operation::RETI => self.do_return(&opcode),
            Operation::RLA => self.do_rla(&opcode),
            Operation::RLCA => self.do_rlca(&opcode),
            Operation::RRA => self.do_rra(&opcode),
            Operation::RRCA => self.do_rrca(&opcode),
            Operation::RST => self.do_restart(&opcode),
            Operation::SBC => self.do_sub(&opcode, true),
            Operation::SCF => self.do_set_carry_flag(&opcode),
            Operation::STOP => opcode.cycles,
            Operation::SUB => self.do_sub(&opcode, false),
            Operation::XOR => self.do_xor(&opcode),
            _ => panic!("Operation not found - {}", opcode.operation)
        };

        // Deal with interrupt enabling/disabling
        self.toggle_interrupts_enabled();
        self.last_op = Some(opcode.operation);

        // Sync remaining cycles for the instruction
        self.sync_cycles(cycles - self.cycle_tracker);

        cycles
    }

    pub fn handle_interrupts(&mut self) {
        let interrupt_option = get_servicable_interrupt(&self.mmu);
        if let Some(interrupt) = interrupt_option {
            self.service_interrupt(interrupt);
        }
    }

    pub fn get_screen(&self) -> &Vec<u8> {
        self.ppu.get_screen()
    }

    pub fn set_button_state(&mut self, button: usize) {
        self.mmu.set_button_state(button);
    }

    pub fn reset_button_state(&mut self, button: usize) {
        self.mmu.reset_button_state(button);
    }

    fn sync_cycles(&mut self, cycles: u8) {
        // Instructions increment other components clock during execution
        // not all at once - this is used to be able to sync components
        // during execution

        self.timer.update(&mut self.mmu, cycles);
        self.ppu.update_graphics(&mut self.mmu, cycles, self.debug_pc == 0x0B7A);

        self.cycle_tracker += cycles;
    }

    fn service_interrupt(&mut self, interrupt: Interrupt) {
        // Unhalt the CPU
        self.halted = false;

        // IF interrupt master switch is enabled, we can go ahead and service
        if self.interrupts_enabled {
            let interrupt_bit = AVAILABLE_INTERRUPTS.iter().position(|&i| i == interrupt).unwrap();

            // Disable any additional interrupts for now
            self.interrupts_enabled = false;

            // Turn off the request for the requested interrupt
            let mut interrupts_requested = self.read_memory(INTERRUPT_FLAG_ADDR);
            reset_bit(&mut interrupts_requested, interrupt_bit);
            self.write_memory(INTERRUPT_FLAG_ADDR, interrupts_requested);

            // Push current PC to the stack
            self.push_word_to_stack(self.program_counter);

            // Service the Interrupt based on value
            // VBlank Interrupt - INT $40
            // LCD Stat Interrupt - INT $48
            // Timer Interrupt - INT $50
            // Serial Interrupt - INT $58
            // Joypad Interrupt - INT $60
            self.program_counter = match interrupt {
                Interrupt::V_BLANK => 0x40,
                Interrupt::LCD_STAT => 0x48,
                Interrupt::TIMER => 0x50,
                Interrupt::SERIAL => 0x58,
                Interrupt::JOYPAD => 0x60,
            };
        }
    }

    fn read_memory(&mut self, addr: Word) -> Byte {
        self.mmu.read_byte(addr)
    }

    fn write_memory(&mut self, addr: Word, data: Byte) {
        // Use for Serial out from Blargg - Debug only
        if addr == 0xFF01 {
            if self.read_memory(0xFF02) == 0x81 {
                print!("{}", data as char);
            }
        }

        self.mmu.write_byte(addr, data);
    }

    fn get_next_byte(&mut self) -> Byte {
        let data = self.read_memory(self.program_counter);
        self.program_counter = self.program_counter.wrapping_add(1);
        data
    }

    fn get_next_byte_signed(&mut self) -> SignedByte {
        let data = self.get_next_byte();
        data as SignedByte
    }

    fn get_next_word(&mut self) -> Word {
        // CPU is little endian so get lo first then hi
        let lo = self.get_next_byte();
        let hi = self.get_next_byte();
        ((hi as Word) << 8) | lo as Word
    }

    fn is_zero_flag_set(&self) -> bool {
        unsafe {
            is_bit_set(&self.af.parts.lo, ZERO_FLAG)
        }
    }

    fn is_carry_flag_set(&self) -> bool {
        unsafe {
            is_bit_set(&self.af.parts.lo, CARRY_FLAG)
        }
    }

    fn is_half_carry_flag_set(&self) -> bool {
        unsafe {
            is_bit_set(&self.af.parts.lo, HALF_CARRY_FLAG)
        }
    }

    fn is_sub_flag_set(&self) -> bool {
        unsafe {
            is_bit_set(&self.af.parts.lo, SUBTRACTION_FLAG)
        }
    }

    fn update_zero_flag(&mut self, val: bool) {
        unsafe {
            match val {
                true => set_bit(&mut self.af.parts.lo, ZERO_FLAG),
                false => reset_bit(&mut self.af.parts.lo, ZERO_FLAG)
            };
        }
    }

    fn update_carry_flag(&mut self, val: bool) {
        unsafe {
            match val {
                true => set_bit(&mut self.af.parts.lo, CARRY_FLAG),
                false => reset_bit(&mut self.af.parts.lo, CARRY_FLAG)
            };
        }
    }

    fn update_half_carry_flag(&mut self, val: bool) {
        unsafe {
            match val {
                true => set_bit(&mut self.af.parts.lo, HALF_CARRY_FLAG),
                false => reset_bit(&mut self.af.parts.lo, HALF_CARRY_FLAG)
            };
        }
    }

    fn update_sub_flag(&mut self, val: bool) {
        unsafe {
            match val {
                true => set_bit(&mut self.af.parts.lo, SUBTRACTION_FLAG),
                false => reset_bit(&mut self.af.parts.lo, SUBTRACTION_FLAG)
            };
        }
    }

    fn push_byte_to_stack(&mut self, data: Byte) {
        self.stack_pointer = self.stack_pointer.wrapping_sub(1);
        self.write_memory(self.stack_pointer, data);
    }

    fn pop_byte_from_stack(&mut self) -> Byte {
        let data = self.read_memory(self.stack_pointer);
        self.stack_pointer = self.stack_pointer.wrapping_add(1);
        data
    }

    fn push_word_to_stack(&mut self, data: Word) {
        let lo = data & 0xFF;
        let hi = data >> 8;
        self.push_byte_to_stack(hi as Byte);
        self.push_byte_to_stack(lo as Byte);
    }

    fn pop_word_from_stack(&mut self) -> Word {
        let lo = self.pop_byte_from_stack();
        let hi = self.pop_byte_from_stack();
        ((hi as Word) << 8) | lo as Word
    }

    fn toggle_interrupts_enabled(&mut self) {
        match self.last_op {
            None => (),
            Some(op) => {
                if op == Operation::DI && self.will_disable_interrupts {
                    self.will_disable_interrupts = false;
                    self.interrupts_enabled = false;
                } else if op == Operation::EI && self.will_enable_interrupts {
                    self.will_enable_interrupts = false;
                    self.interrupts_enabled = true;
                }
            }
        };
    }

    fn do_add(&mut self, opcode: &OpCode, with_carry: bool) -> u8 {
        unsafe {
            let to_add = match opcode.code {
                0x80 => self.bc.parts.hi,
                0x81 => self.bc.parts.lo,
                0x82 => self.de.parts.hi,
                0x83 => self.de.parts.lo,
                0x84 => self.hl.parts.hi,
                0x85 => self.hl.parts.lo,
                0x86 => self.read_memory(self.hl.val),
                0x87 => self.af.parts.hi,
                0x88 => self.bc.parts.hi,
                0x89 => self.bc.parts.lo,
                0x8A => self.de.parts.hi,
                0x8B => self.de.parts.lo,
                0x8C => self.hl.parts.hi,
                0x8D => self.hl.parts.lo,
                0x8E => self.read_memory(self.hl.val),
                0x8F => self.af.parts.hi,
                0xC6 => self.get_next_byte(),
                0xCE => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            let carry = if with_carry && self.is_carry_flag_set() {1} else {0};
            let a_reg = self.af.parts.hi;
            let res = a_reg as usize + to_add as usize + carry;
            let lower_nibble = (a_reg & 0xF) as Word;

            self.update_zero_flag(((res & 0xFF) as Byte) == 0);
            self.update_sub_flag(false);
            self.update_carry_flag(res > 0xFF);
            self.update_half_carry_flag(lower_nibble + ((to_add & 0xF) as Word) + carry as Word > 0xF);

            self.af.parts.hi = (res & 0xFF) as Byte;

            opcode.cycles
        }
    }

    fn do_add_16_bit(&mut self, opcode: &OpCode) -> u8 {
        if opcode.code == 0xE8 {
            // 16 bit arithmetic but it doesn't follow the same flag conventions
            let offset = self.get_next_byte_signed();
            self.update_zero_flag(false);
            self.update_sub_flag(false);

            if offset > 0 {
                self.update_carry_flag((self.stack_pointer & 0xFF) + ((offset as Word) & 0xFF) > 0xFF);
                self.update_half_carry_flag((self.stack_pointer & 0xF) + ((offset as Word) & 0xF) > 0xF);
                self.stack_pointer = self.stack_pointer.wrapping_add(offset as Word);
            } else {
                self.update_carry_flag((self.stack_pointer & 0xFF) + ((offset as Word) & 0xFF) > 0xFF);
                self.update_half_carry_flag((self.stack_pointer & 0xF) + ((offset as Word) & 0xF) > 0xF);
                self.stack_pointer = self.stack_pointer.wrapping_sub(offset.abs() as Word);
            }
        } else {
            unsafe {
                let to_add = match opcode.code {
                    0x09 => self.bc.val,
                    0x19 => self.de.val,
                    0x29 => self.hl.val,
                    0x39 => self.stack_pointer,
                    _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
                };

                self.update_sub_flag(false);
                self.update_carry_flag((self.hl.val as usize) + (to_add as usize) > 0xFFFF);
                self.update_half_carry_flag((self.hl.val & 0xFFF) + (to_add & 0xFFF) & 0x1000 > 0);
                self.hl.val = self.hl.val.wrapping_add(to_add);
            }
        }

        opcode.cycles
    }

    fn do_and(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let to_and = match opcode.code {
                0xA0 => self.bc.parts.hi,
                0xA1 => self.bc.parts.lo,
                0xA2 => self.de.parts.hi,
                0xA3 => self.de.parts.lo,
                0xA4 => self.hl.parts.hi,
                0xA5 => self.hl.parts.lo,
                0xA6 => self.read_memory(self.hl.val),
                0xA7 => self.af.parts.hi,
                0xE6 => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.af.parts.hi &= to_and;

            self.update_zero_flag(self.af.parts.hi == 0);
            self.update_half_carry_flag(true);
            self.update_sub_flag(false);
            self.update_carry_flag(false);

            opcode.cycles
        }
    }

    fn do_bit(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0x40 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 0)),
                0x41 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 0)),
                0x42 => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 0)),
                0x43 => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 0)),
                0x44 => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 0)),
                0x45 => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 0)),
                0x46 => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 0));
                },
                0x47 => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 0)),
                0x48 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 1)),
                0x49 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 1)),
                0x4A => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 1)),
                0x4B => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 1)),
                0x4C => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 1)),
                0x4D => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 1)),
                0x4E => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 1));
                },
                0x4F => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 1)),
                0x50 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 2)),
                0x51 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 2)),
                0x52 => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 2)),
                0x53 => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 2)),
                0x54 => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 2)),
                0x55 => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 2)),
                0x56 => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 2));
                },
                0x57 => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 2)),
                0x58 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 3)),
                0x59 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 3)),
                0x5A => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 3)),
                0x5B => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 3)),
                0x5C => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 3)),
                0x5D => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 3)),
                0x5E => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 3));
                },
                0x5F => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 3)),
                0x60 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 4)),
                0x61 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 4)),
                0x62 => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 4)),
                0x63 => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 4)),
                0x64 => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 4)),
                0x65 => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 4)),
                0x66 => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 4));
                },
                0x67 => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 4)),
                0x68 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 5)),
                0x69 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 5)),
                0x6A => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 5)),
                0x6B => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 5)),
                0x6C => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 5)),
                0x6D => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 5)),
                0x6E => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 5));
                },
                0x6F => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 5)),
                0x70 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 6)),
                0x71 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 6)),
                0x72 => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 6)),
                0x73 => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 6)),
                0x74 => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 6)),
                0x75 => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 6)),
                0x76 => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 6));
                },
                0x77 => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 6)),
                0x78 => self.update_zero_flag(!is_bit_set(&self.bc.parts.hi, 7)),
                0x79 => self.update_zero_flag(!is_bit_set(&self.bc.parts.lo, 7)),
                0x7A => self.update_zero_flag(!is_bit_set(&self.de.parts.hi, 7)),
                0x7B => self.update_zero_flag(!is_bit_set(&self.de.parts.lo, 7)),
                0x7C => self.update_zero_flag(!is_bit_set(&self.hl.parts.hi, 7)),
                0x7D => self.update_zero_flag(!is_bit_set(&self.hl.parts.lo, 7)),
                0x7E => {
                    self.sync_cycles(4);
                    let val = self.read_memory(self.hl.val);
                    self.update_zero_flag(!is_bit_set(&val, 7));
                },
                0x7F => self.update_zero_flag(!is_bit_set(&self.af.parts.hi, 7)),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_half_carry_flag(true);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_call(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0xC4 => {
                    if !self.is_zero_flag_set() {
                        let addr = self.get_next_word();
                        self.push_word_to_stack(self.program_counter);
                        self.program_counter = addr;
                        opcode.cycles
                    } else {
                        self.program_counter = self.program_counter.wrapping_add(2);
                        opcode.alt_cycles.unwrap_or(opcode.cycles)
                    }
                },
                0xCC => {
                    if self.is_zero_flag_set() {
                        let addr = self.get_next_word();
                        self.push_word_to_stack(self.program_counter);
                        self.program_counter = addr;
                        opcode.cycles
                    } else {
                        self.program_counter = self.program_counter.wrapping_add(2);
                        opcode.alt_cycles.unwrap_or(opcode.cycles)
                    }
                },
                0xCD => {
                    let addr = self.get_next_word();
                    self.push_word_to_stack(self.program_counter);
                    self.program_counter = addr;
                    opcode.cycles
                },
                0xD4 => {
                    if !self.is_carry_flag_set() {
                        let addr = self.get_next_word();
                        self.push_word_to_stack(self.program_counter);
                        self.program_counter = addr;
                        opcode.cycles
                    } else {
                        self.program_counter = self.program_counter.wrapping_add(2);
                        opcode.alt_cycles.unwrap_or(opcode.cycles)
                    }
                },
                0xDC => {
                    if self.is_carry_flag_set() {
                        let addr = self.get_next_word();
                        self.push_word_to_stack(self.program_counter);
                        self.program_counter = addr;
                        opcode.cycles
                    } else {
                        self.program_counter = self.program_counter.wrapping_add(2);
                        opcode.alt_cycles.unwrap_or(opcode.cycles)
                    }
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            }
        }
    }

    fn do_compare(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let to_cp = match opcode.code {
                0xB8 => self.bc.parts.hi,
                0xB9 => self.bc.parts.lo,
                0xBA => self.de.parts.hi,
                0xBB => self.de.parts.lo,
                0xBC => self.hl.parts.hi,
                0xBD => self.hl.parts.lo,
                0xBE => self.read_memory(self.hl.val),
                0xBF => self.af.parts.hi,
                0xFE => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(self.af.parts.hi == to_cp);
            self.update_sub_flag(true);
            self.update_carry_flag(self.af.parts.hi < to_cp);
            self.update_half_carry_flag(((self.af.parts.hi as SignedWord) & 0xF) - ((to_cp as SignedWord) & 0xF) < 0);

            opcode.cycles
        }
    }

    fn do_complement(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            self.af.parts.hi = !self.af.parts.hi;

            self.update_half_carry_flag(true);
            self.update_sub_flag(true);

            opcode.cycles
        }
    }

    fn do_complement_carry(&mut self, opcode: &OpCode) -> u8 {
        self.update_carry_flag(!self.is_carry_flag_set());
        self.update_half_carry_flag(false);
        self.update_sub_flag(false);

        opcode.cycles
    }

    fn do_daa(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let mut val = self.af.parts.hi;
            let mut should_set_carry = false;

            if !self.is_sub_flag_set() {
                if self.is_carry_flag_set() || val > 0x99 {
                    val = val.wrapping_add(0x60);
                    should_set_carry = true;
                }

                if self.is_half_carry_flag_set() || (val & 0x0F) > 0x09 {
                    val = val.wrapping_add(0x6);
                }

            } else {
                if self.is_carry_flag_set() {
                    val = val.wrapping_sub(0x60);
                    should_set_carry = true;
                }

                if self.is_half_carry_flag_set() {
                    val = val.wrapping_sub(0x6);
                }
            }

            self.update_zero_flag(val == 0);
            self.update_half_carry_flag(false);
            self.update_carry_flag(should_set_carry);

            self.af.parts.hi = val;

            opcode.cycles
        }
    }

    fn do_decrement(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let result = match opcode.code {
                0x05 => {
                    self.bc.parts.hi = self.bc.parts.hi.wrapping_sub(1);
                    self.bc.parts.hi
                },
                0x0D => {
                    self.bc.parts.lo = self.bc.parts.lo.wrapping_sub(1);
                    self.bc.parts.lo
                },
                0x15 => {
                    self.de.parts.hi = self.de.parts.hi.wrapping_sub(1);
                    self.de.parts.hi
                },
                0x1D => {
                    self.de.parts.lo = self.de.parts.lo.wrapping_sub(1);
                    self.de.parts.lo
                },
                0x25 => {
                    self.hl.parts.hi = self.hl.parts.hi.wrapping_sub(1);
                    self.hl.parts.hi
                },
                0x2D => {
                    self.hl.parts.lo = self.hl.parts.lo.wrapping_sub(1);
                    self.hl.parts.lo
                },
                0x35 => {
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    val = val.wrapping_sub(1);
                    self.write_memory(self.hl.val, val);
                    val
                },
                0x3D => {
                    self.af.parts.hi = self.af.parts.hi.wrapping_sub(1);
                    self.af.parts.hi
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(result == 0);
            self.update_sub_flag(true);
            self.update_half_carry_flag(result & 0xF == 0xF);

            opcode.cycles
        }
    }

    fn do_decrement_16_bit(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0x0B => self.bc.val = self.bc.val.wrapping_sub(1),
                0x1B => self.de.val = self.de.val.wrapping_sub(1),
                0x2B => self.hl.val = self.hl.val.wrapping_sub(1),
                0x3B => self.stack_pointer = self.stack_pointer.wrapping_sub(1),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_disable_interrupts(&mut self, opcode: &OpCode) -> u8 {
        self.will_disable_interrupts = true;
        opcode.cycles
    }

    fn do_enable_interrupts(&mut self, opcode: &OpCode) -> u8 {
        self.will_enable_interrupts = true;
        opcode.cycles
    }

    fn do_halt(&mut self, opcode: &OpCode) -> u8 {
        self.halted = true;
        opcode.cycles
    }

    fn do_increment(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let result = match opcode.code {
                0x04 => {
                    self.bc.parts.hi = self.bc.parts.hi.wrapping_add(1);
                    self.bc.parts.hi
                },
                0x0C => {
                    self.bc.parts.lo = self.bc.parts.lo.wrapping_add(1);
                    self.bc.parts.lo
                },
                0x14 => {
                    self.de.parts.hi = self.de.parts.hi.wrapping_add(1);
                    self.de.parts.hi
                },
                0x1C => {
                    self.de.parts.lo = self.de.parts.lo.wrapping_add(1);
                    self.de.parts.lo
                },
                0x24 => {
                    self.hl.parts.hi = self.hl.parts.hi.wrapping_add(1);
                    self.hl.parts.hi
                },
                0x2C => {
                    self.hl.parts.lo = self.hl.parts.lo.wrapping_add(1);
                    self.hl.parts.lo
                },
                0x34 => {
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    val = val.wrapping_add(1);
                    self.write_memory(self.hl.val, val);
                    val
                },
                0x3C => {
                    self.af.parts.hi = self.af.parts.hi.wrapping_add(1);
                    self.af.parts.hi
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(result == 0);
            self.update_sub_flag(false);
            self.update_half_carry_flag(result & 0xF == 0);

            opcode.cycles
        }
    }

    fn do_increment_16_bit(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0x03 => self.bc.val = self.bc.val.wrapping_add(1),
                0x13 => self.de.val = self.de.val.wrapping_add(1),
                0x23 => self.hl.val = self.hl.val.wrapping_add(1),
                0x33 => self.stack_pointer = self.stack_pointer.wrapping_add(1),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_jump(&mut self, opcode: &OpCode) -> u8 {
        match opcode.code {
            0xC2 => {
                self.program_counter = if !self.is_zero_flag_set() { self.get_next_word() } else { self.program_counter.wrapping_add(2) };
                if self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xC3 => {
                self.program_counter = self.get_next_word();
                opcode.cycles
            },
            0xCA => {
                self.program_counter = if self.is_zero_flag_set() { self.get_next_word() } else { self.program_counter.wrapping_add(2) };
                if !self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xD2 => {
                self.program_counter = if !self.is_carry_flag_set() { self.get_next_word() } else { self.program_counter.wrapping_add(2) };
                if self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xDA => {
                self.program_counter = if self.is_carry_flag_set() {self.get_next_word()} else { self.program_counter.wrapping_add(2) };
                if !self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xE9 => {
                unsafe {
                    self.program_counter = self.hl.val;
                    opcode.cycles
                }
            },
            _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
        }
    }

    fn do_jump_relative(&mut self, opcode: &OpCode) -> u8 {
        match opcode.code {
            0x18 => {
                let offset = self.get_next_byte_signed();
                if offset > 0 {
                    self.program_counter += offset as Word;
                } else {
                    self.program_counter -= offset.abs() as Word;
                }

                opcode.cycles
            },
            0x20 => {
                let offset = self.get_next_byte_signed();
                if !self.is_zero_flag_set() {
                    if offset > 0 {
                        self.program_counter += offset as Word;
                    } else {
                        self.program_counter -= offset.abs() as Word;
                    }
                }

                if self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0x28 => {
                let offset = self.get_next_byte_signed();
                if self.is_zero_flag_set() {
                    if offset > 0 {
                        self.program_counter += offset as Word;
                    } else {
                        self.program_counter -= offset.abs() as Word;
                    }
                }

                if !self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0x30 => {
                let offset = self.get_next_byte_signed();
                if !self.is_carry_flag_set() {
                    if offset > 0 {
                        self.program_counter += offset as Word;
                    } else {
                        self.program_counter -= offset.abs() as Word;
                    }
                }

                if self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0x38 => {
                let offset = self.get_next_byte_signed();
                if self.is_carry_flag_set() {
                    if offset > 0 {
                        self.program_counter += offset as Word;
                    } else {
                        self.program_counter -= offset.abs() as Word;
                    }
                }

                if !self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
        }
    }

    fn do_load(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0x01 => self.bc.val = self.get_next_word(),
                0x02 => self.write_memory(self.bc.val, self.af.parts.hi),
                0x06 => self.bc.parts.hi = self.get_next_byte(),
                0x08 => {
                    let addr = self.get_next_word();
                    self.write_memory(addr, (self.stack_pointer & 0xFF) as Byte);
                    self.write_memory(addr + 1, (self.stack_pointer >> 8) as Byte);
                },
                0x0A => self.af.parts.hi = self.read_memory(self.bc.val),
                0x0E => self.bc.parts.lo = self.get_next_byte(),
                0x11 => self.de.val = self.get_next_word(),
                0x12 => self.write_memory(self.de.val, self.af.parts.hi),
                0x16 => self.de.parts.hi = self.get_next_byte(),
                0x1A => self.af.parts.hi = self.read_memory(self.de.val),
                0x1E => self.de.parts.lo = self.get_next_byte(),
                0x21 => self.hl.val = self.get_next_word(),
                0x22 => {
                    self.write_memory(self.hl.val, self.af.parts.hi);
                    self.hl.val = self.hl.val.wrapping_add(1);
                },
                0x26 => self.hl.parts.hi = self.get_next_byte(),
                0x2A => {
                    self.af.parts.hi = self.read_memory(self.hl.val);
                    self.hl.val = self.hl.val.wrapping_add(1);;
                }
                0x2E => self.hl.parts.lo = self.get_next_byte(),
                0x31 => self.stack_pointer = self.get_next_word(),
                0x32 => {
                    self.write_memory(self.hl.val, self.af.parts.hi);
                    self.hl.val = self.hl.val.wrapping_sub(1);
                },
                0x36 => {
                    self.sync_cycles(4);
                    let val = self.get_next_byte();
                    self.write_memory(self.hl.val, val);
                },
                0x3A => {
                    self.af.parts.hi = self.read_memory(self.hl.val);
                    self.hl.val = self.hl.val.wrapping_sub(1);
                },
                0x40 => self.bc.parts.hi = self.bc.parts.hi,
                0x41 => self.bc.parts.hi = self.bc.parts.lo,
                0x42 => self.bc.parts.hi = self.de.parts.hi,
                0x43 => self.bc.parts.hi = self.de.parts.lo,
                0x44 => self.bc.parts.hi = self.hl.parts.hi,
                0x45 => self.bc.parts.hi = self.hl.parts.lo,
                0x46 => self.bc.parts.hi = self.read_memory(self.hl.val),
                0x47 => self.bc.parts.hi = self.af.parts.hi,
                0x48 => self.bc.parts.lo = self.bc.parts.hi,
                0x49 => self.bc.parts.lo = self.bc.parts.lo,
                0x4A => self.bc.parts.lo = self.de.parts.hi,
                0x4B => self.bc.parts.lo = self.de.parts.lo,
                0x4C => self.bc.parts.lo = self.hl.parts.hi,
                0x4D => self.bc.parts.lo = self.hl.parts.lo,
                0x4E => self.bc.parts.lo = self.read_memory(self.hl.val),
                0x4F => self.bc.parts.lo = self.af.parts.hi,
                0x50 => self.de.parts.hi = self.bc.parts.hi,
                0x51 => self.de.parts.hi = self.bc.parts.lo,
                0x52 => self.de.parts.hi = self.de.parts.hi,
                0x53 => self.de.parts.hi = self.de.parts.lo,
                0x54 => self.de.parts.hi = self.hl.parts.hi,
                0x55 => self.de.parts.hi = self.hl.parts.lo,
                0x56 => self.de.parts.hi = self.read_memory(self.hl.val),
                0x57 => self.de.parts.hi = self.af.parts.hi,
                0x58 => self.de.parts.lo = self.bc.parts.hi,
                0x59 => self.de.parts.lo = self.bc.parts.lo,
                0x5A => self.de.parts.lo = self.de.parts.hi,
                0x5B => self.de.parts.lo = self.de.parts.lo,
                0x5C => self.de.parts.lo = self.hl.parts.hi,
                0x5D => self.de.parts.lo = self.hl.parts.lo,
                0x5E => self.de.parts.lo = self.read_memory(self.hl.val),
                0x5F => self.de.parts.lo = self.af.parts.hi,
                0x60 => self.hl.parts.hi = self.bc.parts.hi,
                0x61 => self.hl.parts.hi = self.bc.parts.lo,
                0x62 => self.hl.parts.hi = self.de.parts.hi,
                0x63 => self.hl.parts.hi = self.de.parts.lo,
                0x64 => self.hl.parts.hi = self.hl.parts.hi,
                0x65 => self.hl.parts.hi = self.hl.parts.lo,
                0x66 => self.hl.parts.hi = self.read_memory(self.hl.val),
                0x67 => self.hl.parts.hi = self.af.parts.hi,
                0x68 => self.hl.parts.lo = self.bc.parts.hi,
                0x69 => self.hl.parts.lo = self.bc.parts.lo,
                0x6A => self.hl.parts.lo = self.de.parts.hi,
                0x6B => self.hl.parts.lo = self.de.parts.lo,
                0x6C => self.hl.parts.lo = self.hl.parts.hi,
                0x6D => self.hl.parts.lo = self.hl.parts.lo,
                0x6E => self.hl.parts.lo = self.read_memory(self.hl.val),
                0x6F => self.hl.parts.lo = self.af.parts.hi,
                0x70 => self.write_memory(self.hl.val, self.bc.parts.hi),
                0x71 => self.write_memory(self.hl.val, self.bc.parts.lo),
                0x72 => self.write_memory(self.hl.val, self.de.parts.hi),
                0x73 => self.write_memory(self.hl.val, self.de.parts.lo),
                0x74 => self.write_memory(self.hl.val, self.hl.parts.hi),
                0x75 => self.write_memory(self.hl.val, self.hl.parts.lo),
                0x77 => self.write_memory(self.hl.val, self.af.parts.hi),
                0x78 => self.af.parts.hi = self.bc.parts.hi,
                0x79 => self.af.parts.hi = self.bc.parts.lo,
                0x7A => self.af.parts.hi = self.de.parts.hi,
                0x7B => self.af.parts.hi = self.de.parts.lo,
                0x7C => self.af.parts.hi = self.hl.parts.hi,
                0x7D => self.af.parts.hi = self.hl.parts.lo,
                0x7E => self.af.parts.hi = self.read_memory(self.hl.val),
                0x7F => self.af.parts.hi = self.af.parts.hi,
                0x3E => self.af.parts.hi = self.get_next_byte(),
                0xE2 => self.write_memory(0xFF00 + (self.bc.parts.lo as Word), self.af.parts.hi),
                0xEA => {
                    self.sync_cycles(8);
                    let addr = self.get_next_word();
                    self.write_memory(addr, self.af.parts.hi);
                },
                0xF2 => self.af.parts.hi = self.read_memory(0xFF00 + (self.bc.parts.lo as Word)),
                0xF8 => {
                    let offset = self.get_next_byte_signed();
                    if offset > 0 {
                        self.hl.val = self.stack_pointer.wrapping_add(offset as Word);
                        self.update_carry_flag((self.stack_pointer & 0xFF) + ((offset as Word) & 0xFF) > 0xFF);
                        self.update_half_carry_flag((self.stack_pointer & 0xF) + ((offset as Word) & 0xF) > 0xF);
                    } else {
                        self.hl.val = self.stack_pointer.wrapping_sub(offset.abs() as Word);
                        self.update_carry_flag((self.stack_pointer & 0xFF) + ((offset as Word) & 0xFF) > 0xFF);
                        self.update_half_carry_flag((self.stack_pointer & 0xF) + ((offset as Word) & 0xF) > 0xF);
                    }

                    self.update_zero_flag(false);
                    self.update_sub_flag(false);
                },
                0xF9 => self.stack_pointer = self.hl.val,
                0xFA => {
                    let word = self.get_next_word();
                    self.sync_cycles(8);
                    self.af.parts.hi = self.read_memory(word);
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_load_h(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0xE0 => {
                    self.sync_cycles(4);
                    let addr = self.get_next_byte();
                    self.write_memory(0xFF00 | addr as Word, self.af.parts.hi);
                },
                0xF0 => {
                    let addr = self.get_next_byte();
                    self.sync_cycles(4);
                    self.af.parts.hi = self.read_memory(0xFF00 | addr as Word);
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_or(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let to_or = match opcode.code {
                0xB0 => self.bc.parts.hi,
                0xB1 => self.bc.parts.lo,
                0xB2 => self.de.parts.hi,
                0xB3 => self.de.parts.lo,
                0xB4 => self.hl.parts.hi,
                0xB5 => self.hl.parts.lo,
                0xB6 => self.read_memory(self.hl.val),
                0xB7 => self.af.parts.hi,
                0xF6 => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.af.parts.hi |= to_or;

            self.update_zero_flag(self.af.parts.hi == 0);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);
            self.update_carry_flag(false);

            opcode.cycles
        }
    }

    fn do_pop(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0xC1 => self.bc.val = self.pop_word_from_stack(),
                0xD1 => self.de.val = self.pop_word_from_stack(),
                0xE1 => self.hl.val = self.pop_word_from_stack(),
                0xF1 => {
                    self.af.val = self.pop_word_from_stack();
                    self.af.parts.lo &= 0xF0;
                },
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_prefix(&mut self) -> u8 {
        let op = self.read_memory(self.program_counter);
        let opcode = PREFIX_OPCODE_MAP
            .get(&op)
            .expect(&format!("Prefix OpCode 0x{:02x} is not recognized", op));

        self.program_counter = self.program_counter.wrapping_add(1);

        match opcode.operation {
            Operation::BIT => self.do_bit(&opcode),
            Operation::RES => self.do_res(&opcode),
            Operation::RL => self.do_rotate_left(&opcode, true),
            Operation::RLC => self.do_rotate_left(&opcode, false),
            Operation::RR => self.do_rotate_right(&opcode, true),
            Operation::RRC => self.do_rotate_right(&opcode, false),
            Operation::SET => self.do_set(&opcode),
            Operation::SLA => self.do_shift_left(&opcode),
            Operation::SRA => self.do_shift_right(&opcode, true),
            Operation::SRL => self.do_shift_right(&opcode, false),
            Operation::SWAP => self.do_swap(&opcode),
            _ => panic!("Operation not found - {}", opcode.operation)
        }
    }

    fn do_push(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            match opcode.code {
                0xC5 => self.push_word_to_stack(self.bc.val),
                0xD5 => self.push_word_to_stack(self.de.val),
                0xE5 => self.push_word_to_stack(self.hl.val),
                0xF5 => self.push_word_to_stack(self.af.val),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_return(&mut self, opcode: &OpCode) -> u8 {
        match opcode.code {
            0xC0 => {
                self.program_counter = if !self.is_zero_flag_set() { self.pop_word_from_stack() } else { self.program_counter };
                if self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xC8 => {
                self.program_counter = if self.is_zero_flag_set() { self.pop_word_from_stack() } else { self.program_counter };
                if !self.is_zero_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xC9 => {
                self.program_counter = self.pop_word_from_stack();
                opcode.cycles
            },
            0xD0 => {
                self.program_counter = if !self.is_carry_flag_set() { self.pop_word_from_stack() } else { self.program_counter };
                if self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xD8 => {
                self.program_counter = if self.is_carry_flag_set() {self.pop_word_from_stack()} else { self.program_counter };
                if !self.is_carry_flag_set() { opcode.alt_cycles.unwrap_or(opcode.cycles) } else { opcode.cycles }
            },
            0xD9 => {
                self.program_counter = self.pop_word_from_stack();
                self.interrupts_enabled = true;
                opcode.cycles
            },
            _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
        }
    }

    fn do_restart(&mut self, opcode: &OpCode) -> u8 {
        self.push_word_to_stack(self.program_counter);

        match opcode.code {
            0xC7 => self.program_counter = 0x00,
            0xCF => self.program_counter = 0x08,
            0xD7 => self.program_counter = 0x10,
            0xDF => self.program_counter = 0x18,
            0xE7 => self.program_counter = 0x20,
            0xEF => self.program_counter = 0x28,
            0xF7 => self.program_counter = 0x30,
            0xFF => self.program_counter = 0x38,
            _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
        }

        opcode.cycles
    }

    fn do_res(&mut self, opcode: &OpCode) -> u8 {

        unsafe {
            match opcode.code {
                0x80 => reset_bit(&mut self.bc.parts.hi, 0),
                0x81 => reset_bit(&mut self.bc.parts.lo, 0),
                0x82 => reset_bit(&mut self.de.parts.hi, 0),
                0x83 => reset_bit(&mut self.de.parts.lo, 0),
                0x84 => reset_bit(&mut self.hl.parts.hi, 0),
                0x85 => reset_bit(&mut self.hl.parts.lo, 0),
                0x86 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 0);
                    self.write_memory(self.hl.val, val);
                },
                0x87 => reset_bit(&mut self.af.parts.hi, 0),
                0x88 => reset_bit(&mut self.bc.parts.hi, 1),
                0x89 => reset_bit(&mut self.bc.parts.lo, 1),
                0x8A => reset_bit(&mut self.de.parts.hi, 1),
                0x8B => reset_bit(&mut self.de.parts.lo, 1),
                0x8C => reset_bit(&mut self.hl.parts.hi, 1),
                0x8D => reset_bit(&mut self.hl.parts.lo, 1),
                0x8E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 1);
                    self.write_memory(self.hl.val, val);
                },
                0x8F => reset_bit(&mut self.af.parts.hi, 1),
                0x90 => reset_bit(&mut self.bc.parts.hi, 2),
                0x91 => reset_bit(&mut self.bc.parts.lo, 2),
                0x92 => reset_bit(&mut self.de.parts.hi, 2),
                0x93 => reset_bit(&mut self.de.parts.lo, 2),
                0x94 => reset_bit(&mut self.hl.parts.hi, 2),
                0x95 => reset_bit(&mut self.hl.parts.lo, 2),
                0x96 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 2);
                    self.write_memory(self.hl.val, val);
                },
                0x97 => reset_bit(&mut self.af.parts.hi, 2),
                0x98 => reset_bit(&mut self.bc.parts.hi, 3),
                0x99 => reset_bit(&mut self.bc.parts.lo, 3),
                0x9A => reset_bit(&mut self.de.parts.hi, 3),
                0x9B => reset_bit(&mut self.de.parts.lo, 3),
                0x9C => reset_bit(&mut self.hl.parts.hi, 3),
                0x9D => reset_bit(&mut self.hl.parts.lo, 3),
                0x9E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 3);
                    self.write_memory(self.hl.val, val);
                },
                0x9F => reset_bit(&mut self.af.parts.hi, 3),
                0xA0 => reset_bit(&mut self.bc.parts.hi, 4),
                0xA1 => reset_bit(&mut self.bc.parts.lo, 4),
                0xA2 => reset_bit(&mut self.de.parts.hi, 4),
                0xA3 => reset_bit(&mut self.de.parts.lo, 4),
                0xA4 => reset_bit(&mut self.hl.parts.hi, 4),
                0xA5 => reset_bit(&mut self.hl.parts.lo, 4),
                0xA6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 4);
                    self.write_memory(self.hl.val, val);
                },
                0xA7 => reset_bit(&mut self.af.parts.hi, 4),
                0xA8 => reset_bit(&mut self.bc.parts.hi, 5),
                0xA9 => reset_bit(&mut self.bc.parts.lo, 5),
                0xAA => reset_bit(&mut self.de.parts.hi, 5),
                0xAB => reset_bit(&mut self.de.parts.lo, 5),
                0xAC => reset_bit(&mut self.hl.parts.hi, 5),
                0xAD => reset_bit(&mut self.hl.parts.lo, 5),
                0xAE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 5);
                    self.write_memory(self.hl.val, val);
                },
                0xAF => reset_bit(&mut self.af.parts.hi, 5),
                0xB0 => reset_bit(&mut self.bc.parts.hi, 6),
                0xB1 => reset_bit(&mut self.bc.parts.lo, 6),
                0xB2 => reset_bit(&mut self.de.parts.hi, 6),
                0xB3 => reset_bit(&mut self.de.parts.lo, 6),
                0xB4 => reset_bit(&mut self.hl.parts.hi, 6),
                0xB5 => reset_bit(&mut self.hl.parts.lo, 6),
                0xB6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 6);
                    self.write_memory(self.hl.val, val);
                },
                0xB7 => reset_bit(&mut self.af.parts.hi, 6),
                0xB8 => reset_bit(&mut self.bc.parts.hi, 7),
                0xB9 => reset_bit(&mut self.bc.parts.lo, 7),
                0xBA => reset_bit(&mut self.de.parts.hi, 7),
                0xBB => reset_bit(&mut self.de.parts.lo, 7),
                0xBC => reset_bit(&mut self.hl.parts.hi, 7),
                0xBD => reset_bit(&mut self.hl.parts.lo, 7),
                0xBE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    reset_bit(&mut val, 7);
                    self.write_memory(self.hl.val, val);
                },
                0xBF => reset_bit(&mut self.af.parts.hi, 7),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_rotate_left(&mut self, opcode: &OpCode, through_carry: bool) -> u8 {
        unsafe {
            let do_rotate = |val: &mut Byte, carry_bit: u8| {
                let most_significant_bit = get_bit_val(&val, 7);
                let res = (*val << 1) | (if through_carry { carry_bit } else { most_significant_bit });
                *val = res;
                (res, most_significant_bit)
            };

            let carry_bit = if self.is_carry_flag_set() {1} else {0};
            let (res, most_significant_bit) = match opcode.code {
                0x00 => do_rotate(&mut self.bc.parts.hi, carry_bit),
                0x01 => do_rotate(&mut self.bc.parts.lo, carry_bit),
                0x02 => do_rotate(&mut self.de.parts.hi, carry_bit),
                0x03 => do_rotate(&mut self.de.parts.lo, carry_bit),
                0x04 => do_rotate(&mut self.hl.parts.hi, carry_bit),
                0x05 => do_rotate(&mut self.hl.parts.lo, carry_bit),
                0x06 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, most_significant_bit) = do_rotate(&mut val, carry_bit);
                    self.write_memory(self.hl.val, val);
                    (res, most_significant_bit)
                },
                0x07 => do_rotate(&mut self.af.parts.hi, carry_bit),
                0x10 => do_rotate(&mut self.bc.parts.hi, carry_bit),
                0x11 => do_rotate(&mut self.bc.parts.lo, carry_bit),
                0x12 => do_rotate(&mut self.de.parts.hi, carry_bit),
                0x13 => do_rotate(&mut self.de.parts.lo, carry_bit),
                0x14 => do_rotate(&mut self.hl.parts.hi, carry_bit),
                0x15 => do_rotate(&mut self.hl.parts.lo, carry_bit),
                0x16 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, most_significant_bit) = do_rotate(&mut val, carry_bit);
                    self.write_memory(self.hl.val, val);
                    (res, most_significant_bit)
                },
                0x17 => do_rotate(&mut self.af.parts.hi, carry_bit),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(res == 0);
            self.update_carry_flag(most_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_rotate_right(&mut self, opcode: &OpCode, through_carry: bool) -> u8 {
        unsafe {
            let do_rotate = |val: &mut Byte, carry_bit: u8| {
                let least_significant_bit = get_bit_val(&val, 0);
                let res = (if through_carry { carry_bit << 7 } else { least_significant_bit << 7 }) | (*val >> 1);
                *val = res;
                (res, least_significant_bit)
            };

            let carry_bit = if self.is_carry_flag_set() {1} else {0};
            let (res, least_significant_bit) = match opcode.code {
                0x08 => do_rotate(&mut self.bc.parts.hi, carry_bit),
                0x09 => do_rotate(&mut self.bc.parts.lo, carry_bit),
                0x0A => do_rotate(&mut self.de.parts.hi, carry_bit),
                0x0B => do_rotate(&mut self.de.parts.lo, carry_bit),
                0x0C => do_rotate(&mut self.hl.parts.hi, carry_bit),
                0x0D => do_rotate(&mut self.hl.parts.lo, carry_bit),
                0x0E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, least_significant_bit) = do_rotate(&mut val, carry_bit);
                    self.write_memory(self.hl.val, val);
                    (res, least_significant_bit)
                },
                0x0F => do_rotate(&mut self.af.parts.hi, carry_bit),
                0x18 => do_rotate(&mut self.bc.parts.hi, carry_bit),
                0x19 => do_rotate(&mut self.bc.parts.lo, carry_bit),
                0x1A => do_rotate(&mut self.de.parts.hi, carry_bit),
                0x1B => do_rotate(&mut self.de.parts.lo, carry_bit),
                0x1C => do_rotate(&mut self.hl.parts.hi, carry_bit),
                0x1D => do_rotate(&mut self.hl.parts.lo, carry_bit),
                0x1E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, least_significant_bit) = do_rotate(&mut val, carry_bit);
                    self.write_memory(self.hl.val, val);
                    (res, least_significant_bit)
                },
                0x1F => do_rotate(&mut self.af.parts.hi, carry_bit),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(res == 0);
            self.update_carry_flag(least_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_rla(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let most_significant_bit = get_bit_val(&self.af.parts.hi, 7);
            let carry_bit = if self.is_carry_flag_set() {1} else {0};
            let res = (self.af.parts.hi << 1) | carry_bit;

            self.update_zero_flag(false);
            self.update_carry_flag(most_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            self.af.parts.hi = res;

            opcode.cycles
        }
    }

    fn do_rlca(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let most_significant_bit = get_bit_val(&self.af.parts.hi, 7);
            let res = (self.af.parts.hi << 1) | most_significant_bit;

            self.update_zero_flag(false);
            self.update_carry_flag(most_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            self.af.parts.hi = res;

            opcode.cycles
        }
    }

    fn do_rra(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let least_significant_bit = get_bit_val(&self.af.parts.hi, 0);
            let carry_bit = if self.is_carry_flag_set() {1} else {0};
            let res = (carry_bit << 7) | (self.af.parts.hi >> 1);

            self.update_zero_flag(false);
            self.update_carry_flag(least_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            self.af.parts.hi = res;

            opcode.cycles
        }
    }

    fn do_rrca(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let least_significant_bit = get_bit_val(&self.af.parts.hi, 0);
            let res = (least_significant_bit << 7) | (self.af.parts.hi >> 1);

            self.update_zero_flag(false);
            self.update_carry_flag(least_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            self.af.parts.hi = res;

            opcode.cycles
        }
    }

    fn do_set(&mut self, opcode: &OpCode) -> u8 {

        unsafe {
            match opcode.code {
                0xC0 => set_bit(&mut self.bc.parts.hi, 0),
                0xC1 => set_bit(&mut self.bc.parts.lo, 0),
                0xC2 => set_bit(&mut self.de.parts.hi, 0),
                0xC3 => set_bit(&mut self.de.parts.lo, 0),
                0xC4 => set_bit(&mut self.hl.parts.hi, 0),
                0xC5 => set_bit(&mut self.hl.parts.lo, 0),
                0xC6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 0);
                    self.write_memory(self.hl.val, val);
                },
                0xC7 => set_bit(&mut self.af.parts.hi, 0),
                0xC8 => set_bit(&mut self.bc.parts.hi, 1),
                0xC9 => set_bit(&mut self.bc.parts.lo, 1),
                0xCA => set_bit(&mut self.de.parts.hi, 1),
                0xCB => set_bit(&mut self.de.parts.lo, 1),
                0xCC => set_bit(&mut self.hl.parts.hi, 1),
                0xCD => set_bit(&mut self.hl.parts.lo, 1),
                0xCE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 1);
                    self.write_memory(self.hl.val, val);
                },
                0xCF => set_bit(&mut self.af.parts.hi, 1),
                0xD0 => set_bit(&mut self.bc.parts.hi, 2),
                0xD1 => set_bit(&mut self.bc.parts.lo, 2),
                0xD2 => set_bit(&mut self.de.parts.hi, 2),
                0xD3 => set_bit(&mut self.de.parts.lo, 2),
                0xD4 => set_bit(&mut self.hl.parts.hi, 2),
                0xD5 => set_bit(&mut self.hl.parts.lo, 2),
                0xD6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 2);
                    self.write_memory(self.hl.val, val);
                },
                0xD7 => set_bit(&mut self.af.parts.hi, 2),
                0xD8 => set_bit(&mut self.bc.parts.hi, 3),
                0xD9 => set_bit(&mut self.bc.parts.lo, 3),
                0xDA => set_bit(&mut self.de.parts.hi, 3),
                0xDB => set_bit(&mut self.de.parts.lo, 3),
                0xDC => set_bit(&mut self.hl.parts.hi, 3),
                0xDD => set_bit(&mut self.hl.parts.lo, 3),
                0xDE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 3);
                    self.write_memory(self.hl.val, val);
                },
                0xDF => set_bit(&mut self.af.parts.hi, 3),
                0xE0 => set_bit(&mut self.bc.parts.hi, 4),
                0xE1 => set_bit(&mut self.bc.parts.lo, 4),
                0xE2 => set_bit(&mut self.de.parts.hi, 4),
                0xE3 => set_bit(&mut self.de.parts.lo, 4),
                0xE4 => set_bit(&mut self.hl.parts.hi, 4),
                0xE5 => set_bit(&mut self.hl.parts.lo, 4),
                0xE6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 4);
                    self.write_memory(self.hl.val, val);
                },
                0xE7 => set_bit(&mut self.af.parts.hi, 4),
                0xE8 => set_bit(&mut self.bc.parts.hi, 5),
                0xE9 => set_bit(&mut self.bc.parts.lo, 5),
                0xEA => set_bit(&mut self.de.parts.hi, 5),
                0xEB => set_bit(&mut self.de.parts.lo, 5),
                0xEC => set_bit(&mut self.hl.parts.hi, 5),
                0xED => set_bit(&mut self.hl.parts.lo, 5),
                0xEE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 5);
                    self.write_memory(self.hl.val, val);
                },
                0xEF => set_bit(&mut self.af.parts.hi, 5),
                0xF0 => set_bit(&mut self.bc.parts.hi, 6),
                0xF1 => set_bit(&mut self.bc.parts.lo, 6),
                0xF2 => set_bit(&mut self.de.parts.hi, 6),
                0xF3 => set_bit(&mut self.de.parts.lo, 6),
                0xF4 => set_bit(&mut self.hl.parts.hi, 6),
                0xF5 => set_bit(&mut self.hl.parts.lo, 6),
                0xF6 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 6);
                    self.write_memory(self.hl.val, val);
                },
                0xF7 => set_bit(&mut self.af.parts.hi, 6),
                0xF8 => set_bit(&mut self.bc.parts.hi, 7),
                0xF9 => set_bit(&mut self.bc.parts.lo, 7),
                0xFA => set_bit(&mut self.de.parts.hi, 7),
                0xFB => set_bit(&mut self.de.parts.lo, 7),
                0xFC => set_bit(&mut self.hl.parts.hi, 7),
                0xFD => set_bit(&mut self.hl.parts.lo, 7),
                0xFE => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    set_bit(&mut val, 7);
                    self.write_memory(self.hl.val, val);
                },
                0xFF => set_bit(&mut self.af.parts.hi, 7),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            opcode.cycles
        }
    }

    fn do_set_carry_flag(&mut self, opcode: &OpCode) -> u8 {
        self.update_half_carry_flag(false);
        self.update_sub_flag(false);
        self.update_carry_flag(true);
        opcode.cycles
    }

    fn do_shift_left(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let do_shift = |val: &mut Byte| {
                let most_significant_bit = get_bit_val(&val, 7);
                let mut res = *val << 1;

                *val = res;
                (res, most_significant_bit)
            };

            let (res, most_significant_bit) = match opcode.code {
                0x20 => do_shift(&mut self.bc.parts.hi),
                0x21 => do_shift(&mut self.bc.parts.lo),
                0x22 => do_shift(&mut self.de.parts.hi),
                0x23 => do_shift(&mut self.de.parts.lo),
                0x24 => do_shift(&mut self.hl.parts.hi),
                0x25 => do_shift(&mut self.hl.parts.lo),
                0x26 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, most_significant_bit) = do_shift(&mut val);
                    self.write_memory(self.hl.val, val);
                    (res, most_significant_bit)
                },
                0x27 => do_shift(&mut self.af.parts.hi),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(res == 0);
            self.update_carry_flag(most_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_shift_right(&mut self, opcode: &OpCode, maintain_msb: bool) -> u8 {
        unsafe {
            let do_shift = |val: &mut Byte| {
                let most_significant_bit = get_bit_val(&val, 7);
                let least_significant_bit = get_bit_val(&val, 0);
                let mut res = *val >> 1;
                if maintain_msb {
                    res |= (most_significant_bit << 7);
                }

                *val = res;
                (res, least_significant_bit)
            };

            let (res, least_significant_bit) = match opcode.code {
                0x28 => do_shift(&mut self.bc.parts.hi),
                0x29 => do_shift(&mut self.bc.parts.lo),
                0x2A => do_shift(&mut self.de.parts.hi),
                0x2B => do_shift(&mut self.de.parts.lo),
                0x2C => do_shift(&mut self.hl.parts.hi),
                0x2D => do_shift(&mut self.hl.parts.lo),
                0x2E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, least_significant_bit) = do_shift(&mut val);
                    self.write_memory(self.hl.val, val);
                    (res, least_significant_bit)
                },
                0x2F => do_shift(&mut self.af.parts.hi),
                0x38 => do_shift(&mut self.bc.parts.hi),
                0x39 => do_shift(&mut self.bc.parts.lo),
                0x3A => do_shift(&mut self.de.parts.hi),
                0x3B => do_shift(&mut self.de.parts.lo),
                0x3C => do_shift(&mut self.hl.parts.hi),
                0x3D => do_shift(&mut self.hl.parts.lo),
                0x3E => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let (res, least_significant_bit) = do_shift(&mut val);
                    self.write_memory(self.hl.val, val);
                    (res, least_significant_bit)
                },
                0x3F => do_shift(&mut &mut self.af.parts.hi),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(res == 0);
            self.update_carry_flag(least_significant_bit == 1);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_sub(&mut self, opcode: &OpCode, with_carry: bool) -> u8 {
        unsafe {
            let to_sub = match opcode.code {
                0x90 => self.bc.parts.hi,
                0x91 => self.bc.parts.lo,
                0x92 => self.de.parts.hi,
                0x93 => self.de.parts.lo,
                0x94 => self.hl.parts.hi,
                0x95 => self.hl.parts.lo,
                0x96 => self.read_memory(self.hl.val),
                0x97 => self.af.parts.hi,
                0x98 => self.bc.parts.hi,
                0x99 => self.bc.parts.lo,
                0x9A => self.de.parts.hi,
                0x9B => self.de.parts.lo,
                0x9C => self.hl.parts.hi,
                0x9D => self.hl.parts.lo,
                0x9E => self.read_memory(self.hl.val),
                0x9F => self.af.parts.hi,
                0xD6 => self.get_next_byte(),
                0xDE => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            let carry = if with_carry && self.is_carry_flag_set() {1} else {0};
            let a_reg = self.af.parts.hi;
            let res = a_reg.wrapping_sub(to_sub).wrapping_sub(carry);
            let lower_nibble = (a_reg & 0xF) as Word;

            self.update_zero_flag(res == 0);
            self.update_sub_flag(true);
            self.update_carry_flag((a_reg as Word) < (to_sub as Word) + (carry as Word));
            self.update_half_carry_flag((a_reg & 0xF) < (to_sub & 0xF) + (carry as Byte));

            self.af.parts.hi = res;

            opcode.cycles
        }
    }

    fn do_swap(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let swap = |val: &mut Byte| {
                let res = ((*val & 0xF) << 4) | (*val >> 4);
                *val = res;
                res
            };

            let res = match opcode.code {
                0x30 => swap(&mut self.bc.parts.hi),
                0x31 => swap(&mut self.bc.parts.lo),
                0x32 => swap(&mut self.de.parts.hi),
                0x33 => swap(&mut self.de.parts.lo),
                0x34 => swap(&mut self.hl.parts.hi),
                0x35 => swap(&mut self.hl.parts.lo),
                0x36 => {
                    self.sync_cycles(4);
                    let mut val = self.read_memory(self.hl.val);
                    self.sync_cycles(4);
                    let res = swap(&mut val);
                    self.write_memory(self.hl.val, val);
                    res
                },
                0x37 => swap(&mut self.af.parts.hi),
                _ => panic!("Unknown prefix operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.update_zero_flag(res == 0);
            self.update_carry_flag(false);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);

            opcode.cycles
        }
    }

    fn do_xor(&mut self, opcode: &OpCode) -> u8 {
        unsafe {
            let to_xor = match opcode.code {
                0xA8 => self.bc.parts.hi,
                0xA9 => self.bc.parts.lo,
                0xAA => self.de.parts.hi,
                0xAB => self.de.parts.lo,
                0xAC => self.hl.parts.hi,
                0xAD => self.hl.parts.lo,
                0xAE => self.read_memory(self.hl.val),
                0xAF => self.af.parts.hi,
                0xEE => self.get_next_byte(),
                _ => panic!("Unknown operation encountered 0x{:02x} - {}", opcode.code, opcode.mnemonic),
            };

            self.af.parts.hi ^= to_xor;

            self.update_zero_flag(self.af.parts.hi == 0);
            self.update_half_carry_flag(false);
            self.update_sub_flag(false);
            self.update_carry_flag(false);

            opcode.cycles
        }
    }

    fn debug(&mut self) {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open("debug.txt")
            .unwrap();

        unsafe {
            let a = self.af.parts.hi;
            let f = self.af.parts.lo;
            let b = self.bc.parts.hi;
            let c = self.bc.parts.lo;
            let d = self.de.parts.hi;
            let e = self.de.parts.lo;
            let h = self.hl.parts.hi;
            let l = self.hl.parts.lo;
            let sp = self.stack_pointer;
            let pc = self.program_counter;

            let pc_1 = self.read_memory(self.program_counter);
            let pc_2 = self.read_memory(self.program_counter + 1);
            let pc_3 = self.read_memory(self.program_counter + 2);
            let pc_4 = self.read_memory(self.program_counter + 3);

            let stat = self.read_memory(LCD_STATUS_ADDR);
            let ly = self.read_memory(CURRENT_SCANLINE_ADDR);

            let line = format!("A: {:02X} F: {:02X} B: {:02X} C: {:02X} D: {:02X} E: {:02X} H: {:02X} L: {:02X} SP: {:04X} PC: 00:{:04X} ({:02X} {:02X} {:02X} {:02X}) STAT: {:02X} LY: {:02X}", a, f, b, c, d, e, h, l, sp, pc, pc_1, pc_2, pc_3, pc_4, stat, ly);
            if let Err(e) = writeln!(file, "{}", line) {
                eprintln!("Couldn't write to file: {}", e);
            }
        }
    }
}
