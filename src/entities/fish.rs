// =============================================================================
// entities/fish.rs — Fish struct, AI behaviours, rendering
// =============================================================================
//
// This is the most complex file in the project.  It implements three layers:
//
//   1. DATA  — the Fish struct and FishType enum
//   2. AI    — wander, school, feed-seek, avoid (the fun part)
//   3. DRAW  — which ASCII glyph to use, facing direction
//
// RUST CONCEPT: ENUMS AS SUM TYPES
// ─────────────────────────────────
// The original C++ code uses `int type` with magic numbers (0, 1, 2, 3).
// Rust enums are far safer:
//
//   enum FishType { Small, Medium, Large, Exotic }
//
//   let t = FishType::Small;
//   match t {
//       FishType::Small  => { /* ... */ }
//       FishType::Medium => { /* ... */ }
//       FishType::Large  => { /* ... */ }
//       FishType::Exotic => { /* ... */ }
//       // Forgetting a variant is a COMPILE ERROR — no silent fallthrough.
//   }
//
// RUST CONCEPT: THE BORROW-CHECKER CHALLENGE WITH SCHOOLING
// ──────────────────────────────────────────────────────────
// The schooling algorithm needs each fish to *read* all other fish while
// *writing* its own velocity.  In C++ this is fine.  In Rust:
//
//   for fish in pool.iter_mut() {          // mutable borrow of pool
//       for other in pool.iter() { ... }   // ERROR: immutable borrow while
//   }                                      //        mutable borrow is active
//
// The compiler rejects this because it cannot prove that `fish` and `other`
// don't alias (point to the same memory).  If they did, writing `fish.vel`
// while reading `other.vel` could produce inconsistent state.
//
// SOLUTION — two-pass pattern (see Aquarium::update in aquarium.rs):
//   Pass 1: iterate immutably over all fish, compute steering forces → Vec<Vec2>
//   Pass 2: iterate mutably, apply the pre-computed forces
//
// This is the borrow checker teaching you a concurrent-programming best
// practice: read phase → barrier → write phase.
// =============================================================================

use crate::renderer::Renderer;
use crate::vec2::Vec2;
use crossterm::style::Color;
use rand::Rng;

// =============================================================================
// FishType — what kind of fish is this?
//
// Each variant gets different glyphs, sizes, speed ranges and colours.
// The `#[derive(Debug, Clone, Copy, PartialEq)]` block auto-generates:
//   Debug    → allows {:?} printing
//   Clone    → .clone()
//   Copy     → cheap bitwise duplication (no heap, so free)
//   PartialEq → == and !=
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FishType {
    Small,  // fast, numerous, tight schooling radius
    Medium, // balanced
    Large,  // slow, wide schooling radius, roam alone
    Exotic, // rare, colourful, individual wanderer
}

impl FishType {
    // Speed range [min, max] in terminal cells per second.
    pub fn speed_range(self) -> (f32, f32) {
        match self {
            FishType::Small  => (6.0, 10.0),
            FishType::Medium => (4.0,  7.0),
            FishType::Large  => (2.5,  4.5),
            FishType::Exotic => (5.0,  9.0),
        }
    }

    // Schooling radius in terminal cells.
    pub fn school_radius(self) -> f32 {
        match self {
            FishType::Small  => 12.0,
            FishType::Medium => 9.0,
            FishType::Large  => 6.0,
            FishType::Exotic => 0.0, // exotic fish don't school
        }
    }

    // How strongly this fish wanders (sine-wave force magnitude).
    pub fn wander_bias(self) -> f32 {
        match self {
            FishType::Small  => 0.6,
            FishType::Medium => 0.4,
            FishType::Large  => 0.25,
            FishType::Exotic => 0.8,
        }
    }

    // All ASCII glyph variants for this fish type.
    // Each entry is (right-facing, left-facing).
    // `&'static [...]` — a reference to a slice living in the binary's
    // read-only data section; zero heap allocation.
    pub fn all_glyphs(self) -> &'static [(&'static str, &'static str)] {
        match self {
            FishType::Small => &[
                ("><>",  "<><"),   // classic
                (">->",  "<-<"),   // slim
                ("><>>", "<<><"),  // wide tail
                (">*>",  "<*<"),   // spotted
            ],
            FishType::Medium => &[
                ("))><",  "<(("),    // classic
                ("><))>", "<((<"),   // round
                (">-))>", "<((-<"),  // slender
                (">O))>", "<))O<"),  // big eye
            ],
            FishType::Large => &[
                ("><))°>",  "<°((<"),   // classic
                ("><)))°>", "<°(((<"),  // longer body
                ("><))=>",  "<=))<"),   // swept tail
                (">~~)°>",  "<°)~~<"),  // wavy fins
            ],
            FishType::Exotic => &[
                (">°><",  "<><°"),   // classic
                (">*><",  "<><*"),   // sparkle
                ("><°°>", "<°°><"),  // twin spots
                ("><~>",  "<~><"),   // wavy
            ],
        }
    }

    // Return one glyph pair by variant index (wraps around).
    pub fn glyphs(self, variant: usize) -> (&'static str, &'static str) {
        let all = self.all_glyphs();
        all[variant % all.len()]
    }

    // Pick a random color appropriate for this fish type.
    pub fn random_color(self, rng: &mut impl Rng) -> Color {
        // Color::AnsiValue(n) uses the 256-color xterm palette.
        // See: https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit
        // Values 1–15: bright basic colors
        // Values 16–231: 6x6x6 color cube
        // Values 232–255: grayscale ramp
        match self {
            FishType::Small => {
                let choices = [Color::Cyan, Color::White, Color::Yellow,
                               Color::AnsiValue(87), Color::AnsiValue(159)];
                choices[rng.gen_range(0..choices.len())]
            }
            FishType::Medium => {
                let choices = [Color::Green, Color::AnsiValue(46),
                               Color::AnsiValue(77), Color::AnsiValue(118)];
                choices[rng.gen_range(0..choices.len())]
            }
            FishType::Large => {
                let choices = [Color::Red, Color::AnsiValue(196),
                               Color::AnsiValue(202), Color::AnsiValue(208)];
                choices[rng.gen_range(0..choices.len())]
            }
            FishType::Exotic => {
                let choices = [Color::Magenta, Color::AnsiValue(201),
                               Color::AnsiValue(165), Color::AnsiValue(213),
                               Color::AnsiValue(219)];
                choices[rng.gen_range(0..choices.len())]
            }
        }
    }
}

// =============================================================================
// Fish — one fish in the tank
// =============================================================================

pub struct Fish {
    pub pos:           Vec2,     // position in terminal cells (f32 for smooth motion)
    pub vel:           Vec2,     // velocity in cells/second
    pub speed:         f32,      // max speed (target magnitude of vel)
    pub phase:         f32,      // random phase offset (radians) for wander sine wave
    pub wander_bias:   f32,      // how strongly this fish wanders
    pub color:         Color,    // ANSI colour
    pub fish_type:     FishType,
    pub glyph_variant: usize,    // which ASCII art shape to use (index into all_glyphs)
    pub active:        bool,     // inactive fish are skipped and can be respawned
}

impl Fish {
    // -------------------------------------------------------------------------
    // Constructor — spawn a new fish at the given position.
    //
    // `rng: &mut impl Rng`
    //   This is a *generic parameter with a trait bound*, written in the
    //   "impl Trait in argument position" shorthand.  It means: accept any
    //   type `R` such that `R: Rng`.  The compiler monomorphises (generates
    //   a specific version for) every concrete type used.
    //   This is zero-cost — no virtual dispatch, no heap allocation.
    // -------------------------------------------------------------------------
    pub fn new(pos: Vec2, fish_type: FishType, rng: &mut impl Rng) -> Self {
        let (speed_min, speed_max) = fish_type.speed_range();
        let speed = rng.gen_range(speed_min..speed_max);

        // Give the fish a random initial direction.
        let angle = rng.gen_range(0.0_f32..std::f32::consts::TAU); // 0..2π
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;

        let variant_count = fish_type.all_glyphs().len();
        Self {
            pos,
            vel,
            speed,
            phase: rng.gen_range(0.0_f32..std::f32::consts::TAU),
            wander_bias: fish_type.wander_bias(),
            color: fish_type.random_color(rng),
            fish_type,
            glyph_variant: rng.gen_range(0..variant_count),
            active: true,
        }
    }

    // -------------------------------------------------------------------------
    // apply_wander — add sine-wave force to velocity.
    //
    // This is the "random walk" behaviour.  Two out-of-phase sine waves on x
    // and y produce a smooth, organic-looking curved path.
    //
    //   vx += cos(time * 0.9 + phase) * wander_bias * dt
    //   vy += sin(time * 0.7 + phase * 1.7) * 0.22 * dt
    //
    // The different multipliers (0.9 / 0.7 / 1.7) make x and y drift at
    // slightly different rates so the path doesn't just trace a circle.
    // -------------------------------------------------------------------------
    pub fn apply_wander(&mut self, dt: f32, time: f32) {
        let wx = (time * 0.9 + self.phase).cos() * self.wander_bias;
        let wy = (time * 0.7 + self.phase * 1.7).sin() * 0.22;
        self.vel.x += wx * dt;
        self.vel.y += wy * dt;
    }

    // -------------------------------------------------------------------------
    // apply_school_force — steer toward the average position and velocity of
    // nearby same-type fish (cohesion + alignment).
    //
    // This method only *receives* a pre-computed force — it doesn't access
    // the global fish pool.  The caller (Aquarium::update) computes the force
    // in a separate read-only pass to avoid borrow-checker conflicts.
    //
    // The force is a Vec2 pre-scaled by dt so this method just adds it.
    // -------------------------------------------------------------------------
    pub fn apply_school_force(&mut self, force: Vec2) {
        self.vel += force;
    }

    // -------------------------------------------------------------------------
    // apply_feed_seek — steer toward the nearest food flake.
    //
    // `Option<Vec2>` — the idiomatic Rust way to express "maybe a value".
    // Instead of passing a null pointer (C++) we pass Some(position) or None.
    // The compiler forces us to handle both cases — no null dereference bugs.
    //
    // SEEK FORCE FORMULA:
    //   desired_velocity = normalize(target - position) * speed
    //   steering         = desired_velocity - current_velocity
    //   apply a fraction of steering each frame (smooth pursuit)
    // -------------------------------------------------------------------------
    pub fn apply_feed_seek(&mut self, dt: f32, nearest_flake: Option<Vec2>) {
        // `if let Some(target) = ...` is pattern matching on Option.
        // If nearest_flake is None, the whole block is skipped — no crash.
        if let Some(target) = nearest_flake {
            let to_target = target - self.pos;
            let dist = to_target.length();

            // Only seek within detection range (14 cells).
            if dist < 14.0 && dist > 0.0 {
                let desired = to_target.normalized() * self.speed * 1.4; // slightly faster when seeking food
                let steering = desired - self.vel;
                // Lerp toward desired direction — smooth pursuit.
                self.vel += steering * (3.0 * dt);
            }
        }
    }

    // -------------------------------------------------------------------------
    // apply_avoidance — repel from a threat position (octopus / seahorse).
    //
    // The repulsion force is inversely proportional to distance: the closer
    // the threat, the stronger the push.
    // -------------------------------------------------------------------------
    pub fn apply_avoidance(&mut self, dt: f32, threat: Option<Vec2>) {
        if let Some(threat_pos) = threat {
            let away = self.pos - threat_pos;
            let dist = away.length();
            if dist < 12.0 && dist > 0.01 {
                // Force magnitude grows as 1/dist (closer = stronger).
                let magnitude = (12.0 - dist) / 12.0 * self.speed * 2.0;
                self.vel += away.normalized() * magnitude * dt;
            }
        }
    }

    // -------------------------------------------------------------------------
    // clamp_speed — normalise velocity to max speed.
    //
    // After all forces are applied the velocity can exceed `speed`.  We
    // rescale it back to exactly `speed` so fish don't accelerate forever.
    // -------------------------------------------------------------------------
    pub fn clamp_speed(&mut self) {
        let len = self.vel.length();
        if len > self.speed {
            self.vel = self.vel * (self.speed / len);
        }
        // Also enforce a minimum speed so fish never stop completely.
        if len < self.speed * 0.3 && len > 0.001 {
            self.vel = self.vel * (self.speed * 0.3 / len);
        }
    }

    // -------------------------------------------------------------------------
    // integrate — move fish by velocity × dt.
    //
    // Wrap horizontally (fish exit left → reenter right and vice versa).
    // Clamp vertically so fish stay in the water column.
    // -------------------------------------------------------------------------
    pub fn integrate(&mut self, dt: f32, width: f32, height: f32) {
        self.pos += self.vel * dt;

        // Horizontal wrap-around.
        let glyph_w = self.glyph().chars().count() as f32;
        if self.pos.x > width {
            self.pos.x = -glyph_w;
        } else if self.pos.x < -glyph_w {
            self.pos.x = width;
        }

        // Vertical clamp: top margin (row 1 = below HUD) and bottom margin
        // (row h-5 = above seaweed).
        let top    = 1.5_f32;
        let bottom = height - 5.0;
        if self.pos.y < top {
            self.pos.y = top;
            self.vel.y = self.vel.y.abs(); // bounce downward
        } else if self.pos.y > bottom {
            self.pos.y = bottom;
            self.vel.y = -self.vel.y.abs(); // bounce upward
        }
    }

    // -------------------------------------------------------------------------
    // glyph — which ASCII string to draw for this fish.
    //
    // Fish face the direction they're swimming.  If vel.x > 0 they face right;
    // otherwise they face left.
    // -------------------------------------------------------------------------
    pub fn glyph(&self) -> &'static str {
        let (right, left) = self.fish_type.glyphs(self.glyph_variant);
        if self.vel.x >= 0.0 { right } else { left }
    }

    // -------------------------------------------------------------------------
    // draw — write the fish glyph into the renderer's back buffer.
    //
    // We convert the f32 position to u16 for terminal coordinates.
    // `as u16` in Rust is a saturating cast (no panic on overflow), but we
    // guard against negative values explicitly.
    // -------------------------------------------------------------------------
    pub fn draw(&self, renderer: &mut Renderer) {
        if !self.active {
            return;
        }
        if self.pos.x < 0.0 || self.pos.y < 0.0 {
            return;
        }
        let x = self.pos.x as u16;
        let y = self.pos.y as u16;
        renderer.put_str(x, y, self.glyph(), self.color, Color::Reset);
    }
}

// =============================================================================
// FREE FUNCTION: compute_school_force
//
// This is a MODULE-LEVEL function, not a method on Fish.  In Rust you can
// freely mix methods and free functions.  This one lives outside `impl Fish`
// because it needs to read *all* fish simultaneously (the two-pass pattern).
//
// Called from Aquarium::update for each fish index.
//
// PARAMETERS:
//   idx   — index of the fish we're computing the force *for*
//   pool  — shared (immutable) borrow of the whole fish pool
//   dt    — delta-time in seconds
//
// RETURNS Vec2 — the schooling + separation force to apply.
// =============================================================================

pub fn compute_school_force(idx: usize, pool: &[Fish], dt: f32) -> Vec2 {
    // `&[Fish]` is a *slice* — a view into part of a Vec (or any contiguous
    // array).  It carries a pointer + length.  We borrow the whole pool but
    // don't own it, so we can't modify it here — perfect for the read pass.

    let me = &pool[idx]; // immutable reference to the fish we're computing for
    if !me.active {
        return Vec2::default();
    }

    let school_r  = me.fish_type.school_radius();
    let school_r2 = school_r * school_r; // compare squared distances to avoid sqrt

    let mut avg_vel = Vec2::default();   // accumulator for alignment
    let mut avg_pos = Vec2::default();   // accumulator for cohesion
    let mut sep_force = Vec2::default(); // separation accumulator
    let mut count = 0usize;

    // Iterate over all *other* fish.
    // `pool.iter().enumerate()` yields (index, &Fish) pairs.
    for (j, other) in pool.iter().enumerate() {
        if j == idx || !other.active {
            continue; // skip self and inactive fish
        }

        let diff  = other.pos - me.pos;
        let dist2 = diff.length_sq();

        // --- Separation (all nearby fish, any type) ---
        // Push away when another fish is within 5 cells — wide enough to
        // prevent fish glyphs from visually overlapping before they repel.
        let sep_r2 = 25.0_f32; // separation radius = 5 cells
        if dist2 < sep_r2 && dist2 > 0.001 {
            let push = -diff.normalized() * (sep_r2.sqrt() - dist2.sqrt()) * 0.8;
            sep_force += push;
        }

        // --- Schooling (same type only, within school radius) ---
        if other.fish_type != me.fish_type {
            continue;
        }
        if dist2 > school_r2 || school_r2 == 0.0 {
            continue;
        }

        avg_vel += other.vel;
        avg_pos += other.pos;
        count   += 1;
    }

    let mut force = Vec2::default();

    if count > 0 {
        let n = count as f32;

        // ALIGNMENT: steer toward average velocity of neighbors.
        avg_vel = avg_vel * (1.0 / n);
        let align = (avg_vel - me.vel) * (0.8 * dt);

        // COHESION: steer toward average position of neighbors.
        // Kept weak (0.15) so fish loosely follow their school without
        // clumping tightly together.
        avg_pos = avg_pos * (1.0 / n);
        let to_center = avg_pos - me.pos;
        let cohere = to_center.normalized() * me.speed * (0.15 * dt);

        force += align + cohere;
    }

    // Apply separation scaled by dt.
    force += sep_force * dt;

    force
}
