use std::cmp;

use crate::joypad::*;
use crate::mbc::*;
use crate::rom::*;
use crate::utils::*;

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
    oam_access: bool,
    color_pallette_access: bool,
    vram_access: bool,
    timer_frequency_changed: bool,
    rom: Rom,
    joypad: Joypad,
    mbc: Option<Box<dyn Mbc>>,

    // CGB Specifics
    // There are 2 VRAM banks, each of size 0x2000
    cgb_vram: [Byte; 0x2000 * 2],
    cgb_vram_bank: usize,
    cgb_background_palettes: [Byte; 64],
    cgb_object_palettes: [Byte; 64],
}

impl Mmu {

    pub fn new(rom: Rom, joypad: Joypad) -> Mmu {
        Mmu {
            memory: [0; MEMORY_SIZE],
            oam_access: true,
            color_pallette_access: true,
            vram_access: true,
            timer_frequency_changed: false,
            rom: rom,
            joypad: joypad,
            mbc: None,
            cgb_vram: [0; 0x2000 * 2],
            cgb_vram_bank: 0,
            cgb_background_palettes: [0; 64],
            cgb_object_palettes: [0; 64],
        }
    }

    pub fn debug(&self) -> String {
        // format!("TODO MMU")
        let color_1 = ((self.cgb_background_palettes[57] as Word) << 8) | (self.cgb_background_palettes[56] as Word);
        let color_2 = ((self.cgb_background_palettes[59] as Word) << 8) | (self.cgb_background_palettes[58] as Word);
        let color_3 = ((self.cgb_background_palettes[61] as Word) << 8) | (self.cgb_background_palettes[60] as Word);
        let color_4 = ((self.cgb_background_palettes[63] as Word) << 8) | (self.cgb_background_palettes[62] as Word);
        
        format!("BG Palette 7: {:04X} {:04X} {:04X} {:04X}", color_1, color_2, color_3, color_4)
    }

    pub fn get_external_ram(&self) -> &[Byte] {
        match &self.mbc {
            Some(mbc) => mbc.get_external_ram() ,
            None => &self.memory[0xA000..0xC000]
        }
    }

    pub fn load_external_ram(&mut self, buffer: Vec<Byte>) {
        match &mut self.mbc {
            Some(mbc) => mbc.load_external_ram(buffer),
            None => {
                let ram_len = 0xC000 - 0xA000;
                for i in 0..cmp::min(ram_len, buffer.len()) {
                    self.memory[0xA000 + i] = buffer[i];
                }
            }
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
            self.read_rom_bank(addr)

        } else if addr >= 0x8000 && addr < 0xA000 && self.is_cgb() {
            // If we are in color game boy mode, then we have multiple banks of VRAM, so we need to ensure
            // we are reading from the correct bank
            self.cgb_vram[((addr - 0x8000) as usize) + (0x2000 * self.cgb_vram_bank)]
        
        } else if addr >= 0xA000 && addr < 0xC000 {
            self.read_ram_bank(addr)
            
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
                0x8000..=0x9FFF => self.handle_vram_write(addr, data),
                0xA000..=0xBFFF => self.write_ram_bank(addr, data),
                0xE000..=0xFDFF => {
                    // This is echo RAM so write to Working RAM as well
                    self.memory[(addr - 0x2000) as usize] = data;
                    self.memory[addr as usize] = data;
                },
                0xFEA0..=0xFEFF => (),
                JOYPAD_REGISTER_ADDR => self.handle_joypad(addr, data),
                DIVIDER_REGISTER_ADDR | CURRENT_SCANLINE_ADDR => self.memory[addr as usize] = 0,
                0xFF46 => self.do_dma_transfer(data),
                0xFF4F => self.do_vram_bank_switch(addr, data),
                TIMER_CONTROL_ADDR => self.do_timer_control_update(data),
                BACKGROUND_PALETTE_DATA_ADDR => self.handle_cgb_palette_write(addr, data),
                OBJECT_PALETTE_DATA_ADDR => self.handle_cgb_palette_write(addr, data),
                _ => self.memory[addr as usize] = data
            };
        }
    }

    pub fn is_cgb(&self) -> bool {
        self.rom.is_cgb()
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

    pub fn get_cgb_vram(&self) -> &[Byte] {
        &self.cgb_vram
    }

    pub fn get_cgb_background_palettes(&self) -> &[Byte] {
        &self.cgb_background_palettes
    }

    pub fn get_cgb_object_palettes(&self) -> &[Byte] {
        &self.cgb_object_palettes
    }

    fn load_rom(&mut self) {
        let end_addr = 0x8000;
        for i in 0..cmp::min(end_addr, self.rom.length()) {
            self.memory[i] = self.rom.get_byte(i);
        }

        self.mbc = get_mbc(&self.rom)
    }

    fn read_rom_bank(&self, addr: Word) -> Byte {
        match &self.mbc {
            Some(mbc) => mbc.read_rom(addr - 0x4000),
            None => self.rom.get_byte(addr as usize),
        }
    }

    fn read_ram_bank(&self, addr: Word) -> Byte {
        match &self.mbc {
            Some(mbc) => mbc.read_ram(addr - 0xA000),
            None => self.memory[addr as usize],
        }
    }

    fn write_ram_bank(&mut self, addr: Word, data: Byte) {
        match &mut self.mbc {
            Some(mbc) => mbc.write_ram(addr - 0xA000, data),
            None => self.memory[addr as usize] = data,
        }
    }

    fn handle_banking(&mut self, addr: Word, data: Byte) {
        match &mut self.mbc {
            Some(mbc) => mbc.handle_banking(addr, data),
            None => {},
        }
    }

    fn handle_vram_write(&mut self, addr: Word, data: Byte) {
        // In CGB mode, write to appropriate VRAM Bank 
        match self.is_cgb() {
            true => self.cgb_vram[((addr - 0x8000) as usize) + (0x2000 * self.cgb_vram_bank)] = data,
            false => self.memory[addr as usize] = data,
        };
    }

    fn do_vram_bank_switch(&mut self, addr: Word, data: Byte) {
        if self.is_cgb() {
            // In color GB, get Bit 0 if data to determine what Bank to use for VRAM
            self.cgb_vram_bank = get_bit_val(&data, 0) as usize;
        }

        self.memory[addr as usize] = data;
    }

    fn handle_cgb_palette_write(&mut self, addr: Word, data: Byte) {
        let palette_index_addr = match addr {
            BACKGROUND_PALETTE_DATA_ADDR => BACKGROUND_PALETTE_INDEX_ADDR,
            OBJECT_PALETTE_DATA_ADDR => OBJECT_PALETTE_INDEX_ADDR,
            _ => panic!("Invalid address used for palette data. Did you call this function by mistake?")
        };

        // In CGB mode, we should handle a proper palette update, in DMG, just write the data to memory
        match self.is_cgb() {
            true => {
                // We write the data through this register, using the index register to figure out 
                // which CGB palette byte we should write to. We use the lower 6 bits to get an address
                // between 0 and 63 (4 bytes per palette and 8 palettes total)
                let palette_index = self.memory[palette_index_addr as usize];
                let auto_increment = is_bit_set(&palette_index, 7);
                let mut palette_addr = palette_index & 0b111111;  // bottom 6 bits here for addr

                if addr == BACKGROUND_PALETTE_DATA_ADDR {
                    self.cgb_background_palettes[palette_addr as usize] = data;
                } else if addr == OBJECT_PALETTE_DATA_ADDR {
                    self.cgb_object_palettes[palette_addr as usize] = data;
                }

                // If the auto increment bit is set, then increment the palette address stored in those lower
                // 6 bits
                if auto_increment {
                    palette_addr = (palette_addr + 1) & 0b111111;
                    let new_idx = (palette_index & 0b11000000) | palette_addr;
                    self.memory[palette_index_addr as usize] = new_idx;
                }
            },
            false => self.memory[palette_index_addr as usize] = data
        }
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