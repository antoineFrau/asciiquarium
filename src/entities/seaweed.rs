// =============================================================================
// entities/seaweed.rs — Animated seaweed
// =============================================================================
//
// Each seaweed blade is a vertical column of characters that sways left and
// right via a sine wave.  The sway is computed per-segment so the top sways
// more than the base (like real seaweed anchored to the floor).
//
// WAVE FORMULA (from the original .ino):
//   x_offset = sin(time * wave_speed - segment_u * 5.1 + phase) * amplitude
//
// where `segment_u` goes from 0.0 (root) to 1.0 (tip).
// The tip (`segment_u = 1.0`) experiences the full `-1 * 5.1` phase shift, so
// it is always several radians ahead of the root in the wave cycle.  That's
// what produces the bending shape.
//
// GLYPH SELECTION
// We choose the character based on the *direction* the offset is moving:
//   positive and moving right → ')'
//   negative and moving left  → '('
//   near zero                 → '|'  or '/' or '\'
//
// In practice we simplify: just alternate | ( ) based on the sign of the
// wave value at each segment.
// =============================================================================

use crate::renderer::Renderer;
use crossterm::style::Color;
use rand::Rng;

// Green shades for seaweed.  AnsiValue palette:
//   22 = dark green, 28 = medium green, 34 = bright green, 40 = lime
const SEAWEED_COLORS: &[Color] = &[
    Color::AnsiValue(22),
    Color::AnsiValue(28),
    Color::AnsiValue(34),
    Color::AnsiValue(40),
    Color::AnsiValue(64),
    Color::AnsiValue(70),
];

pub struct Seaweed {
    pub base_x:     f32,   // column in terminal cells (the "root" x)
    pub base_y:     f32,   // row of the sea floor (bottom of this blade)
    pub height:     u16,   // number of segments (rows) this blade spans
    pub phase:      f32,   // per-blade random phase offset
    pub wave_speed: f32,   // how fast this blade sways
    pub amplitude:  f32,   // max horizontal sway in cells
    pub color:      Color,
}

impl Seaweed {
    pub fn new(base_x: f32, base_y: f32, rng: &mut impl Rng) -> Self {
        Self {
            base_x,
            base_y,
            height:     rng.gen_range(3_u16..8),
            phase:      rng.gen_range(0.0_f32..std::f32::consts::TAU),
            wave_speed: rng.gen_range(1.0_f32..2.5),
            amplitude:  rng.gen_range(0.4_f32..1.2),
            color:      SEAWEED_COLORS[rng.gen_range(0..SEAWEED_COLORS.len())],
        }
    }

    // -------------------------------------------------------------------------
    // draw — compute and render all segments this frame.
    //
    // We don't store segment positions; we recompute them each frame from the
    // wave function.  This avoids allocating a Vec every update cycle.
    //
    // RUST CONCEPT: RANGES
    //   `0..self.height` is a Range<u16>, which implements Iterator.
    //   We iterate with a for loop; each iteration gives us segment index `s`.
    // -------------------------------------------------------------------------
    pub fn draw(&self, renderer: &mut Renderer, time: f32) {
        for s in 0..self.height {
            // `segment_u` is the normalised position along the blade [0, 1].
            // Root (s == height-1) → u near 0.  Tip (s == 0) → u near 1.
            let segment_u = 1.0 - (s as f32 / self.height as f32);

            // Wave formula: x offset for this segment at this time.
            let wave_val = (time * self.wave_speed
                - segment_u * 5.1
                + self.phase)
                .sin()
                * self.amplitude;

            // Terminal column: base_x + rounded wave offset.
            let x = (self.base_x + wave_val).round() as i32;
            // Terminal row: base_y - s (segment 0 is at the top of the blade).
            let y = (self.base_y - s as f32).round() as i32;

            if x < 0 || y < 0 {
                continue;
            }

            // Choose glyph based on wave value:
            //   strong left  → '('
            //   strong right → ')'
            //   near centre  → '|'
            let glyph = if wave_val < -0.3 {
                '('
            } else if wave_val > 0.3 {
                ')'
            } else {
                '|'
            };

            renderer.put(x as u16, y as u16, glyph, self.color, Color::Reset);
        }
    }
}
