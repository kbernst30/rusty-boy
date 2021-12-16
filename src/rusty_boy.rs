use crate::cpu::*;
use crate::mmu::*;
use crate::rom::*;
use crate::timer::*;
use crate::utils::*;

pub struct RustyBoy {
    cpu: Cpu,
}

impl RustyBoy {

    pub fn new(file: &str) -> RustyBoy {
        let rom = Rom::new(file);
        rom.debug_header();

        let mut mmu = Mmu::new(rom);
        mmu.reset();

        let mut timer = Timer::new();

        let mut cpu = Cpu::new(mmu, timer);
        cpu.reset();

        RustyBoy {
            cpu: cpu
        }

    }

    pub fn run(&mut self) {
        let mut frame_cycles = 0;

        while frame_cycles < MAX_CYCLES_PER_FRAME {
            let cycles = self.cpu.execute();
            frame_cycles += cycles as usize;

            // TODO interrupts
            // interrupt = self.interrupts.get_servicable_interrupt()
            // if interrupt is not None:
            //     self.cpu.service_interrupt(interrupt)
        }
    }

}