// =============================================================================
// entities/flake.rs — Food flakes (dropped by the user)
// =============================================================================
//
// When the user clicks in the terminal, a food flake spawns at that position
// and drifts downward.  Fish detect nearby flakes and steer toward them.
// The flake deactivates when it reaches the sea floor (SEA_LEVEL_Y).
//
// DRIFT FORMULA:
//   y += fall_speed * dt
//   x += sin(time * 1.2 + index_as_float) * 1.5 * dt
//
// Using the flake's *pool index* in the sine formula means each flake drifts
// with a unique phase — they don't all sway in unison.
//
// RUST CONCEPT: `usize` FOR INDICES
// Rust array/Vec indices are always `usize` — a pointer-sized integer
// (64-bit on 64-bit systems).  We pass `index: usize` from the pool so we
// can compute a stable per-flake phase without storing an extra field.
// =============================================================================

use crate::renderer::Renderer;
use crate::vec2::Vec2;
use crossterm::style::Color;

pub struct Flake {
    pub pos:        Vec2,
    pub fall_speed: f32,
    pub active:     bool,
}

impl Flake {
    // Create a new flake at position `pos` (terminal cell coordinates).
    pub fn new(pos: Vec2) -> Self {
        Self {
            pos,
            fall_speed: 2.5, // cells per second downward
            active: true,
        }
    }

    // -------------------------------------------------------------------------
    // update — advance flake physics for one frame.
    //
    // `index: usize` — the flake's position in the pool Vec.
    //   We use it as a phase seed: `index as f32` converts usize → f32.
    //   Casting with `as` in Rust is explicit and never panics.
    //   This is a zero-cost way to give each flake a unique drift pattern.
    //
    // `sea_level: f32` — the y coordinate of the sea floor.
    //   When the flake reaches or passes it, we set active = false.
    //   The pool slot can then be reused for the next click.
    // -------------------------------------------------------------------------
    pub fn update(&mut self, dt: f32, time: f32, index: usize, sea_level: f32) {
        self.pos.y += self.fall_speed * dt;

        // Lateral drift: unique sine phase per flake via pool index.
        self.pos.x += (time * 1.2 + index as f32).sin() * 1.5 * dt;

        if self.pos.y >= sea_level {
            self.active = false;
        }
    }

    pub fn draw(&self, renderer: &mut Renderer) {
        if !self.active || self.pos.x < 0.0 || self.pos.y < 0.0 {
            return;
        }
        renderer.put(
            self.pos.x as u16,
            self.pos.y as u16,
            '*',
            Color::Yellow,
            Color::Reset,
        );
    }
}
