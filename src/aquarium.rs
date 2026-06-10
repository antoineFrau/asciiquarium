// =============================================================================
// aquarium.rs — Top-level simulation state + update orchestration
// =============================================================================
//
// `Aquarium` owns all entities and drives the entire simulation.
// Its public interface is minimal:
//   new(w, h)           — create a full tank
//   update(dt)          — advance by `dt` seconds
//   spawn_flake(x, y)   — drop food at a terminal position (from mouse click)
//   render_to(renderer) — write all entities into the back buffer
//
// WHY A SINGLE STRUCT FOR EVERYTHING?
// We could have separate managers for fish, bubbles, etc., but for a project
// this size one struct is simpler.  The borrow checker's aliasing rules mean
// we sometimes need to temporarily split the data anyway (see update_fish).
//
// RUST CONCEPT: STRUCT OWNERSHIP
// `Aquarium` *owns* all its Vecs and the Visitor enum.  When an `Aquarium`
// is dropped (goes out of scope), all its data is automatically freed.
// No destructor to write, no memory leak to worry about.
// =============================================================================

use crate::entities::{
    compute_school_force, Bubble, Fish, FishType, Flake, Seaweed, Visitor,
};
use crate::renderer::Renderer;
use crate::vec2::Vec2;
use chrono::Local;
use crossterm::style::Color;
use rand::{rngs::SmallRng, Rng, SeedableRng as _};

// ─── Configuration constants ────────────────────────────────────────────────

const MAX_BUBBLES: usize = 30;
const MAX_FLAKES:  usize = 12;
const MAX_SEAWEED: usize = 14;

// ─── User-facing configuration ──────────────────────────────────────────────

pub struct Config {
    /// How many fish to spawn (default: 24).
    pub fish_count: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { fish_count: 24 }
    }
}

// How many seconds between visitor spawns (on average).
const VISITOR_SPAWN_INTERVAL_S: f32 = 20.0;

// Sea floor: this many rows above the bottom of the terminal.
const SEA_FLOOR_OFFSET: f32 = 2.0;
// Seaweed starts this many rows above the sea floor.
const SEA_LEVEL_OFFSET: f32 = 4.0;

// ─── Cat-mode (living room) palette ──────────────────────────────────────────
// Deliberately muted so the bright tank "glows" against the dim room.
// `Color::AnsiValue(_)` is a plain enum variant, so these are valid consts.
const WALL_BG:   Color = Color::AnsiValue(236); // dark room wall
const CARPET_BG: Color = Color::AnsiValue(95);  // warm carpet strip at the floor
const FRAME_FG:  Color = Color::AnsiValue(214); // light wood / brass frame
const FRAME_BG:  Color = Color::AnsiValue(94);  // darker wood shadow
const COUCH_FG:  Color = Color::AnsiValue(110); // couch piping / outline
const COUCH_BG:  Color = Color::AnsiValue(24);  // muted blue upholstery
const CAT_FG:    Color = Color::AnsiValue(208); // orange tabby
const CAT_EYE:   Color = Color::AnsiValue(46);  // bright green eyes, fixed on the fish

// =============================================================================
// Aquarium
// =============================================================================

pub struct Aquarium {
    // Simulation time in seconds since startup.
    // We use absolute time in all wave formulas to avoid accumulated drift.
    pub time: f32,

    // Terminal dimensions.
    pub width:  f32,
    pub height: f32,

    // Derived coordinates.
    pub sea_floor: f32,   // row of the bottom sand line
    pub sea_level: f32,   // row where seaweed tops are (and flakes deactivate)

    // Entity pools.
    // Using Vec<T> (heap array) for fish and bubbles lets us choose the count
    // at runtime.  Fixed-size [T; N] arrays would also work but require the
    // count at compile time.
    pub fish:    Vec<Fish>,
    pub bubbles: Vec<Bubble>,
    pub flakes:  Vec<Flake>,
    pub seaweed: Vec<Seaweed>,

    // Only one visitor at a time.  `Visitor::None` when none is present.
    pub visitor: Visitor,

    // Timer counting down to the next visitor spawn.
    visitor_timer: f32,

    // Per-simulation RNG.  `SmallRng` is a fast non-cryptographic generator,
    // appropriate for games.  We seed it from the system's random source with
    // `SmallRng::from_entropy()`.
    rng: SmallRng,

    // How many fish were requested (preserved across resize).
    fish_count: usize,

    // ── Harpoon mode ─────────────────────────────────────────────────────────
    // When true, left-clicks kill the fish under the cursor instead of
    // dropping food.  Toggled with the 'h' key.
    pub harpoon_mode: bool,

    // Transient kill notification.
    // Tuple: (display message, fish colour, seconds remaining).
    // `Option<T>` — None when no notification is active.
    kill_msg: Option<(String, Color, f32)>,

    // ── Cat (living-room) mode ─────────────────────────────────────────────────
    // When true, the aquarium is shrunk into a framed, wall-mounted tank set
    // into a cosy living room, with a cat sitting on a couch watching it.
    // Toggled with the 'c' key.
    //
    // The simulation's logical `width`/`height` (above) become the tank's
    // *interior* in this mode; `term_width`/`term_height` keep the true screen
    // size for drawing the room around it.
    pub cat_mode: bool,
    term_width:  f32,
    term_height: f32,

    // Tank interior viewport in absolute terminal cells: top-left (x, y) and
    // size (w, h).  Equal to the whole screen when cat mode is off.
    vp_x: u16,
    vp_y: u16,
    vp_w: u16,
    vp_h: u16,
}

impl Aquarium {
    // -------------------------------------------------------------------------
    // new — initialise a full aquarium for the given terminal size.
    //
    // Starts in full-screen mode (cat mode off).  Cat mode is toggled later at
    // runtime with `toggle_cat_mode`, which rebuilds the tank at the smaller
    // size via the same `build` routine.
    // -------------------------------------------------------------------------
    pub fn new(width: u16, height: u16, config: Config) -> Self {
        Self::build(width, height, config.fish_count.max(1), false)
    }

    // -------------------------------------------------------------------------
    // build — the real constructor, shared by `new`, `resize` and
    // `toggle_cat_mode`.  It works out the tank viewport for the current mode,
    // then populates every entity pool to fit that interior.
    // -------------------------------------------------------------------------
    fn build(term_w: u16, term_h: u16, fish_count: usize, cat_mode: bool) -> Self {
        // The tank interior is the whole screen normally, or a framed
        // sub-rectangle in cat mode.  The simulation only ever sees this
        // interior as its `width`/`height`.
        let (vp_x, vp_y, vp_w, vp_h) = tank_viewport(term_w, term_h, cat_mode);

        let w = vp_w as f32;
        let h = vp_h as f32;

        let sea_floor = h - SEA_FLOOR_OFFSET;
        let sea_level = h - SEA_LEVEL_OFFSET;

        // `SmallRng::from_entropy()` seeds from the OS's random source
        // (e.g. /dev/urandom on Linux).  Each run is unique.
        let mut rng = SmallRng::from_entropy();

        // Spawn fish on a loose grid to prevent initial clustering.
        // We divide the swim area into a grid of cells and place one fish
        // per cell (with random jitter within the cell), guaranteeing an
        // even initial distribution regardless of fish count.
        let cols = (fish_count as f32).sqrt().ceil() as usize;
        let rows = (fish_count + cols - 1) / cols;
        let swim_h = (sea_level - 3.0).max(1.0);
        let cell_w = w / cols as f32;
        let cell_h = swim_h / rows as f32;

        let mut fish = Vec::with_capacity(fish_count);
        for i in 0..fish_count {
            // Pick a random type with weighted probability.
            // Small fish are most common, exotic are rare.
            let n: f32 = rng.gen();
            let fish_type = if n < 0.45 {
                FishType::Small
            } else if n < 0.75 {
                FishType::Medium
            } else if n < 0.92 {
                FishType::Large
            } else {
                FishType::Exotic
            };

            let col = i % cols;
            let row = i / cols;
            let pos = Vec2::new(
                col as f32 * cell_w + rng.gen_range(0.0..cell_w),
                2.0 + row as f32 * cell_h + rng.gen_range(0.0..cell_h),
            );
            fish.push(Fish::new(pos, fish_type, &mut rng));
        }

        // Spawn bubbles distributed along the sea floor.
        let mut bubbles = Vec::with_capacity(MAX_BUBBLES);
        for _ in 0..MAX_BUBBLES {
            let pos = Vec2::new(
                rng.gen_range(0.0..w),
                rng.gen_range((sea_level * 0.3)..sea_floor),
            );
            bubbles.push(Bubble::new(pos, &mut rng));
        }

        // Flake pool starts empty (all inactive).
        // `Vec::with_capacity` pre-allocates memory without creating elements.
        let flakes = Vec::with_capacity(MAX_FLAKES);

        // Place seaweed evenly spaced along the sea floor with jitter.
        let mut seaweed = Vec::with_capacity(MAX_SEAWEED);
        let spacing = w / MAX_SEAWEED as f32;
        for i in 0..MAX_SEAWEED {
            let jitter = rng.gen_range(-spacing * 0.3..spacing * 0.3);
            let x = i as f32 * spacing + spacing * 0.5 + jitter;
            seaweed.push(Seaweed::new(x, sea_floor - 1.0, &mut rng));
        }

        Self {
            time: 0.0,
            width: w,
            height: h,
            sea_floor,
            sea_level,
            fish,
            bubbles,
            flakes,
            seaweed,
            visitor: Visitor::None,
            visitor_timer: VISITOR_SPAWN_INTERVAL_S * 0.5, // first visitor arrives sooner
            rng,
            fish_count,
            harpoon_mode: false,
            kill_msg: None,
            cat_mode,
            term_width:  term_w as f32,
            term_height: term_h as f32,
            vp_x,
            vp_y,
            vp_w,
            vp_h,
        }
    }

    // -------------------------------------------------------------------------
    // resize — called when the terminal window changes size (Event::Resize).
    // We reinitialise to avoid entities being outside the new bounds, keeping
    // the current display mode.
    // -------------------------------------------------------------------------
    pub fn resize(&mut self, width: u16, height: u16) {
        *self = Aquarium::build(width, height, self.fish_count, self.cat_mode);
    }

    // -------------------------------------------------------------------------
    // toggle_cat_mode — switch between full-screen and living-room (cat) mode.
    //
    // The tank changes size, so we rebuild the simulation to fit the new
    // interior (just like a resize).  The current fish count is preserved.
    // -------------------------------------------------------------------------
    pub fn toggle_cat_mode(&mut self) {
        let (w, h) = (self.term_width as u16, self.term_height as u16);
        *self = Aquarium::build(w, h, self.fish_count, !self.cat_mode);
    }

    // -------------------------------------------------------------------------
    // to_tank_coords — map an absolute terminal click into tank-local space.
    //
    // In full-screen mode the tank fills the screen, so this is the identity.
    // In cat mode the tank is offset by (vp_x, vp_y); clicks outside the tank
    // interior return None so they don't feed/harpoon phantom fish through the
    // wall.
    // -------------------------------------------------------------------------
    fn to_tank_coords(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let lx = x - self.vp_x as f32;
        let ly = y - self.vp_y as f32;
        if lx < 0.0 || ly < 0.0 || lx >= self.vp_w as f32 || ly >= self.vp_h as f32 {
            return None;
        }
        Some((lx, ly))
    }

    // -------------------------------------------------------------------------
    // spawn_flake — drop a food flake at the given terminal coordinate.
    //
    // We reuse an inactive slot if one exists; otherwise create a new entry
    // if we haven't hit MAX_FLAKES yet.  This avoids unbounded allocation.
    // -------------------------------------------------------------------------
    pub fn spawn_flake(&mut self, x: f32, y: f32) {
        // Translate the absolute click into tank-local space; ignore clicks
        // that land on the room rather than inside the tank (cat mode).
        let Some((x, y)) = self.to_tank_coords(x, y) else { return };
        let pos = Vec2::new(x, y);

        // Look for an inactive slot first.
        // `.iter_mut()` gives mutable references to each element.
        // `.find()` returns `Option<&mut Flake>` — the first match or None.
        if let Some(slot) = self.flakes.iter_mut().find(|f| !f.active) {
            // Reuse the existing slot: reset it.
            slot.pos    = pos;
            slot.active = true;
            return;
        }

        // No inactive slot; push a new one if under the cap.
        if self.flakes.len() < MAX_FLAKES {
            self.flakes.push(Flake::new(pos));
        }
        // If cap is reached, silently ignore the click.
    }

    // -------------------------------------------------------------------------
    // toggle_harpoon — switch harpoon mode on or off.
    // -------------------------------------------------------------------------
    pub fn toggle_harpoon(&mut self) {
        self.harpoon_mode = !self.harpoon_mode;
    }

    // -------------------------------------------------------------------------
    // try_harpoon — attempt to kill the fish at terminal position (x, y).
    //
    // Returns `true` if a fish was hit.
    //
    // RUST CONCEPT: TWO-STEP BORROW PATTERN
    // We first find the target index in a read-only loop (immutable borrow of
    // `self.fish`).  Only after that loop ends do we take a mutable borrow of
    // `self.fish[i]` to deactivate it.  This satisfies the borrow checker:
    // an immutable borrow and a mutable borrow cannot overlap.
    //
    // HIT DETECTION
    // Each fish occupies a rectangle: columns [pos.x, pos.x + glyph_width),
    // row pos.y.  We expand each edge by 1 cell so a click near (but not
    // exactly on) the glyph still registers.
    // -------------------------------------------------------------------------
    pub fn try_harpoon(&mut self, x: f32, y: f32) -> bool {
        // Translate the absolute click into tank-local space; a click on the
        // room (cat mode) hits nothing.
        let Some((x, y)) = self.to_tank_coords(x, y) else { return false };
        let click = Vec2::new(x, y);

        // ── Pass 1: find the closest fish whose bounding box contains the click.
        let mut best_idx  = None;
        let mut best_dist = f32::MAX;

        for (i, fish) in self.fish.iter().enumerate() {
            if !fish.active {
                continue;
            }
            // Glyph width for this fish's specific ASCII variant.
            let glyph_w = fish.fish_type.glyphs(fish.glyph_variant).0.chars().count() as f32;

            // Expand the bounding box by 1 cell on each side for forgiving aim.
            let in_x = click.x >= fish.pos.x - 1.0
                    && click.x <= fish.pos.x + glyph_w + 1.0;
            let in_y = (click.y - fish.pos.y).abs() <= 1.0;

            if in_x && in_y {
                let dist = (fish.pos - click).length();
                if dist < best_dist {
                    best_dist = dist;
                    best_idx  = Some(i);
                }
            }
        }

        // ── Pass 2: deactivate the hit fish and record the kill notification.
        let Some(i) = best_idx else { return false };

        // Mutable borrow — safe because the read loop above has already ended.
        let fish = &mut self.fish[i];
        fish.active = false;

        // Collect what we need *before* the mutable borrow ends so we can
        // move the values into the notification tuple.
        let (glyph, _) = fish.fish_type.glyphs(fish.glyph_variant);
        let type_name  = match fish.fish_type {
            FishType::Small  => "Small",
            FishType::Medium => "Medium",
            FishType::Large  => "Large",
            FishType::Exotic => "Exotic",
        };
        let color = fish.color; // Color is Copy, so this is a value copy

        // `fish` borrow ends here (end of mutable borrow scope).
        self.kill_msg = Some((
            format!(" >> Harpooned a {} fish!  {}  << ", type_name, glyph),
            color,
            2.5, // display for 2.5 seconds
        ));
        true
    }

    // -------------------------------------------------------------------------
    // update — the main simulation step.
    //
    // Order matters:
    //   1. Advance time
    //   2. Update fish (includes borrow-checker two-pass schooling)
    //   3. Update other entities
    //   4. Maybe spawn a visitor
    // -------------------------------------------------------------------------
    pub fn update(&mut self, dt: f32) {
        // Clamp dt to 50 ms to avoid "spiral of death" if a frame takes too long.
        // Without this, a 2-second freeze would send fish flying across the screen.
        let dt = dt.min(0.05);
        self.time += dt;

        self.update_fish(dt);
        self.update_bubbles(dt);
        self.update_flakes(dt);
        self.update_visitor(dt);

        // Tick down the kill notification timer.
        //
        // RUST CONCEPT: TEMPORARY MUTABLE BORROW SCOPE
        // We borrow `self.kill_msg` mutably inside the `if let` block, decrement
        // the timer, and capture the expiry check as a plain `bool`.
        // Because `expired` is a value (not a reference), the mutable borrow of
        // `self.kill_msg` ends before we use `expired` to write `None` — no
        // aliasing conflict.
        let expired = if let Some((_, _, ref mut timer)) = self.kill_msg {
            *timer -= dt;
            *timer <= 0.0
        } else {
            false
        };
        if expired {
            self.kill_msg = None;
        }
    }

    // ─── Private update helpers ───────────────────────────────────────────────

    // -------------------------------------------------------------------------
    // update_fish — the two-pass borrow-checker pattern
    //
    // PASS 1 (read-only): iterate over the whole pool and pre-compute the
    //   schooling + separation force for each fish.  We store all forces in a
    //   temporary Vec<Vec2>.
    //
    // WHY CAN'T WE DO THIS IN ONE PASS?
    //   The fish.rs `compute_school_force` function takes `&self.fish` (an
    //   immutable borrow of the whole Vec).  Meanwhile, applying the force
    //   requires `&mut fish_entry` (a mutable borrow of one element).
    //   Rust does NOT allow both at the same time over the same Vec.
    //   This is the borrow checker preventing data races — the same aliasing
    //   rules that make Rust safe in multi-threaded code apply here too.
    //
    // PASS 2 (write): now that we have a standalone `forces` Vec, we can
    //   mutably iterate over `self.fish` without conflicting borrows.
    // -------------------------------------------------------------------------
    fn update_fish(&mut self, dt: f32) {
        let time = self.time;
        let w    = self.width;
        let h    = self.height;

        // Find the threat position (if any visitor is on screen).
        let threat = self.visitor.threat_pos();

        // Find the nearest active food flake position (for all fish to target).
        // We pick one nearest for simplicity; a more detailed simulation would
        // let each fish independently pick its own nearest flake.
        let nearest_flake: Option<Vec2> = {
            // This is a chain of iterator adapters — Rust's idiomatic style.
            // `.iter()` — iterate over &Flake references
            // `.filter()` — keep only active flakes
            // `.map()` — project to position
            // `.min_by()` — find the one with minimum y (highest on screen)
            //               so fish go for the freshest (highest) flake
            // `.copied()` — turn Option<&Vec2> into Option<Vec2>
            self.flakes
                .iter()
                .filter(|f| f.active)
                .map(|f| f.pos)
                .min_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
        };

        // ── PASS 1: compute forces ────────────────────────────────────────
        // `(0..self.fish.len())` is a Range — it implements Iterator.
        // `.map(|i| ...)` transforms each index to a Vec2.
        // `.collect::<Vec<Vec2>>()` materialises the lazy iterator into a Vec.
        //
        // At this point `self.fish` is only BORROWED (read), which is fine.
        let forces: Vec<Vec2> = (0..self.fish.len())
            .map(|i| compute_school_force(i, &self.fish, dt))
            .collect();

        // ── PASS 2: apply forces and integrate ───────────────────────────
        // `.iter_mut().enumerate()` gives (index, &mut Fish) pairs.
        // `.zip(forces.iter())` pairs each fish with its pre-computed force.
        // Now `self.fish` is MUTABLY BORROWED, which is fine because `forces`
        // is a completely separate allocation — no aliasing.
        for (fish, force) in self.fish.iter_mut().zip(forces.iter()) {
            if !fish.active {
                continue;
            }

            fish.apply_wander(dt, time);
            fish.apply_school_force(*force);  // *force dereferences &Vec2 to Vec2 (Copy)
            fish.apply_feed_seek(dt, nearest_flake);
            fish.apply_avoidance(dt, threat);
            fish.clamp_speed();
            fish.integrate(dt, w, h);
        }

        // Check if any fish ate a flake (within 1.5 cells).
        for fish in self.fish.iter() {
            for flake in self.flakes.iter_mut() {
                if !flake.active {
                    continue;
                }
                if (fish.pos - flake.pos).length() < 1.5 {
                    flake.active = false; // eaten!
                }
            }
        }
    }

    fn update_bubbles(&mut self, dt: f32) {
        let time = self.time;
        let h    = self.height;
        for bubble in self.bubbles.iter_mut() {
            bubble.update(dt, time, h);
        }
    }

    fn update_flakes(&mut self, dt: f32) {
        let time      = self.time;
        let sea_level = self.sea_level;
        // `enumerate()` yields (index, &mut Flake).
        // We pass the index as the per-flake phase seed (see flake.rs).
        for (i, flake) in self.flakes.iter_mut().enumerate() {
            if flake.active {
                flake.update(dt, time, i, sea_level);
            }
        }
    }

    fn update_visitor(&mut self, dt: f32) {
        let time = self.time;
        let w    = self.width;

        // Update existing visitor.
        self.visitor.update(dt, time, w);

        // Countdown timer for next spawn.
        if self.visitor.is_none() {
            self.visitor_timer -= dt;
            if self.visitor_timer <= 0.0 {
                let h = self.height;
                let sl = self.sea_level;
                self.visitor = Visitor::spawn(&mut self.rng, w, h, sl);
                // Reset timer with some randomness.
                self.visitor_timer = VISITOR_SPAWN_INTERVAL_S
                    + self.rng.gen_range(-5.0_f32..5.0);
            }
        }
    }

    // -------------------------------------------------------------------------
    // render_to — draw everything into the renderer's back buffer.
    //
    // Draw order (back to front):
    //   1. Background (gradient)
    //   2. Seaweed
    //   3. Sea floor
    //   4. Bubbles
    //   5. Flakes
    //   6. Fish
    //   7. Visitor
    //   8. HUD (clock + controls hint)
    // -------------------------------------------------------------------------
    pub fn render_to(&self, renderer: &mut Renderer) {
        renderer.clear_back();

        // In cat mode the room (wall, frame, couch, cat) is painted first at
        // absolute screen coordinates, then the tank contents are drawn inside
        // a viewport so they stay within the frame.
        if self.cat_mode {
            self.draw_room(renderer);
            renderer.set_viewport(self.vp_x, self.vp_y, self.vp_w, self.vp_h);
        }

        // The world (water + entities) is drawn identically in both modes —
        // the viewport, when active, transparently offsets and clips it.
        self.draw_world(renderer);
        // Kill banner belongs to the tank, so draw it while the viewport (if
        // any) is still active and on top of the fish.
        self.draw_kill_notification(renderer);

        if self.cat_mode {
            renderer.reset_viewport();
        }

        // HUD always lives on the full screen, above everything.
        self.draw_hud(renderer);
    }

    // -------------------------------------------------------------------------
    // draw_world — the ocean and its inhabitants, drawn in tank-local
    // coordinates (the renderer's viewport handles any offset in cat mode).
    // -------------------------------------------------------------------------
    fn draw_world(&self, renderer: &mut Renderer) {
        let time = self.time;
        self.draw_background(renderer);
        for sw in &self.seaweed {
            sw.draw(renderer, time);
        }
        self.draw_sea_floor(renderer);
        for bubble in &self.bubbles {
            bubble.draw(renderer);
        }
        for flake in &self.flakes {
            flake.draw(renderer);
        }
        for fish in &self.fish {
            fish.draw(renderer);
        }
        self.visitor.draw(renderer, time);
    }

    // ─── Draw helpers ─────────────────────────────────────────────────────────

    fn draw_background(&self, renderer: &mut Renderer) {
        let w = self.width as u16;

        // Simple vertical gradient: darker at top (deep water), lighter
        // near the sea floor (shallow water).
        // We use background colors with space characters to paint each row.
        // 256-color palette blues: 17 (deep navy) → 19 → 20 → 21 (bright blue)
        // then 27 near the surface.
        let sea_level_u = self.sea_level as u16;

        for y in 1..sea_level_u {
            // Map row to gradient index.
            let depth_frac = y as f32 / self.sea_level;
            let bg = interpolate_ocean_color(depth_frac);

            for x in 0..w {
                // Only paint cells not already occupied (seaweed/fish will
                // overwrite these anyway, but we set a baseline color).
                renderer.put(x, y, ' ', Color::Reset, bg);
            }
        }
    }

    fn draw_sea_floor(&self, renderer: &mut Renderer) {
        let w       = self.width as u16;
        let floor_y = self.sea_floor as u16;
        let level_y = self.sea_level as u16;

        // Seaweed row (light green sand/rock line just above the floor).
        for x in 0..w {
            renderer.put(x, level_y, '~', Color::AnsiValue(64), Color::Reset);
        }
        // Sand floor line.
        for x in 0..w {
            renderer.put(x, floor_y, '=', Color::AnsiValue(136), Color::Reset);
        }
        // Bottom row: deep sea sediment.
        if floor_y + 1 < self.height as u16 {
            for x in 0..w {
                renderer.put(x, floor_y + 1, ' ', Color::Reset, Color::AnsiValue(94));
            }
        }
    }

    fn draw_hud(&self, renderer: &mut Renderer) {
        // The HUD spans the whole screen, so use the terminal width (which
        // equals the tank width when cat mode is off).
        let w = self.term_width as u16;

        // Clock in the top-left corner.
        let now = Local::now();
        let clock_str = format!(" {} ", now.format("%H:%M:%S"));
        renderer.put_str(0, 0, &clock_str, Color::White, Color::AnsiValue(17));

        // Harpoon mode indicator — shown right after the clock when active.
        if self.harpoon_mode {
            let ind = " [HARPOON] ";
            let ind_x = clock_str.chars().count() as u16;
            renderer.put_str(ind_x, 0, ind, Color::Red, Color::AnsiValue(17));
        }

        // Controls hint at the top-right.
        // The text and colour change to signal the active mode.
        let hint = if self.harpoon_mode {
            " q:quit  h:off  click:KILL "
        } else if self.cat_mode {
            " q:quit  c:full  h:harpoon  click:feed "
        } else {
            " q:quit  c:cat  h:harpoon  click:feed "
        };
        let hint_color = if self.harpoon_mode {
            Color::Red            // red in harpoon mode — danger!
        } else {
            Color::AnsiValue(246) // dim grey in normal mode
        };
        let hint_x = w.saturating_sub(hint.chars().count() as u16);
        renderer.put_str(hint_x, 0, hint, hint_color, Color::AnsiValue(17));

        // Fish count at the top-centre — but only if it fits cleanly between
        // the clock (left) and the hint (right).  On a narrow terminal the
        // three would collide, so we drop the least-critical counter rather
        // than render a garbled overlap.
        let active_fish = self.fish.iter().filter(|f| f.active).count();
        let count_str = format!(" {}/{} fish ", active_fish, self.fish.len());
        let count_len = count_str.chars().count() as u16;
        let count_x   = (w / 2).saturating_sub(count_len / 2);
        let clock_end = clock_str.chars().count() as u16;
        if count_x >= clock_end && count_x + count_len <= hint_x {
            renderer.put_str(count_x, 0, &count_str, Color::AnsiValue(159), Color::AnsiValue(17));
        }
    }

    // -------------------------------------------------------------------------
    // draw_kill_notification — render the transient "you killed X" banner.
    //
    // The banner appears centred at 1/3 of the screen height so it floats in
    // the open water column, clearly visible without obscuring the sea floor.
    // It uses the killed fish's own colour so players can identify it at a
    // glance even after the fish has disappeared.
    // -------------------------------------------------------------------------
    fn draw_kill_notification(&self, renderer: &mut Renderer) {
        // `let … else` — Rust 1.65+ early-return pattern on Option/Result.
        // If kill_msg is None we return immediately; if Some we destructure.
        let Some((msg, color, _)) = &self.kill_msg else { return };

        let w   = self.width as u16;
        let y   = (self.height as u16) / 3; // upper third of the water column
        let len = msg.chars().count() as u16;
        let x   = (w / 2).saturating_sub(len / 2);

        // Draw the message in the killed fish's colour on a dark navy background
        // so it pops against any ocean shade underneath.
        renderer.put_str(x, y, msg, *color, Color::AnsiValue(17));
    }

    // =========================================================================
    // Cat mode — the living room around the tank
    // =========================================================================
    //
    // All of these draw at *absolute* terminal coordinates (the viewport is not
    // active when they run).  The single source of truth for layout is the
    // stored tank viewport (`vp_x/y/w/h`): the picture frame hugs it, and the
    // couch + cat sit in the strip of screen below it.
    // -------------------------------------------------------------------------
    fn draw_room(&self, renderer: &mut Renderer) {
        let tw = self.term_width as u16;
        let th = self.term_height as u16;

        // 1. Wall — fill the whole screen.  The tank contents and furniture
        //    paint over it afterwards.
        for y in 0..th {
            for x in 0..tw {
                renderer.put(x, y, ' ', Color::Reset, WALL_BG);
            }
        }

        // 2. Carpet — a couple of warm rows along the very bottom.
        let carpet_top = th.saturating_sub(2);
        for y in carpet_top..th {
            for x in 0..tw {
                renderer.put(x, y, ' ', Color::Reset, CARPET_BG);
            }
        }

        // 3. Picture frame around the tank, derived from the viewport.
        self.draw_tank_frame(renderer);

        // 4. Couch + cat in the foreground strip below the tank.
        //    Width is ~70% of the screen, centred, but never wider than the
        //    screen itself.
        let couch_w = ((tw * 7) / 10).max(24).min(tw.saturating_sub(2)).max(8);
        let couch_x = (tw.saturating_sub(couch_w)) / 2;
        let couch_h = 4u16;
        // Sit the couch just above the carpet.
        let couch_top = th.saturating_sub(couch_h + 1);

        self.draw_couch(renderer, couch_x, couch_top, couch_w);

        // The cat sits centred on the couch, just above the backrest, looking
        // up at the fish.
        let cat_x = (couch_x + couch_w / 2).saturating_sub(9);
        let cat_top = couch_top.saturating_sub(3);
        self.draw_cat(renderer, cat_x, cat_top);
    }

    // -------------------------------------------------------------------------
    // draw_tank_frame — a wooden picture frame hugging the tank interior.
    // -------------------------------------------------------------------------
    fn draw_tank_frame(&self, renderer: &mut Renderer) {
        // Outer rectangle = interior expanded by the 1-cell border.
        let ox = self.vp_x.saturating_sub(1);
        let oy = self.vp_y.saturating_sub(1);
        let ow = self.vp_w + 2;
        let oh = self.vp_h + 2;
        let right  = ox + ow - 1;
        let bottom = oy + oh - 1;

        // Top and bottom edges.
        for x in ox..=right {
            renderer.put(x, oy,     '=', FRAME_FG, FRAME_BG);
            renderer.put(x, bottom, '=', FRAME_FG, FRAME_BG);
        }
        // Left and right edges.
        for y in oy..=bottom {
            renderer.put(ox,    y, '|', FRAME_FG, FRAME_BG);
            renderer.put(right, y, '|', FRAME_FG, FRAME_BG);
        }
        // Corners.
        renderer.put(ox,    oy,     '+', FRAME_FG, FRAME_BG);
        renderer.put(right, oy,     '+', FRAME_FG, FRAME_BG);
        renderer.put(ox,    bottom, '+', FRAME_FG, FRAME_BG);
        renderer.put(right, bottom, '+', FRAME_FG, FRAME_BG);

        // Little engraved label on the top rail.
        let label = " ~ aquarium ~ ";
        let len   = label.chars().count() as u16;
        if ow > len + 2 {
            let lx = ox + (ow - len) / 2;
            renderer.put_str(lx, oy, label, FRAME_FG, FRAME_BG);
        }
    }

    // -------------------------------------------------------------------------
    // draw_couch — a filled four-row sofa with cushion seams.
    //
    //   /----------\
    //   |    |     |
    //   |    |     |
    //   \__________/
    // -------------------------------------------------------------------------
    fn draw_couch(&self, renderer: &mut Renderer, cx: u16, cy: u16, cw: u16) {
        if cw < 4 {
            return;
        }
        let right = cx + cw - 1;

        for r in 0..4u16 {
            let y = cy + r;
            for x in cx..=right {
                let left_edge  = x == cx;
                let right_edge = x == right;
                let ch = match r {
                    0 => if left_edge { '/' } else if right_edge { '\\' } else { '-' },
                    3 => if left_edge { '\\' } else if right_edge { '/' } else { '_' },
                    _ => if left_edge || right_edge { '|' } else { ' ' },
                };
                renderer.put(x, y, ch, COUCH_FG, COUCH_BG);
            }
        }

        // Cushion seams on the two backrest rows.
        let ncush = (cw / 14).max(2);
        for i in 1..ncush {
            let sx = cx + (i * cw) / ncush;
            renderer.put(sx, cy + 1, '|', COUCH_FG, COUCH_BG);
            renderer.put(sx, cy + 2, '|', COUCH_FG, COUCH_BG);
        }
    }

    // -------------------------------------------------------------------------
    // draw_cat — "Melu", a sitting cat drawn against the wall just above the
    // couch.  It blinks every few seconds and waves a paw beside its head;
    // spaces in the sprite are skipped so the wall shows through the gaps.
    //
    //     |\__/,|   (`\
    //   _.|o o  |_   ) )
    //  -(((---(((--------
    // -------------------------------------------------------------------------
    fn draw_cat(&self, renderer: &mut Renderer, x: u16, y: u16) {
        let time = self.time;

        // Blink for a short window every ~4.5 seconds.
        let blink = (time % 4.5) < 0.18;
        let eyes = if blink { "- -" } else { "o o" };

        // The paw waves: two poses alternate every ~0.5s.  The body (cols 0..12
        // of the top two rows) is fixed; only the paw half on the right moves.
        let paw_up = (time * 2.0) as i32 % 2 == 0;
        let (row0, row1) = if paw_up {
            ("    |\\__/,|   (`\\".to_string(),
             format!("  _.|{}  |_   ) )", eyes))
        } else {
            ("    |\\__/,|     /´)".to_string(),
             format!("  _.|{}  |_   ( (", eyes))
        };
        let rows = [row0.as_str(), row1.as_str(), "-(((---(((--------"];

        for (r, line) in rows.iter().enumerate() {
            let yy = y + r as u16;
            for (i, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    continue; // let the wall show through
                }
                renderer.put(x + i as u16, yy, ch, CAT_FG, WALL_BG);
            }
        }

        // Green eyes, fixed on the tank — overdrawn only when the cat isn't
        // mid-blink.  Eye glyphs sit at offsets 5 and 7 of the middle row.
        if !blink {
            renderer.put(x + 5, y + 1, 'o', CAT_EYE, WALL_BG);
            renderer.put(x + 7, y + 1, 'o', CAT_EYE, WALL_BG);
        }

        // Her name, signed on the wall beside her on the bottom row.
        renderer.put_str(x + 24, y + 2, "Melu", CAT_FG, WALL_BG);
    }
}

// =============================================================================
// Free helper: work out the tank interior rectangle for the current mode.
//
// Returns (x, y, w, h) in absolute terminal cells.
//   - full-screen mode → the whole terminal
//   - cat mode         → a framed sub-rectangle, leaving a margin of wall on
//                        the sides/top and a foreground strip at the bottom for
//                        the couch and cat.
//
// We avoid `clamp` (which panics when min > max on tiny terminals) and lean on
// saturating arithmetic with min/max so the layout degrades gracefully instead
// of crashing in a one-line window.
// =============================================================================
fn tank_viewport(term_w: u16, term_h: u16, cat_mode: bool) -> (u16, u16, u16, u16) {
    if !cat_mode {
        return (0, 0, term_w, term_h);
    }

    // Foreground strip reserved at the bottom for the couch + cat (~1/3 of the
    // height, but at least 9 rows and never eating the whole screen).
    let foreground = (term_h / 3).max(9).min(term_h.saturating_sub(6)).max(3);
    // Side margins of wall around the frame.
    let side = (term_w / 8).max(2).min(10);
    let top  = 1u16; // a row of wall above the frame

    // Outer frame rectangle.
    let outer_x = side;
    let outer_y = top;
    let outer_w = term_w.saturating_sub(2 * side).max(6);
    let outer_h = term_h.saturating_sub(foreground + top).max(6);

    // Interior = outer minus the 1-cell border on every edge.
    let vp_x = outer_x + 1;
    let vp_y = outer_y + 1;
    let vp_w = outer_w.saturating_sub(2).max(1);
    let vp_h = outer_h.saturating_sub(2).max(1);
    (vp_x, vp_y, vp_w, vp_h)
}

// =============================================================================
// Free helper: interpolate ocean color by depth fraction.
//
// `depth_frac` is in [0, 1] where 0 = top (deep/dark) and 1 = sea floor.
// =============================================================================
fn interpolate_ocean_color(depth_frac: f32) -> Color {
    // Simple step function over the ANSI 256-color blue ramp.
    // Values chosen to look like a water gradient in most terminals.
    if depth_frac < 0.15 {
        Color::AnsiValue(17) // very dark navy
    } else if depth_frac < 0.30 {
        Color::AnsiValue(18)
    } else if depth_frac < 0.50 {
        Color::AnsiValue(19)
    } else if depth_frac < 0.70 {
        Color::AnsiValue(20)
    } else if depth_frac < 0.85 {
        Color::AnsiValue(21)
    } else {
        Color::AnsiValue(27) // lighter blue near surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::Renderer;

    // Render one cat-mode frame and dump it as text so a human can eyeball the
    // living-room scene.  Run with: cargo test scene_snapshot -- --nocapture
    #[test]
    fn scene_snapshot() {
        let (w, h) = (80u16, 28u16);
        let mut aq = Aquarium::build(w, h, 18, true);
        let mut r = Renderer::new(w, h);
        // Advance a little so fish/bubbles spread out.
        for _ in 0..30 {
            aq.update(0.05);
        }
        aq.render_to(&mut r);
        println!("\n--- cat-mode scene ({}x{}) ---", w, h);
        for row in r.debug_rows() {
            println!("{}", row);
        }
    }
}
