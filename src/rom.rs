use std::fs;

use crate::utils::*;

#[derive(Debug)]
pub struct Rom {
    data: Vec<u8>
}

impl Rom {

    pub fn new(file: &str) -> Rom {
        let contents = fs::read(file)
            .expect("Something went wrong reading the file");

        Rom {
            data: contents
        }
    }

    pub fn debug_header(&self) {
        println!("\n---------------------------------\n");
        let rom_title: String = self.data[0x134..0x143].to_vec().into_iter().map(|c| c as char).collect();
        println!("ROM Title: {}", rom_title);
        println!("Cartridge Type: 0x{:02X}", self.data[0x147]);
        println!("Number of Banks: {}", self.get_number_of_banks());
        println!("\n---------------------------------\n");
    }

    pub fn get_byte(&self, addr: usize) -> Byte {
        self.data[addr]
    }

    pub fn length(&self) -> usize {
        self.data.len()
    }

    pub fn get_cartridge_type(&self) -> Byte {
        self.data[0x0147]
    }

    pub fn get_number_of_banks(&self) -> u16 {
        match self.data[0x0148] {
            0x00 => 2,
            0x01 => 4,
            0x02 => 8,
            0x03 => 16,
            0x04 => 32,
            0x05 => 64,
            0x06 => 128,
            0x07 => 256,
            0x08 => 512,
            0x52 => 72,
            0x53 => 80,
            0x54 => 96,
            _ => 2,
        }
    }

}