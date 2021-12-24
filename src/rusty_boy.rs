use crate::cpu::*;
use crate::joypad::*;
use crate::mmu::*;
use crate::ppu::*;
use crate::rom::*;
use crate::timer::*;
use crate::utils::*;

pub struct RustyBoy {
    cpu: Cpu,
    pause: bool,
}

impl RustyBoy {

    pub fn new(file: &str) -> RustyBoy {
        let rom = Rom::new(file);
        rom.debug_header();

        let mut joypad = Joypad::new();

        let mut mmu = Mmu::new(rom, joypad);
        mmu.reset();

        let mut timer = Timer::new();

        let mut ppu = Ppu::new();

        let mut cpu = Cpu::new(mmu, timer, ppu);
        cpu.reset();

        RustyBoy {
            cpu: cpu,
            pause: false,
        }

    }

    pub fn run(&mut self) {
        let mut frame_cycles = 0;

        if !self.pause {
            while frame_cycles < MAX_CYCLES_PER_FRAME {
                let cycles = self.cpu.execute();
                frame_cycles += cycles as usize;

                self.cpu.handle_interrupts();
            }
        }
    }

    pub fn get_screen(&self) -> &Vec<u8> {
        self.cpu.get_screen()
    }

    pub fn get_vram_tiles(&mut self) -> Vec<u8> {
        self.cpu.get_vram_tiles()
    }

    pub fn set_button_state(&mut self, button: usize) {
        self.cpu.set_button_state(button);
    }

    pub fn reset_button_state(&mut self, button: usize) {
        self.cpu.reset_button_state(button);
    }

    pub fn toggle_pause(&mut self) {
        self.pause = !self.pause;
        println!("Paused: {}", self.pause);
    }

    pub fn get_external_ram(&self) -> &[Byte] {
        self.cpu.get_external_ram()
    }

    pub fn load_external_ram(&mut self, buffer: Vec<Byte>) {
        self.cpu.load_external_ram(buffer);
    }

    pub fn debug(&self) {
        if self.pause {
            println!("\n---------------- PPU ----------------\n");
            println!("{}", self.cpu.debug_ppu());
            println!("\n---------------- MMU ----------------\n");
            println!("{}", self.cpu.debug_mmu());
            println!("\n---------------- CPU ----------------\n");
            println!("{}", self.cpu.debug_cpu());
        }
    }
}