use crate::interrupts::*;
use crate::mmu::*;
use crate::timer::*;
use crate::utils::*;

pub struct Ppu {
    scanline_counter: isize,
    screen: Vec<u8>,  // This needs to be a flat vec so SDL2 can accept this to update the texture
    debug: bool,
}

impl Ppu {
    pub fn new() -> Ppu {
        Ppu {
            scanline_counter: CYCLES_PER_SCANLINE,
            screen: vec![0; (SCREEN_WIDTH as usize) * (SCREEN_HEIGHT as usize) * 3],
            debug: true,
        }
    }

    pub fn get_screen(&self) -> &Vec<u8> {
        &self.screen
    }

    pub fn update_graphics(&mut self, mmu: &mut Mmu, cycles: u8) {
        // Attempt to update the graphics. If we have taken more than the number
        // of cycles needed to update a scanline, it is time to draw it
        //
        // In reality, CPU and PPU are running in parallel but we need to do this
        // a bit more synchornously

        self.update_lcd_status(mmu);

        // Only update the counter if the LCD is enabled
        if self.is_lcd_enabled(mmu) {
            self.scanline_counter -= cycles as isize;
        }

        // We have run the number of necessary cycles to draw a scanline
        if self.scanline_counter <= 0 {
            self.scanline_counter = CYCLES_PER_SCANLINE;

            let scanline = mmu.read_byte(CURRENT_SCANLINE_ADDR);
            mmu.update_scanline();

            if scanline == 144 {
                // Entering VBLANK
                request_interrupt(mmu, Interrupt::V_BLANK);

            } else if scanline > MAX_SCANLINE_VALUE {
                mmu.reset_scanline();
            } else {
                self.draw_scanline(mmu);
            }
        }
    }

    fn get_current_scanline(&self, mmu: &Mmu) -> Byte {
        // This is the current scanline the PPU is operating on
        mmu.read_byte(CURRENT_SCANLINE_ADDR)
    }

    fn update_lcd_status(&mut self, mmu: &mut Mmu) {
        // Update LCD status to ensure we are correctly drawing graphics depending on the
        // state of the hardware

        let scanline = mmu.read_byte(CURRENT_SCANLINE_ADDR);
        let scanline_compare = mmu.read_byte(CURRENT_SCANLINE_COMPARE_ADDR);

        if !self.is_lcd_enabled(mmu) {
            // LCD is disabled, this means we are in VBlank, so reset scanline
            self.scanline_counter = CYCLES_PER_SCANLINE;
            mmu.reset_scanline();

            // Set VBlank mode to LCD Status
            self.set_lcd_mode(mmu, LcdMode::V_BLANK);

            mmu.open_oam_access();
            mmu.open_vram_access();

            return
        }

        let mut should_request_stat_interrupt = false;
        let current_mode = self.get_lcd_mode(mmu);

        let max_cycles_per_frame = MAX_CYCLES_PER_FRAME as isize;

        // If LCD is enabled, we should cycle through different LCD modes depending on what
        // "dot" we are drawing in the current scanline. We have 456 cycles per scanline
        // for scanlines 0-143. This is broken down as follows:
        //   Length 80 Dots - Mode 2 - Sprite (OAM) Scan
        //   Length 168 - 291 dots (depending on sprite count) - Mode 3 - LCD Transfer (use 172 for now)
        //   Length 85 - 208 Dots (depending on previous length) - Mode 0 - HBlank (use 204 for now)
        // If we are operating on a scanline greater than the visible screen (i.e. scanline >= 144)
        // We are in VBlank and should set LCD status to that mode
        if scanline >= 144 {
            self.set_lcd_mode(mmu, LcdMode::V_BLANK);

            mmu.open_oam_access();
            mmu.open_vram_access();

            should_request_stat_interrupt = self.is_vblank_stat_interrupt_enabled(mmu);
        } else {
            if self.scanline_counter >= max_cycles_per_frame - 80 {
                // This is Mode 2
                self.set_lcd_mode(mmu, LcdMode::SPRITE_SEARCH);

                // Restrict OAM access for Mode 2
                mmu.restrict_oam_access();
                mmu.open_vram_access();

                should_request_stat_interrupt = self.is_oam_stat_interrupt_enabled(mmu);

            } else if self.scanline_counter >= max_cycles_per_frame - 80 - 172 {
                // This is Mode 3
                self.set_lcd_mode(mmu, LcdMode::LCD_TRANSFER);

                // Restrict OAM and VRAM access for Mode 3
                mmu.restrict_oam_access();
                mmu.restrict_vram_access();

            } else {
                // THis is Mode 0
                self.set_lcd_mode(mmu, LcdMode::H_BLANK);

                mmu.open_oam_access();
                mmu.open_vram_access();

                should_request_stat_interrupt = self.is_hblank_stat_interrupt_enabled(mmu);

            }
        }

        // IF we changed mode and should interrupt, do it
        if current_mode != self.get_lcd_mode(mmu) && should_request_stat_interrupt {
            request_interrupt(mmu, Interrupt::LCD_STAT);
        }

        // If current scanline (LY) is equal to value to compare to (LYC)
        // Then set the coincidence flag (bit 2) of LCD status and request
        // An STAT interrupt
        if scanline == scanline_compare {
            self.update_coincidence_flag(mmu, true);
            request_interrupt(mmu, Interrupt::LCD_STAT);
        } else {
            self.update_coincidence_flag(mmu, false);
        }

    }

    fn draw_scanline(&mut self, mmu: &Mmu) {
        // Draw a specific scanline to the display
        if self.is_background_enabled(mmu) {
            self.render_background(mmu)
        }

        if self.is_sprites_enabled(mmu) {
            self.render_sprites(mmu)
        }
    }

    fn is_lcd_enabled(&mut self, mmu: &Mmu) -> bool {
        // Bit 7 of LCD Control specifies if it is enabled or not
        is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 7)
    }

    fn set_lcd_mode(&mut self, mmu: &mut Mmu, mode: LcdMode) {
        // Set the current LCD mode into the Status register
        let mut current_status = mmu.read_byte(LCD_STATUS_ADDR);
        current_status |= mode as u8;
        mmu.write_byte(LCD_STATUS_ADDR, current_status);
    }

    fn get_lcd_mode(&mut self, mmu: &mut Mmu) -> LcdMode {
        // Returns the current LCD mode from the Status register

        let msb = get_bit_val(&mmu.read_byte(LCD_STATUS_ADDR), 1);
        let lsb = get_bit_val(&mmu.read_byte(LCD_STATUS_ADDR), 0);
        let lcd_mode = msb << 1 | lsb;

        match lcd_mode {
            0 => LcdMode::H_BLANK,
            1 => LcdMode::V_BLANK,
            2 => LcdMode::SPRITE_SEARCH,
            3 => LcdMode::LCD_TRANSFER,
            _ => panic!("Invalid LCD Mode - {}", lcd_mode)
        }
    }

    fn is_vblank_stat_interrupt_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return whether or not a STAT interrupt should occur during VBlank
        // Specified by Bit 4 of Status register
        is_bit_set(&mmu.read_byte(LCD_STATUS_ADDR), 4)
    }

    fn is_oam_stat_interrupt_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return whether or not a STAT interrupt should occur during OAM Search
        // Specified by Bit 5 of Status register
        is_bit_set(&mmu.read_byte(LCD_STATUS_ADDR), 5)
    }

    fn is_hblank_stat_interrupt_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return whether or not a STAT interrupt should occur during HBlank
        // Specified by Bit 5 of Status register
        is_bit_set(&mmu.read_byte(LCD_STATUS_ADDR), 3)
    }

    fn update_coincidence_flag(&mut self, mmu: &mut Mmu, val: bool) {
        // Update the coincidence flag (Bit 2) of Status register based on value
        let mut status = mmu.read_byte(LCD_STATUS_ADDR);
        if val {
            set_bit(&mut status, 2);
        } else {
            reset_bit(&mut status, 2);
        }

        // self.set_status(status)
        mmu.write_byte(LCD_STATUS_ADDR, status);
    }

    fn is_background_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return True if the Background is currently enabled and able to be drawn
        // Read from Bit 0 of LCD Control
        is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 0)
    }

    fn is_sprites_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return True if the Sprites are currently enabled and should be drawn
        // Read from Bit 1 of the LCD Control
        is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 1)
    }

    fn is_window_enabled(&mut self, mmu: &Mmu) -> bool {
        // Return True if the Window is currently enabled and able to be drawn
        // Read from Bit 5 of LCD Control
        is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 5)
    }

    fn should_draw_window(&mut self, mmu: &Mmu) -> bool {
        // We should draw the Window instead of the background under a few conditions:
        // 1. If the Window is enabled in Bit 5 of the LCD COntrol register
        // 2. If the Window top-left position (i.e. WY) is above the current scanline
        //     - This would mean that we are currently drawing somewhere the Window is positioned
        self.is_window_enabled(mmu) && self.get_window_position_y(mmu) <= self.get_current_scanline(mmu)
    }

    fn get_background_scroll_x(&mut self, mmu: &Mmu) -> Byte {
        // Get the X Scroll position of the background
        mmu.read_byte(BACKGROUND_SCROLL_X)
    }

    fn get_background_scroll_y(&mut self, mmu: &Mmu) -> Byte {
        // Get the X Scroll position of the background
        mmu.read_byte(BACKGROUND_SCROLL_Y)
    }

    fn get_window_position_x(&mut self, mmu: &Mmu) -> Byte {
        // Get the X position of the Window
        // Remember the value in the WX register is offset by 7
        mmu.read_byte(WINDOW_POS_X).wrapping_sub(7)
    }

    fn get_window_position_y(&mut self, mmu: &Mmu) -> Byte {
        // Get the Y position of the Window
        mmu.read_byte(WINDOW_POS_Y)
    }

    fn get_background_tile_map_area(&mut self, mmu: &Mmu) -> Word {
        // Gets the starting address of the current background tile map
        match is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 3) {
            true => 0x9C00,
            false => 0x9800
        }
    }

    fn get_window_tile_map_area(&mut self, mmu: &Mmu) -> Word {
        // Gets the starting address of the current window tile map
        match is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 6) {
            true => 0x9C00,
            false => 0x9800
        }
    }

    fn get_background_tile_data_area(&mut self, mmu: &Mmu) -> Word {
        // Get the start address for the background/window tiles
        match is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 4) {
            true => 0x8000,
            false => 0x9000
        }
    }

    fn get_sprite_height(&mut self, mmu: &Mmu) -> u8 {
        match is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 2) {
            true => 16,
            false => 8
        }
    }

    fn get_sprite_tile_data_area(&mut self, mmu: &Mmu) -> Word {
        // Get the start address for the sprite tiles
        0x8000
    }

    fn is_background_tile_data_addressing_signed(&mut self, mmu: &Mmu) -> bool {
        // Depending on addressing mode for backgroudn tiles, determine if the identification number
        // for tiles is signed or unsigned. If we are addressing in mode 1 (starting at 0x9000) it should
        // be signed, which will allow us to look back to address 0x8800

        return !is_bit_set(&mmu.read_byte(LCD_CONTROL_ADDR), 4)
    }

    fn render_background(&mut self, mmu: &Mmu) {
        let current_scanline = self.get_current_scanline(mmu);

        // Y Position for scroll is based on if we are drawing window at this scanline
        // or not
        let y_pos = match self.should_draw_window(mmu) {
            true => current_scanline.wrapping_sub(self.get_window_position_y(mmu)),
            false => self.get_background_scroll_y(mmu).wrapping_add(current_scanline)
        };

        let pixels = self.get_background_tile_pixels(mmu, y_pos);
        let mut i = 0;
        for pixel in pixels {
            if (current_scanline as u32) < SCREEN_HEIGHT && current_scanline > 0 {
                // self.screen[i][current_scanline] = pixel
                let base = ((current_scanline as u32) * 3 * SCREEN_WIDTH + i * 3) as usize;
                if base + 2 < self.screen.len() {
                    self.screen[base] = pixel.0;
                    self.screen[base + 1] = pixel.1;
                    self.screen[base + 2] = pixel.2;
                }
                i += 1;
            }
        }
    }

    fn render_sprites(&mut self, mmu: &Mmu) {
        // Sprite data will be copied into OAM and there are 40 sprites in
        // total. We need to look at them all to get there data (i.e. position)
        // and then look up the tiles to draw from there

        let oam_addr = 0xFE00;
        let current_scanline = self.get_current_scanline(mmu);

        for i in 0..40 {
            // Each sprite occupies 4 bytes in OAM, This info is taken from pan docs
            // Byte 0 = Y Position + 16
            // Byte 1 = X Position + 8
            // Byte 2 = Tile Index in Tile memory (i.e. 0x8000 + x)
            // Byte 3 = Sprite Attributes
            let start_addr = oam_addr + (i * 4);
            let y_position = mmu.read_byte(start_addr).wrapping_sub(16);
            let x_position = mmu.read_byte(start_addr + 1).wrapping_sub(8);
            let tile_idx = mmu.read_byte(start_addr + 2);
            let attributes = mmu.read_byte(start_addr + 3);

            let sprite_height = self.get_sprite_height(mmu);

            if current_scanline >= y_position && current_scanline < y_position + sprite_height {

                // Get the current line of sprite
                let mut line = (current_scanline - y_position) as SignedWord;

                // Remember each tile (sprite or background) has two bytes of memory
                // So do this to get the appropriate address
                line *= 2;

                // Recall each tile occupies 16 bytes, and so
                // each line in the sprite is 2 bytes long
                let tile_line_addr = self.get_sprite_tile_data_area(mmu)
                    .wrapping_add((tile_idx as Word) * 16)
                    .wrapping_add(line as Word);

                let lo = mmu.read_byte(tile_line_addr);
                let hi = mmu.read_byte(tile_line_addr + 1);

                for j in (0..8).rev() {
                    let color = self.get_color(mmu, lo, hi, j);

                    // Sprites have "white" as transparent instead of "white", so skip
                    // this pixel
                    if color.0 == 0xFF && color.1 == 0xFF && color.2 == 0xFF {
                        continue;
                    }

                    let pixel_x = 7 - j + x_position;

                    if current_scanline < 0 || (current_scanline as u32) >= SCREEN_HEIGHT || pixel_x < 0 || (pixel_x as u32) >= SCREEN_WIDTH {
                        // If we are outside the visible screen do not set data in the screen data as it will error
                        continue
                    }

                    // Sprite is only hidden under the background for colors 1 - 3 (so not white)
                    if is_bit_set(&attributes, 7) && self.is_pixel_white(pixel_x, current_scanline) {
                        continue
                    }

                    let base = ((current_scanline as u32) * 3 * SCREEN_WIDTH + (pixel_x as u32) * 3) as usize;
                    if base + 2 < self.screen.len() {
                        self.screen[base] = color.0;
                        self.screen[base + 1] = color.1;
                        self.screen[base + 2] = color.2;
                    }
                }
            }
        }
    }

    fn get_background_tile_pixels(&mut self, mmu: &Mmu, y: Byte) -> [(Byte, Byte, Byte); SCREEN_WIDTH as usize] {
        let mut pixels = [(0, 0, 0); SCREEN_WIDTH as usize];

        for i in 0..(SCREEN_WIDTH as isize) {
            let mut x = self.get_background_scroll_x(mmu) as isize + i;
            let tile_map_addr = match self.should_draw_window(mmu) {
                true => self.get_window_tile_map_area(mmu),
                false => self.get_background_tile_map_area(mmu),
            };

            // If we should draw the window and this pixel is within the range of the window,
            // then adjust the offset accordingly with the window X position
            let window_position_x = self.get_window_position_x(mmu) as isize;
            if self.should_draw_window(mmu) && i >= window_position_x {
                x = i - window_position_x;
            }

            let x_offset = x / 8;
            let y_offset = (y as isize / 8) * 32;

            let tile_identifier = mmu.read_byte(tile_map_addr + (x_offset as Word) + (y_offset as Word));
            let is_tile_identifier_signed = self.is_background_tile_data_addressing_signed(mmu);

            // Recall each tile occupies 16 bytes of memory so ensure we account fo 16 total
            // bytes when finding the right y position.
            let tile_data_addr = self.get_background_tile_data_area(mmu);
            let addr = match is_tile_identifier_signed {
                true => {
                    let signed_identifier = tile_identifier as SignedByte;
                    tile_data_addr - ((signed_identifier.abs() as Word) * 16)
                },
                false => tile_data_addr + ((tile_identifier as Word) * 16)
            };

            let line_offset = (y % 8) * 2;
            let pixel_offfset = (7 - x).rem_euclid(8);
            let tile_data_low = mmu.read_byte(addr + line_offset as Word);
            let tile_data_high = mmu.read_byte(addr + (line_offset as Word) + 1);

            let color = self.get_color(mmu, tile_data_low, tile_data_high, pixel_offfset as u8);
            pixels[i as usize] = color;
        }

        pixels
    }

    fn get_color(&mut self, mmu: &Mmu, tile_data_low: Byte, tile_data_high: Byte, bit: u8) -> (Byte, Byte, Byte) {
        let least_significant_bit = get_bit_val(&tile_data_low, bit);
        let most_significant_bit = get_bit_val(&tile_data_high, bit);
        let color_code = (most_significant_bit << 1) | least_significant_bit;

        // this register is where the color pallette is
        // TODO changes for sprites
        let pallette = mmu.read_byte(COLOR_PALLETTE_ADDR);

        // The pallette bits define colors as such (using color ID from 0 - 1)
        // Bit 7-6 - Color for index 3
        // Bit 5-4 - Color for index 2
        // Bit 3-2 - Color for index 1
        // Bit 1-0 - Color for index 0

        let color = match color_code {
            3 => get_bit_val(&pallette, 7) << 1 | get_bit_val(&pallette, 6),
            2 => get_bit_val(&pallette, 5) << 1 | get_bit_val(&pallette, 4),
            1 => get_bit_val(&pallette, 3) << 1 | get_bit_val(&pallette, 2),
            0 => get_bit_val(&pallette, 1) << 1 | get_bit_val(&pallette, 0),
            _ => panic!("Invalid color code - {}", color_code)
        };

        *GB_COLORS
            .get(&color)
            .expect(&format!("Color {} is not recognized", color))
    }

    fn is_pixel_white(&self, x: u8, y: u8) -> bool {
        let base = ((y as u32) * 3 * SCREEN_WIDTH + (x as u32) * 3) as usize;
        let pixel = (self.screen[base], self.screen[base + 1], self.screen[base + 2]);
        pixel.0 == 0xFF && pixel.1 == 0xFF && pixel.2 == 0xFF
    }
}