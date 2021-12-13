use crate::mmu::*;
use crate::rom::*;
use crate::utils::*;

#[derive(Debug)]
pub struct RustyBoy {
    mmu: Mmu,
}

impl RustyBoy {

    pub fn new(file: &str) -> RustyBoy {
        let rom = Rom::new(file);
        rom.debug_header();

        let mut mmu = Mmu::new(rom);
        mmu.reset();

        RustyBoy {
            mmu: mmu
        }

    }

    pub fn run(&self) {

    }

}