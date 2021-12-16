#[macro_use]
extern crate lazy_static;
extern crate sdl2;

pub mod cpu;
pub mod mmu;
pub mod ops;
pub mod rom;
pub mod rusty_boy;
pub mod utils;

use std::env;
use std::collections::HashMap;
use std::time::Duration;

use sdl2::event::Event;
use sdl2::EventPump;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::TextureCreator;

use crate::utils::*;
use crate::rusty_boy::RustyBoy;

fn main() {
    let mut key_map = HashMap::new();
    key_map.insert(Keycode::Down, DOWN_BUTTON);
    key_map.insert(Keycode::Up, UP_BUTTON);
    key_map.insert(Keycode::Right, RIGHT_BUTTON);
    key_map.insert(Keycode::Left, LEFT_BUTTON);
    key_map.insert(Keycode::Space, SELECT_BUTTON);
    key_map.insert(Keycode::Return, START_BUTTON);
    key_map.insert(Keycode::A, A_BUTTON);
    key_map.insert(Keycode::S, B_BUTTON);

    // Initialize SDL
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window("Rusty Boy", (SCREEN_WIDTH * DISPLAY_FACTOR) as u32, (SCREEN_HEIGHT * DISPLAY_FACTOR) as u32)
        .position_centered()
        .build().unwrap();

    let mut canvas = window.into_canvas().present_vsync().build().unwrap();

    let mut event_pump = sdl_context.event_pump().unwrap();
    canvas.set_scale(DISPLAY_FACTOR as f32, DISPLAY_FACTOR as f32).unwrap();

    // Setup emulator
    let args: Vec<String> = env::args().collect();
    let mut rusty_boy = RustyBoy::new(&args[1]);

    'running: loop {
        rusty_boy.run();

        canvas.clear();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    break 'running;
                },
                _ => {}
            }
        }

        canvas.present();

        // Run at Gameboy desired Frame rate
        // ::std::thread::sleep(Duration::new(0, (1_000_000_000.0 / 59.7275f32).floor() as u32));
    }

    // let mut creator = canvas.texture_creator();
    // let mut texture = creator
    //     .create_texture_target(PixelFormatEnum::RGB24, SCREEN_WIDTH, SCREEN_HEIGHT).unwrap();

    // canvas.present();
}
