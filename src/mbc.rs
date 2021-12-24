use std::cmp;
use std::fmt;

use crate::rom::*;
use crate::utils::*;

#[derive(Debug)]
pub enum MbcType {
    MBC1,
    MBC2,
    MBC3,
}

pub trait Mbc {
    fn get_mbc_type(&self) -> MbcType;
    fn read_rom(&self, addr: Word) -> Byte;
    fn read_ram(&self, addr: Word) -> Byte;
    fn write_ram(&mut self, addr: Word, data: Byte);
    fn handle_banking(&mut self, addr: Word, data: Byte);
    fn get_external_ram(&self) -> &[Byte];
    fn load_external_ram(&mut self, buffer: Vec<Byte>);
}

impl fmt::Debug for dyn Mbc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MbcType")
         .field("type", &self.get_mbc_type())
         .finish()
    }
}

pub fn get_mbc(rom: &Rom) -> Option<Box<dyn Mbc>> {
    let rom_mode = rom.get_cartridge_type();
    match rom_mode {
        1 | 2 | 3 => Some(Box::new(Mbc1::new(rom))),
        _ => None
    }
}

pub struct Mbc1 {
    memory: Vec<Byte>,
    rom_bank: usize,
    ram_bank: usize,
    external_ram: [Byte; MAXIMUM_RAM_BANKS * RAM_BANK_SIZE],
    enable_ram: bool,
    number_of_rom_banks: u8,
    banking_mode: BankingMode,
}

impl Mbc1 {

    pub fn new(rom: &Rom) -> Mbc1 {
        let mut memory = Vec::new();
        for i in 0..rom.length() {
            memory.push(rom.get_byte(i));
        }

        Mbc1 {
            memory: memory,
            rom_bank: 1,
            ram_bank: 0,
            external_ram: [0; MAXIMUM_RAM_BANKS * RAM_BANK_SIZE],
            enable_ram: false,
            number_of_rom_banks: rom.get_number_of_banks() as u8,
            banking_mode: BankingMode::ROM,
        }
    }
}

impl Mbc for Mbc1 {
    fn get_mbc_type(&self) -> MbcType {
        MbcType::MBC1
    }

    fn read_rom(&self, addr: Word) -> Byte {
        let resolved_addr = (addr as usize) + (self.rom_bank * 0x4000);
        self.memory[resolved_addr]
    }

    fn read_ram(&self, addr: Word) -> Byte {
        self.external_ram[(addr as usize) + (self.ram_bank * RAM_BANK_SIZE) as usize]
    }

    fn write_ram(&mut self, addr: Word, data: Byte) {
        self.external_ram[(addr as usize) + (self.ram_bank * RAM_BANK_SIZE)] = data;
    }

    fn handle_banking(&mut self, addr: Word, data: Byte) {
        match addr {
            0x0000..=0x1FFF => if (data & 0xF) == 0xA {self.enable_ram = true} else {self.enable_ram = false},
            0x2000..=0x3FFF => {
                let new_rom_bank = data & 0x1F;

                // Preserve the high bits and set the lower 5 bits
                self.rom_bank = (self.rom_bank & 0b11100000) | (new_rom_bank as usize);

                // I don't know why for sure, but because ROM Bank 0 is written directly to memory and
                // we'll always read from there, setting bank to 0 doesn't make sense so incrememnt it
                if self.rom_bank == 0 {
                    self.rom_bank += 1;
                }

                if self.rom_bank > self.number_of_rom_banks as usize {
                    // If we request a bank greater than what the ROM has, we need to mask
                    // TODO see pandocs for details
                    println!("TODO TOO MANY BANK");
                }
            },
            0x4000..=0x5FFF => {
                // Set RAM Bank or ROM bank hi bits depending on banking mode
                // RAM Bank is set to bottom 3 bits and ROM bank sets the hi bits of its number
                match self.banking_mode {
                    BankingMode::RAM => self.ram_bank = (data & 0x03) as usize,
                    BankingMode::ROM => {
                        let new_rom_bank = data & 0xE0; // Top 3 bits

                        // Preserve the lo bits and set the higher 3 bits
                        self.rom_bank = (new_rom_bank | ((self.rom_bank as u8) & 0b00011111)) as usize;
                        if self.rom_bank == 0 {
                            self.rom_bank += 1;
                        }

                        if self.rom_bank > self.number_of_rom_banks as usize {
                            // If we request a bank greater than what the ROM has, we need to mask
                            // TODO see pandocs for details
                            println!("TOO MANY BANK");
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
                self.banking_mode = match is_bit_set(&data, 0) {
                    true => BankingMode::RAM,
                    false => BankingMode::ROM,
                };
            },
            _ => println!("Invalid address {}", addr)
        };
    }

    fn get_external_ram(&self) -> &[Byte] {
        &self.external_ram
    }

    fn load_external_ram(&mut self, buffer: Vec<Byte>) {
        let ram_len = self.external_ram.len();
        for i in 0..cmp::min(ram_len, buffer.len()) {
            self.external_ram[i] = buffer[i];
        }
    }
}