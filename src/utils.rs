use std::collections::HashMap;

pub type Byte = u8;
pub type SignedByte = i8;
pub type Word = u16;
pub type SignedWord = i16;

pub const SCREEN_WIDTH: u32 = 160;
pub const SCREEN_HEIGHT: u32 = 144;
pub const DISPLAY_FACTOR: u32 = 4;

pub const MEMORY_SIZE: usize = 0x10000;

// Cycles per frame is determined by the clock frequency of the CPU (4.194304 MHz)
// And the number of expected frames per second (~60) - to make this accurage it should be 59.7275
pub const CLOCK_SPEED: usize = 4194304;
pub const MAX_CYCLES_PER_FRAME: usize = (CLOCK_SPEED as f32 / 59.7275) as usize;

pub const PROGRAM_COUNTER_INIT: Word = 0x100;
pub const STACK_POINTER_INIT: Word = 0xFFFE;

// Timers
pub const DIVIDER_REGISTER_ADDR: Word = 0xFF04;
pub const TIMER_ADDR: Word = 0xFF05;
pub const TIMER_MODULATOR_ADDR: Word = 0xFF06;  // The value at this address is what the timer is set to upon overflow
pub const TIMER_CONTROL_ADDR: Word = 0xFF07;
pub const CYCLES_PER_DIVIDER_INCREMENT: usize = 256;

// LCD and Graphics
// LCDC - the main LCD control register, located in memory. The different
// bits control what and how we display on screen:
//     7 - LCD/PPU enabled, 0 = disabled, 1 = enabled
//     6 - Window tile map area, 0 = 0x9800-0x9BFF, 1 = 0x9C00-0x9FFF
//     5 - Window enabled, 0 = disabled, 1 = enabled
//     4 - BG and Window tile data area, 0 = 0x8800-0x97FF, 1 = 0x8000-0x8FFFF
//     3 - BG tile map area, 0 = 0x9800-0x9BFF, 1 = 0x9C00-0x9FFF
//     2 - Object size, 0 = 8x8, 1 = 8x16
//     1 - Object enabled, 0 = disabled, 1 = enabled
//     0 - Background enabled, 0 = disabled, 1 = enabled
pub const LCD_CONTROL_ADDR: Word = 0xFF40;

// STAT - the main LCD status register, located in memory. The different bits
// indicate the status of the LCD
//     6 - LYC = LY Interrupt - if enabled and LYC = LY, request LCD interrupt
//     5 - Mode 2 (Searching Sprites) interrupt enabled
//     4 - Mode 1 (VBlank) interrupt enabled
//     3 - Mode 0 (Hblank) interrupt enabled
//     2 - LYC = LY - Set if current scanline (LY) is equal to value we are comparing to (LYC)
//     1, 0 - LCD mode
//      00: H-Blank Mode
//      01: V-Blank mode
//      10: Searching Sprites Atts
//      11: Transferring Data to LCD driver
pub const LCD_STATUS_ADDR: Word = 0xFF41;  // The address of the LCD status byte

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum LcdMode {
    H_BLANK = 0,
    V_BLANK = 1,
    SPRITE_SEARCH = 2,
    LCD_TRANSFER = 3,
}

// LY - Current Scanline being processed is written to this address
// This can hold values 0 - 153, but 144-153 indicate VBlank period,
// as there are only 144 vertical scanlines
pub const CURRENT_SCANLINE_ADDR: Word = 0xFF44;

// LYC - Current scanline compare value
pub const CURRENT_SCANLINE_COMPARE_ADDR: Word = 0xFF45;

pub const MAX_SCANLINE_VALUE: u8 = 153;

pub const CYCLES_PER_SCANLINE: isize = 456;  // It takes 456 clock cycles to draw one scanline

// SCX and SCY registers - Those specify the top-left coordinates
// of the visible 160×144 pixel area within the 256×256 pixels BG map.
// Values in the range 0–255 may be used.
pub const BACKGROUND_SCROLL_Y: Word = 0xFF42;
pub const BACKGROUND_SCROLL_X: Word = 0xFF43;

// WY and WX registers - these specify the top-left coordinates
// of the window, which can be displayed over the background
// WX is offset by +7 pixels, so a value of 7 places the window
// at x = 0
pub const WINDOW_POS_Y: Word = 0xFF4A;
pub const WINDOW_POS_X: Word = 0xFF4B;

pub const BG_COLOR_PALLETTE_ADDR: Word = 0xFF47;
pub const OBJ_COLOR_PALLETTE_ADDR_0: Word = 0xFF48;
pub const OBJ_COLOR_PALLETTE_ADDR_1: Word = 0xFF49;

// Banking
pub const ROM_BANKING_MODE_ADDR: Word = 0x147;
pub const RAM_BANK_COUNT_ADDR: Word = 0x148;
pub const MAXIMUM_RAM_BANKS: usize = 4;
pub const RAM_BANK_SIZE: usize = 0x2000;  // In bytes

#[derive(Debug)]
pub enum BankingMode {
    RAM,
    ROM
}

// Interrupts
// Known as IE (Interrupt Enable) register, which denotes which interrupts are currently enabled
// Bit 0 = VBlank Interrupt - INT $40
// Bit 1 = LCD Stat Interrupt - INT $48
// Bit 2 = Timer Interrupt - INT $50
// Bit 3 = Serial Interrupt - INT $58
// Bit 4 = Joypad Interrupt - INT $60
pub const INTERRUPT_ENABLE_ADDR: Word = 0xFFFF;

// Known as IF (Interrupt Flag) register, which denotes which interrupts are currently requested
// Bit 0 = VBlank Interrupt - INT $40
// Bit 1 = LCD Stat Interrupt - INT $48
// Bit 2 = Timer Interrupt - INT $50
// Bit 3 = Serial Interrupt - INT $58
// Bit 4 = Joypad Interrupt - INT $60
pub const INTERRUPT_FLAG_ADDR: Word = 0xFF0F;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Interrupt {
    V_BLANK,
    LCD_STAT,
    TIMER,
    SERIAL,
    JOYPAD,
}

// Joypad register - Bits are as follows:
// Bit 7 - Not used
// Bit 6 - Not used
// Bit 5 - P15 Select Action buttons    (0=Select)
// Bit 4 - P14 Select Direction buttons (0=Select)
// Bit 3 - P13 Input: Down  or Start    (0=Pressed) (Read Only)
// Bit 2 - P12 Input: Up    or Select   (0=Pressed) (Read Only)
// Bit 1 - P11 Input: Left  or B        (0=Pressed) (Read Only)
// Bit 0 - P10 Input: Right or A        (0=Pressed) (Read Only)
pub const JOYPAD_REGISTER_ADDR: Word = 0xFF00;

pub const RIGHT_BUTTON: usize = 0;
pub const LEFT_BUTTON: usize = 1;
pub const UP_BUTTON: usize = 2;
pub const DOWN_BUTTON: usize = 3;
pub const START_BUTTON: usize = 4;
pub const SELECT_BUTTON: usize = 5;
pub const B_BUTTON: usize = 6;
pub const A_BUTTON: usize = 7;

pub enum JoypadMode {
    DIRECTION,
    ACTION,
}

// Flags
// The F register contains flags for the CPU. The following bits
// represent the following flags:
// 7	z	Zero flag
// 6	n	Subtraction flag (BCD)
// 5	h	Half Carry flag (BCD)
// 4	c	Carry flag
pub const ZERO_FLAG: usize = 7;
pub const SUBTRACTION_FLAG: usize = 6;
pub const HALF_CARRY_FLAG: usize = 5;
pub const CARRY_FLAG: usize = 4;

// CGB Specifics
pub const VRAM_BANK_SELECT_ADDR: Word = 0xFF4F;
pub const VRAM_DMA_START_ADDR: Word = 0xFF51;
pub const VRAM_DMA_END_ADDR: Word = 0xFF55;
pub const WRAM_BANK_SELECT_ADDR: Word = 0xFF70;
pub const BACKGROUND_PALETTE_INDEX_ADDR: Word = 0xFF68;
pub const BACKGROUND_PALETTE_DATA_ADDR: Word = 0xFF69;

pub fn is_bit_set(data: &Byte, position: usize) -> bool {
    // Return true if bit at position is
    // set in data, false otherwise
    (data & (1 << position)) > 0
}

pub fn set_bit(data: &mut Byte, position: usize) {
    let setter = 1 << position;
    *data |= setter;
}

pub fn reset_bit(data: &mut Byte, position: usize) {
    let setter = !(1 << position); // Bit wise negate to get a 0 in the appropriate pos
    *data &= setter;
}

pub fn get_bit_val(data: &Byte, position: u8) -> u8 {
    match (data & (1 << position)) > 0 {
        true => 1,
        false => 0
    }
}

pub fn get_rgb888(rgb555: Byte) -> Byte {
    // Data written to CGB palettes are in RGB555 mode (i.e. only using 5 bits) - so convert
    // to propert RGB888 data to draw the correct color
    let lo_bits_888 = (rgb555 & 0x1F) >> 2;
    (rgb555 << 3) | lo_bits_888
}

lazy_static! {
    pub static ref GB_COLORS: HashMap<u8, (Byte, Byte, Byte)> = HashMap::from([
        (0, (0xFF, 0xFF, 0xFF)),
        (1, (0xCC, 0xCC, 0xCC)),
        (2, (0x77, 0x77, 0x77)),
        (3, (0x00, 0x00, 0x00)),
    ]);
}