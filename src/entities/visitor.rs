// =============================================================================
// entities/visitor.rs — Octopus and Seahorse visitors
// =============================================================================
//
// RUST CONCEPT: ENUM WITH DATA (Algebraic Data Types)
// ────────────────────────────────────────────────────
// The original C++ uses two separate structs (Octopus, Seahorse) each with an
// `active` boolean.  In Rust we model this as a single enum:
//
//   enum Visitor {
//       None,
//       Octopus(OctopusState),
//       Seahorse(SeahorseState),
//   }
//
// This is an *algebraic data type* (ADT) / *sum type*.  The enum can be in
// exactly one state at a time, and each variant carries exactly the data it
// needs.  When there's no visitor, `None` carries no data at all — no wasted
// memory for inactive fields.
//
// `match` EXHAUSTIVENESS
// ───────────────────────
// Whenever you `match` on a Visitor, the compiler forces you to handle all
// three variants.  Forgetting one is a COMPILE ERROR.  Compare this to a C++
// switch on an `int` where you can silently fall through.
//
// SPAWN LOGIC
// ───────────
// Visitors spawn from the left or right edge at a random vertical position,
// cross the screen, and despawn when they exit the opposite edge.  A timer
// controls how long before the next visitor appears.
//
// Fish are repelled from visitors (see fish.rs apply_avoidance).
// =============================================================================

use crate::renderer::Renderer;
use crate::vec2::Vec2;
use crossterm::style::Color;
use rand::Rng;

// =============================================================================
// OctopusState — data for an active octopus
// =============================================================================

pub struct OctopusState {
    pub pos:         Vec2,
    pub base_y:      f32,     // y anchor for vertical bobbing
    pub vx:          f32,     // horizontal velocity (positive = moving right)
    pub phase:       f32,     // bobbing phase offset
    pub color_phase: f32,     // phase for color cycling
}

impl OctopusState {
    // Move and animate.  Returns false when the octopus has left the screen.
    pub fn update(&mut self, dt: f32, time: f32, width: f32) -> bool {
        self.pos.x += self.vx * dt;

        // Vertical bob: oscillate around base_y.
        self.pos.y = self.base_y + (time * 1.3 + self.phase).sin() * 1.2;

        // Despawn check: has it crossed the opposite edge?
        if self.vx > 0.0 && self.pos.x > width + 4.0 {
            return false;
        }
        if self.vx < 0.0 && self.pos.x < -6.0 {
            return false;
        }
        true // still on screen
    }

    pub fn draw(&self, renderer: &mut Renderer, time: f32) {
        if self.pos.x < 0.0 || self.pos.y < 0.0 {
            return;
        }
        // Color cycles slowly through magenta shades.
        let hue = ((time * 0.5 + self.color_phase).sin() * 0.5 + 0.5) * 5.0;
        let color = match hue as u8 {
            0 => Color::Magenta,
            1 => Color::AnsiValue(201),
            2 => Color::AnsiValue(165),
            3 => Color::AnsiValue(213),
            _ => Color::AnsiValue(219),
        };

        // Draw body '@' with tentacle trails '~~'.
        let x = self.pos.x as u16;
        let y = self.pos.y as u16;

        if self.vx >= 0.0 {
            renderer.put_str(x, y, "~~@", color, Color::Reset);
        } else {
            renderer.put_str(x, y, "@~~", color, Color::Reset);
        }

        // Tentacles one row below (simplified to three dots).
        renderer.put_str(x, y + 1, "|||", color, Color::Reset);
    }

    // Return the position for fish avoidance calculations.
    pub fn threat_pos(&self) -> Vec2 {
        self.pos
    }
}

// =============================================================================
// SeahorseState — data for an active seahorse
// =============================================================================

pub struct SeahorseState {
    pub pos:         Vec2,
    pub base_y:      f32,
    pub vx:          f32,
    pub phase:       f32,
    pub fin_phase:   f32,  // separate phase for the dorsal fin animation
    pub facing_right: bool,
}

impl SeahorseState {
    pub fn update(&mut self, dt: f32, time: f32, width: f32) -> bool {
        self.pos.x += self.vx * dt;
        // Seahorse bobs more gently than octopus.
        self.pos.y = self.base_y + (time * 0.9 + self.phase).sin() * 0.8;

        if self.vx > 0.0 && self.pos.x > width + 3.0 {
            return false;
        }
        if self.vx < 0.0 && self.pos.x < -4.0 {
            return false;
        }
        true
    }

    pub fn draw(&self, renderer: &mut Renderer, time: f32) {
        if self.pos.x < 0.0 || self.pos.y < 0.0 {
            return;
        }
        // Fin pulses between two states.
        let fin_open = ((time * 4.0 + self.fin_phase).sin()) > 0.0;

        let x = self.pos.x as u16;
        let y = self.pos.y as u16;

        // Two-row sprite: head + body.
        let (head, body) = if self.facing_right {
            if fin_open { ("(§>", " | ") } else { ("(§>", " | ") }
        } else {
            if fin_open { ("<§)", " | ") } else { ("<§)", " | ") }
        };

        renderer.put_str(x, y,     head, Color::AnsiValue(214), Color::Reset);
        renderer.put_str(x, y + 1, body, Color::AnsiValue(220), Color::Reset);
    }

    pub fn threat_pos(&self) -> Vec2 {
        self.pos
    }
}

// =============================================================================
// Visitor — the top-level enum
// =============================================================================
//
// RUST CONCEPT: ENUM WITH VARIANT DATA
// Each variant wraps a completely different data type.  The enum is the size
// of its largest variant (plus a discriminant byte to know which one is active).
// The None variant holds no data at all — it's just a tag.
// =============================================================================

pub enum Visitor {
    None,
    Octopus(OctopusState),
    Seahorse(SeahorseState),
}

impl Visitor {
    // Check if there's currently no visitor on screen.
    pub fn is_none(&self) -> bool {
        // `matches!` is a macro that returns true if the value matches the pattern.
        matches!(self, Visitor::None)
    }

    // -------------------------------------------------------------------------
    // Spawn a new visitor — either octopus or seahorse, entering from a random
    // edge of the screen.
    //
    // `width` and `height` are the terminal dimensions.
    // `sea_level` caps the vertical spawn range to the water column.
    // -------------------------------------------------------------------------
    pub fn spawn(rng: &mut impl Rng, width: f32, _height: f32, sea_level: f32) -> Self {
        // Coin flip: octopus or seahorse?
        let is_octopus: bool = rng.gen_bool(0.5);

        // Enter from left or right edge?
        let from_left: bool = rng.gen_bool(0.5);
        let (start_x, vx) = if from_left {
            (-4.0, rng.gen_range(3.0_f32..6.0)) // enter left, move right
        } else {
            (width + 4.0, -rng.gen_range(3.0_f32..6.0)) // enter right, move left
        };

        let start_y: f32 = rng.gen_range(2.0..(sea_level - 3.0));

        if is_octopus {
            Visitor::Octopus(OctopusState {
                pos:         Vec2::new(start_x, start_y),
                base_y:      start_y,
                vx,
                phase:       rng.gen_range(0.0_f32..std::f32::consts::TAU),
                color_phase: rng.gen_range(0.0_f32..std::f32::consts::TAU),
            })
        } else {
            Visitor::Seahorse(SeahorseState {
                pos:          Vec2::new(start_x, start_y),
                base_y:       start_y,
                vx,
                phase:        rng.gen_range(0.0_f32..std::f32::consts::TAU),
                fin_phase:    rng.gen_range(0.0_f32..std::f32::consts::TAU),
                facing_right: vx > 0.0,
            })
        }
    }

    // -------------------------------------------------------------------------
    // update — advance visitor physics.
    //
    // `match self { ... }` is the idiomatic way to handle enum variants.
    // Each arm destructures the variant to get the inner state.
    //
    // We update in-place: if the visitor exits the screen, we replace it with
    // Visitor::None.
    // -------------------------------------------------------------------------
    pub fn update(&mut self, dt: f32, time: f32, width: f32) {
        // `*self = ...` replaces the value pointed to by the mutable reference.
        let still_alive = match self {
            Visitor::None             => return, // nothing to do
            Visitor::Octopus(state)   => state.update(dt, time, width),
            Visitor::Seahorse(state)  => state.update(dt, time, width),
        };
        if !still_alive {
            *self = Visitor::None;
        }
    }

    pub fn draw(&self, renderer: &mut Renderer, time: f32) {
        match self {
            Visitor::None            => {}
            Visitor::Octopus(state)  => state.draw(renderer, time),
            Visitor::Seahorse(state) => state.draw(renderer, time),
        }
    }

    // Return the threat position for fish avoidance — or None if no visitor.
    // `Option<Vec2>` is Rust's null-safe nullable type.
    pub fn threat_pos(&self) -> Option<Vec2> {
        match self {
            Visitor::None            => None,
            Visitor::Octopus(state)  => Some(state.threat_pos()),
            Visitor::Seahorse(state) => Some(state.threat_pos()),
        }
    }
}
