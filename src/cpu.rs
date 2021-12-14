use crate::mmu::*;
use crate::ops::*;
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
}

impl Cpu {

    pub fn new(mmu: Mmu) -> Cpu {

        Cpu {
            mmu: mmu,
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
        self.will_disable_interrupts = false;
    }

    pub fn execute(&mut self) -> u8 {
        // Reset the cycle tracker for mid iteration cycle syncing
        self.cycle_tracker = 0;

        let op = self.read_memory(self.program_counter);
        let opcode = OPCODE_MAP
            .get(&op)
            .expect(&format!("OpCode 0x{:02x} is not recognized", op));

        // If in HALT mode, don't execute any instructions and incremeny by 1 T-cycle (4 M-cycles)
        if self.halted {
            self.sync_cycles(4);
            return 4;
        }

        self.program_counter = self.program_counter.wrapping_add(1);

        let cycles = match opcode.operation {
            Operation::ADC => self.do_add(&opcode, true),
            Operation::ADD => self.do_add(&opcode, false),
            Operation::NOP => opcode.cycles,
            _ => panic!("Operation not found - {}", opcode.operation)
        };

        // Deal with interrupt enabling/disabling
        self.toggle_interrupts_enabled();
        self.last_op = Some(opcode.operation);

        // Sync remaining cycles for the instruction
        self.sync_cycles(cycles - self.cycle_tracker);

        cycles
    }

    fn sync_cycles(&mut self, cycles: u8) {
        // Instructions increment other components clock during execution
        // not all at once - this is used to be able to sync components
        // during execution

        // self.timer.update_timers(cycles)
        // self.ppu.update_graphics(cycles)

        self.cycle_tracker += cycles;
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
        self.stack_pointer.wrapping_sub(1);
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
        ((hi << 8) as Word) | lo as Word
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
}
