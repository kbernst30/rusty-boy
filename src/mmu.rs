use std::cmp;

use crate::joypad::*;
use crate::rom::*;
use crate::utils::*;

#[derive(Debug)]
enum BankingMode {
    RAM,
    ROM
}

#[derive(Debug)]
pub struct Mmu {
    /**
    * Memory Management Unit for the Gameboy. Memory has a 16 bit address bus and is broken down as follows:
    *    0000 - 3FFF	    16 KiB ROM bank 00	            From cartridge, usually a fixed bank
    *    4000 - 7FFF	    16 KiB ROM Bank 01~NN	        From cartridge, switchable bank via mapper (if any)
    *    8000 - 9FFF	    8 KiB Video RAM (VRAM)	        In CGB mode, switchable bank 0/1
    *    A000 - BFFF	    8 KiB External RAM	            From cartridge, switchable bank if any
    *    C000 - CFFF	    4 KiB Work RAM (WRAM)
    *    D000 - DFFF	    4 KiB Work RAM (WRAM)	        In CGB mode, switchable bank 1~7
    *    E000 - FDFF	    Mirror of C000~DDFF (ECHO RAM)	Nintendo says use of this area is prohibited.
    *    FE00 - FE9F	    Sprite attribute table (OAM)
    *    FEA0 - FEFF	    Not Usable	                    Nintendo says use of this area is prohibited
    *    FF00 - FF7F	    I/O Registers
    *    FF80 - FFFE	    High RAM (HRAM)
    *    FFFF - FFFF	    Interrupt Enable register (IE)
    **/

    memory: [Byte; MEMORY_SIZE],
    ram_banks: [Byte; MAXIMUM_RAM_BANKS * RAM_BANK_SIZE],
    enable_ram: bool,
    oam_access: bool,
    color_pallette_access: bool,
    vram_access: bool,
    rom_bank: Byte,
    ram_bank: Word,
    mbc1: bool,
    mbc2: bool,
    number_of_rom_banks: usize,
    timer_frequency_changed: bool,
    rom: Rom,
    joypad: Joypad,
    banking_mode: BankingMode,
}

impl Mmu {

    pub fn new(rom: Rom, joypad: Joypad) -> Mmu {
        Mmu {
            memory: [0; MEMORY_SIZE],
            ram_banks: [0; MAXIMUM_RAM_BANKS * RAM_BANK_SIZE],
            enable_ram: false,
            oam_access: true,
            color_pallette_access: true,
            vram_access: true,
            rom_bank: 1,
            ram_bank: 0,
            mbc1: false,
            mbc2: false,
            number_of_rom_banks: 2,
            timer_frequency_changed: false,
            rom: rom,
            joypad: joypad,
            banking_mode: BankingMode::ROM
        }
    }

    pub fn debug(&self) -> String {
        format!("Enable RAM: {}\nROM Bank: {}\nNumber of ROM Banks: {}\n", self.enable_ram, self.rom_bank, self.number_of_rom_banks)
    }

    pub fn get_external_ram(&self) -> &[Byte] {
        &self.ram_banks
    }

    pub fn load_external_ram(&mut self, buffer: Vec<Byte>) {
        let ram_len = self.ram_banks.len();
        for i in 0..cmp::min(ram_len, buffer.len()) {
            self.ram_banks[i] = buffer[i];
        }
    }

    pub fn reset(&mut self) {
        // Initial MMU state
        self.memory[0xFF05] = 0x00;
        self.memory[0xFF06] = 0x00;
        self.memory[0xFF07] = 0x00;
        self.memory[0xFF10] = 0x80;
        self.memory[0xFF11] = 0xBF;
        self.memory[0xFF12] = 0xF3;
        self.memory[0xFF14] = 0xBF;
        self.memory[0xFF16] = 0x3F;
        self.memory[0xFF17] = 0x00;
        self.memory[0xFF19] = 0xBF;
        self.memory[0xFF1A] = 0x7F;
        self.memory[0xFF1B] = 0xFF;
        self.memory[0xFF1E] = 0xBF;
        self.memory[0xFF20] = 0xFF;
        self.memory[0xFF21] = 0x00;
        self.memory[0xFF22] = 0x00;
        self.memory[0xFF23] = 0xBF;
        self.memory[0xFF24] = 0x77;
        self.memory[0xFF25] = 0xF3;
        self.memory[0xFF26] = 0xF1;
        self.memory[0xFF40] = 0x91;
        self.memory[0xFF42] = 0x00;
        self.memory[0xFF43] = 0x00;
        self.memory[0xFF45] = 0x00;
        self.memory[0xFF47] = 0xFC;
        self.memory[0xFF48] = 0xFF;
        self.memory[0xFF49] = 0xFF;
        self.memory[0xFF4A] = 0x00;
        self.memory[0xFF4B] = 0x00;
        self.memory[0xFFFF] = 0x00;

        // This iniital state of the joypad is all unpressed
        self.memory[JOYPAD_REGISTER_ADDR as usize] = 0xFF;

        // TEMP
        // self.memory[0xFF44] = 0x90;

        self.rom_bank = 1;

        self.load_rom();
    }

    pub fn read_byte(&self, addr: Word) -> Byte {
        let is_reading_restricted_oam = addr >= 0xFE00 && addr <= 0xFE9F && !self.oam_access;
        let is_reading_restricted_vram = addr >= 0x8000 && addr <= 0x9FFF && !self.vram_access;

        if is_reading_restricted_oam || is_reading_restricted_vram {
            // Reading something currently restricted, return garbage (0xFF)
            0xFF
        } else if addr >= 0x4000 && addr < 0x8000 {
            // First ROM bank will always be mapped into memory, but anything in this range might
            // use a different bank, so let's find the appropriate bank to read from
            // This address should be bigger than a Word as ROM might have more than can fit into a Word
            let resolved_addr = (addr as usize - 0x4000) + (self.rom_bank as usize * 0x4000);
            self.rom.get_byte(resolved_addr)
        } else if addr >= 0xA000 && addr < 0xC000 {
            self.ram_banks[((addr - 0xA000) as usize) + ((self.ram_bank as usize) * RAM_BANK_SIZE) as usize]

        } else {
            self.memory[addr as usize]
        }
    }

    pub fn write_byte(&mut self, addr: Word, data: Byte) {
        let is_writing_restricted_oam = addr >= 0xFE00 && addr <= 0xFE9F && !self.oam_access;
        let is_writing_restricted_vram = addr >= 0x8000 && addr <= 0x9FFF && !self.vram_access;

        if !is_writing_restricted_oam && !is_writing_restricted_vram {
            match addr {
                0x0000..=0x7FFF => self.handle_banking(addr, data),

                // Write to external RAM - choose appropriate Bank
                0xA000..=0xBFFF => self.ram_banks[((addr - 0xA000) as usize) + ((self.ram_bank as usize) * RAM_BANK_SIZE)] = data,
                0xE000..=0xFDFF => {
                    // This is echo RAM so write to Working RAM as well
                    if self.enable_ram {
                        self.memory[(addr - 0x2000) as usize] = data;
                        self.memory[addr as usize] = data;
                    }
                },
                0xFEA0..=0xFEFF => (),
                JOYPAD_REGISTER_ADDR => self.handle_joypad(addr, data),
                DIVIDER_REGISTER_ADDR | CURRENT_SCANLINE_ADDR => self.memory[addr as usize] = 0,
                0xFF46 => self.do_dma_transfer(data),
                TIMER_CONTROL_ADDR => self.do_timer_control_update(data),
                _ => self.memory[addr as usize] = data
            };
        }
    }

    pub fn update_timer_frequency_changed(&mut self, val: bool) {
        self.timer_frequency_changed = val;
    }

    pub fn is_timer_frequency_changed(&self) -> bool {
        self.timer_frequency_changed
    }

    pub fn update_scanline(&mut self) {
        self.memory[CURRENT_SCANLINE_ADDR as usize] = self.memory[CURRENT_SCANLINE_ADDR as usize].wrapping_add(1);
    }

    pub fn reset_scanline(&mut self) {
        self.memory[CURRENT_SCANLINE_ADDR as usize] = 0;
    }

    pub fn restrict_oam_access(&mut self) {
        self.oam_access = false;
    }

    pub fn open_oam_access(&mut self) {
        self.oam_access = true;
    }

    pub fn restrict_color_pallette_access(&mut self) {
        self.color_pallette_access = false;
    }

    pub fn open_color_pallette_access(&mut self) {
        self.color_pallette_access = true;
    }

    pub fn restrict_vram_access(&mut self) {
        self.vram_access = false;
    }

    pub fn open_vram_access(&mut self) {
        self.vram_access = true
    }

    pub fn increment_timer_register(&mut self) {
        self.memory[TIMER_ADDR as usize] = self.memory[TIMER_ADDR as usize].wrapping_add(1);
    }

    pub fn increment_divider_register(&mut self) {
        self.memory[DIVIDER_REGISTER_ADDR as usize] = self.memory[DIVIDER_REGISTER_ADDR as usize].wrapping_add(1);
    }

    pub fn set_button_state(&mut self, button: usize) {
        self.joypad.set_button_state(button);
    }

    pub fn reset_button_state(&mut self, button: usize) {
        self.joypad.reset_button_state(button);
    }

    fn load_rom(&mut self) {
        let end_addr = 0x8000;
        for i in 0..cmp::min(end_addr, self.rom.length()) {
            self.memory[i] = self.rom.get_byte(i);
        }

        // Select proper MBC mode
        // TODO this is not clean - might be better way to do this
        let rom_mode = self.rom.get_cartridge_type();
        if rom_mode == 1 || rom_mode == 2 || rom_mode == 3 {
            self.mbc1 = true;
        } else if rom_mode == 5 || rom_mode == 6 {
            self.mbc2 = true;
        }

        self.number_of_rom_banks = self.rom.get_number_of_banks() as usize;
    }

    fn handle_banking(&mut self, addr: Word, data: Byte) {
        match addr {
            0x0000..=0x1FFF => if (data & 0xF) == 0xA {self.enable_ram = true} else {self.enable_ram = false},
            0x2000..=0x3FFF => {
                if self.mbc1 {
                    let new_rom_bank = data & 0x1F;

                    // Preserve the high bits and set the lower 5 bits
                    self.rom_bank = (self.rom_bank & 0b11100000) | new_rom_bank;

                    // I don't know why for sure, but because ROM Bank 0 is written directly to memory and
                    // we'll always read from there, setting bank to 0 doesn't make sense so incrememnt it
                    if self.rom_bank == 0 {
                        println!("LO OOPS");
                        self.rom_bank += 1;
                    }

                    if self.rom_bank > self.number_of_rom_banks as u8 {
                        // If we request a bank greater than what the ROM has, we need to mask
                        // TODO see pandocs for details
                        println!("TOO MANY BANK");
                    }
                }
            },
            0x4000..=0x5FFF => {
                // Set RAM Bank or ROM bank hi bits depending on banking mode
                // RAM Bank is set to bottom 3 bits and ROM bank sets the hi bits of its number
                match self.banking_mode {
                    BankingMode::RAM => self.ram_bank = (data & 0x03) as Word,
                    BankingMode::ROM => {
                        let new_rom_bank = data & 0xE0; // Top 3 bits

                        // Preserve the lo bits and set the higher 3 bits
                        self.rom_bank = new_rom_bank | (self.rom_bank & 0b00011111);
                        if self.rom_bank == 0 {
                            println!("HI OOPS");
                        }

                        if self.rom_bank > self.number_of_rom_banks as u8 {
                            // If we request a bank greater than what the ROM has, we need to mask
                            // TODO see pandocs for details
                            println!("TOO MANY BANK");
                            self.rom_bank += 1;
                        }
                    },
                };
            },
            0x6000..=0x7FFF => {
                // For MBC1 Change the Banking mode to either RAM or ROM so we can decide
                // which bank number to adjust when writing to addr 0x4000 - 0x5FFF
                // To do this, we check the least signifcant bit of the data being written
                //   0 = ROM Banking Mode (Default)
                //   1 = RAM Banking Mode
                if self.mbc1 {
                    self.banking_mode = match is_bit_set(&data, 0) {
                        true => BankingMode::RAM,
                        false => BankingMode::ROM,
                    };
                }
            },
            _ => println!("Invalid address {}", addr)
        };
    }

    fn handle_joypad(&mut self, addr: Word, data: Byte) {
        // If bit 5 of the data being written is unset, then we should
        // Fetch the Action buttons, if bit 4 is unset, fetch direction
        let mode_bits = (data >> 4) & 0x3;
        let mode = match mode_bits {
            1 => Some(JoypadMode::ACTION),
            2 => Some(JoypadMode::DIRECTION),
            _ => None
        };

        if let Some(joypad_mode) = mode {
            let lower_nibble = self.joypad.get_buttons_for_mode(joypad_mode);
            self.memory[addr as usize] = (data & 0xF0) | lower_nibble;
        } else {
            self.memory[addr as usize] = (data & 0xF0) | 0xF;
        }
    }

    fn do_dma_transfer(&mut self, data: Byte) {
        // When writing to register 0xFF46, copy data from RAM/ROM to Object Attribute
        // Memory (OAM - FE00 - FE9F)

        // We want to copy starting at source address (data) multipled by $100 (256) - this
        // is because this data is supposed to be the source / 0x100

        // This source becomes address $XX00-$XX9F where XX is determined by that data value

        let start_addr = data as Word * 0x100;
        for i in 0..0xA0 {
            // Range should be to 0xA0 as it is inclusive of value 0x9F this way
            self.memory[0xFE00 + i] = self.read_byte(start_addr + i as Word);
        }
    }

    fn do_timer_control_update(&mut self, data: Byte) {
        self.update_timer_frequency_changed(true);
        self.memory[TIMER_CONTROL_ADDR as usize] = data;
    }

}