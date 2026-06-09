// =============================================================================
// entities/bubble.rs — Rising bubbles
// =============================================================================
//
// Bubbles start near the sea floor, rise upward with a gentle sine-wave sway,
// and wrap back to the bottom when they reach the top.
//
// PHYSICS (translated directly from the C++ .ino):
//   y      -= rise_speed * dt                          (move up)
//   x       = base_x + sin(time * 1.8 + phase) * sway  (oscillate sideways)
//
// The `base_x` anchor prevents the bubble from drifting indefinitely; it
// always oscillates around its starting column.
//
// GLYPH SELECTION:
//   Small bubbles  → '·'  (U+00B7 MIDDLE DOT)
//   Medium bubbles → 'o'
//   Large bubbles  → 'O'
// =============================================================================

use crate::renderer::Renderer;
use crate::vec2::Vec2;
use crossterm::style::Color;
use rand::Rng;

pub struct Bubble {
    pub pos:        Vec2,   // current position (f32 for sub-cell precision)
    pub base_x:     f32,    // x anchor for the sway oscillation
    pub rise_speed: f32,    // cells per second upward
    pub phase:      f32,    // individual phase offset (radians) for sway
    pub sway_amp:   f32,    // horizontal sway amplitude in cells
    pub size:       BubbleSize,
    pub active:     bool,
}

// An enum to describe the three bubble sizes — safer than magic numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BubbleSize {
    Small,
    Medium,
    Large,
}

impl BubbleSize {
    pub fn glyph(self) -> char {
        match self {
            BubbleSize::Small  => '·',
            BubbleSize::Medium => 'o',
            BubbleSize::Large  => 'O',
        }
    }

    pub fn color(self) -> Color {
        // Bubbles are shades of cyan / light-blue.
        match self {
            BubbleSize::Small  => Color::AnsiValue(159), // light cyan
            BubbleSize::Medium => Color::Cyan,
            BubbleSize::Large  => Color::AnsiValue(51),  // bright cyan
        }
    }
}

impl Bubble {
    pub fn new(pos: Vec2, rng: &mut impl Rng) -> Self {
        // Random size: 60% small, 30% medium, 10% large.
        let n: f32 = rng.gen();
        let size = if n < 0.6 {
            BubbleSize::Small
        } else if n < 0.9 {
            BubbleSize::Medium
        } else {
            BubbleSize::Large
        };

        // Rise speed scales with size.
        let rise_speed = match size {
            BubbleSize::Small  => rng.gen_range(1.0_f32..2.5),
            BubbleSize::Medium => rng.gen_range(2.0_f32..3.5),
            BubbleSize::Large  => rng.gen_range(3.0_f32..5.0),
        };

        Self {
            base_x: pos.x,
            pos,
            rise_speed,
            phase:    rng.gen_range(0.0_f32..std::f32::consts::TAU),
            sway_amp: rng.gen_range(0.3_f32..1.8),
            size,
            active: true,
        }
    }

    // -------------------------------------------------------------------------
    // update — advance the bubble by dt seconds.
    //
    // PATTERN: all update functions in this project take `time` (the total
    // elapsed aquarium time in seconds).  Using absolute time in sine/cosine
    // avoids accumulation errors you'd get from integrating per-frame angles.
    // -------------------------------------------------------------------------
    pub fn update(&mut self, dt: f32, time: f32, height: f32) {
        // Rise upward.
        self.pos.y -= self.rise_speed * dt;

        // Sway left and right around base_x.
        // sin() returns values in [-1, 1]; multiplying by sway_amp scales
        // that range to [-sway_amp, +sway_amp] cells.
        self.pos.x = self.base_x + (time * 1.8 + self.phase).sin() * self.sway_amp;

        // Wrap: when the bubble exits the top, reset it to the bottom.
        if self.pos.y < 0.0 {
            self.pos.y = height;
        }
    }

    pub fn draw(&self, renderer: &mut Renderer) {
        if !self.active || self.pos.x < 0.0 || self.pos.y < 0.0 {
            return;
        }
        renderer.put(
            self.pos.x as u16,
            self.pos.y as u16,
            self.size.glyph(),
            self.size.color(),
            Color::Reset,
        );
    }
}
