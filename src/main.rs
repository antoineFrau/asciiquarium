// =============================================================================
// main.rs — Entry point, game loop, terminal setup and input handling
// =============================================================================
//
// This file teaches five important Rust ideas:
//
//   1. `fn main() -> Result<T, E>` — returning errors from main
//   2. RAII cleanup via `Drop`      — the CleanupGuard pattern
//   3. `BufWriter`                  — why buffering terminal output matters
//   4. The game loop structure      — poll input → update → render
//   5. `match` on crossterm Events  — pattern matching on enums with guards
//
// GAME LOOP TIMING
// ────────────────
// We target ~60 FPS by polling for input with a 16 ms timeout.
//
//   event::poll(16ms) — wait up to 16ms for an input event
//   if an event arrived: handle it
//   measure dt since last tick
//   aquarium.update(dt)
//   renderer.flush()
//
// This gives us:
//   - Responsive input (events handled within 16ms)
//   - Smooth animation (~60 Hz updates)
//   - No busy-wait / no `sleep` that would block input
//
// RAII CLEANUP (CleanupGuard)
// ────────────────────────────
// When we enable raw mode and the alternate screen, the terminal is in a
// modified state.  If the program crashes (panics), we must still restore
// the terminal or the user's shell becomes unusable.
//
// C++ would use try/catch or a scope guard.  Rust uses `Drop`:
// a struct whose `drop()` method runs *automatically* when the value goes
// out of scope — whether the function returns normally OR panics.
//
// `let _guard = CleanupGuard;`
//   The leading underscore tells Rust we intentionally don't use `_guard`
//   as a value — we only want its `Drop` side effect.
//   If we wrote `let _ = CleanupGuard;` (no name), the value would be
//   dropped immediately — NOT what we want.
// =============================================================================

use std::{
    io::{self, BufWriter},
    time::{Duration, Instant},
};

use crossterm::{
    cursor::Hide,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{
        self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};

mod aquarium;
mod entities;
mod renderer;
mod vec2;

use aquarium::{Aquarium, Config};
use renderer::Renderer;

// =============================================================================
// CleanupGuard — RAII terminal restoration
//
// RUST CONCEPT: the `Drop` trait
// ────────────────────────────────
// Rust has no destructors in the C++ sense, but it has `Drop`.
// Any type that implements `Drop` gets its `drop()` method called
// automatically when the value goes out of scope.
//
// This is not garbage collection — the compiler inserts the drop call
// at compile time, at the exact scope exit point.  Zero runtime overhead.
// =============================================================================

struct CleanupGuard;

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        // These calls might fail (e.g. stdout already closed), so we discard
        // errors with `let _ = ...`.
        // `let _ = expr` evaluates `expr` and drops the result immediately.
        // It's the idiomatic way to silence "unused Result" warnings.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

// =============================================================================
// main — returns `crossterm::Result<()>`
//
// `crossterm::Result<T>` is a type alias for `std::result::Result<T, io::Error>`.
// Returning a Result from main is allowed since Rust 1.26.
// If main returns `Err(e)`, Rust prints the error and exits with code 1.
//
// The `?` operator propagates errors up the call stack:
//   foo()?  ≡  match foo() { Ok(v) => v, Err(e) => return Err(e.into()) }
// =============================================================================

// Parse a minimal set of CLI flags without pulling in a dependency.
fn parse_config() -> Option<Config> {
    let args: Vec<String> = std::env::args().collect();
    let mut fish_count = 24usize;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                eprintln!("Usage: Asciiquarium [OPTIONS]");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --fish N    Number of fish to spawn (default: 24, min: 1, max: 200)");
                eprintln!("  --help      Show this help message");
                eprintln!();
                eprintln!("Controls:");
                eprintln!("  q / Ctrl-C  Quit");
                eprintln!("  c           Toggle cat mode (watch the tank from the couch)");
                eprintln!("  h           Toggle harpoon mode (click to kill fish)");
                eprintln!("  click       Drop food / harpoon fish");
                return None;
            }
            "--fish" => {
                i += 1;
                if i < args.len() {
                    if let Ok(n) = args[i].parse::<usize>() {
                        fish_count = n.max(1).min(200);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Some(Config { fish_count })
}

// `std::io::Result<()>` is `Result<(), std::io::Error>`.
// Returning a Result from main causes Rust to print the error on failure.
// crossterm used to expose its own `crossterm::Result` alias, but removed it
// in 0.28 — so we reference the standard library type directly.
fn main() -> std::io::Result<()> {
    let Some(config) = parse_config() else { return Ok(()); };

    // ── Terminal setup ────────────────────────────────────────────────────────

    // `BufWriter` wraps stdout and accumulates small writes in a memory
    // buffer, flushing to the OS in one syscall when the buffer is full or
    // when `.flush()` is called explicitly.
    //
    // Without BufWriter:  2000 writes/frame × 1 syscall each = slow
    // With BufWriter:     2000 writes batched into 1-2 syscalls = fast
    let mut stdout = BufWriter::new(io::stdout());

    // Enable raw mode: keystrokes are delivered immediately, without line
    // buffering or echo.  This is necessary for real-time input.
    enable_raw_mode()?;

    // EnterAlternateScreen: switches to a separate terminal buffer (like vim
    // does).  When we exit, LeaveAlternateScreen restores the user's
    // scrollback history — our aquarium doesn't pollute their shell history.
    //
    // EnableMouseCapture: lets crossterm report mouse events (clicks, moves).
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        Hide, // hide the blinking cursor — it would flicker over the fish
    )?;

    // Install the cleanup guard.  From this point on, `_guard.drop()` will
    // always run before main exits — even if we `panic!`.
    let _guard = CleanupGuard;

    // ── Initialise simulation ─────────────────────────────────────────────────

    let (w, h) = terminal::size()?;

    let mut aquarium = Aquarium::new(w, h, config);
    let mut renderer = Renderer::new(w, h);

    renderer.init(&mut stdout)?;

    let mut last_tick = Instant::now();

    // ── Main loop ─────────────────────────────────────────────────────────────
    //
    // The loop runs until the user presses 'q' or Ctrl-C.
    //
    // Structure each iteration:
    //   1. Poll for input (non-blocking, 16 ms timeout = ~60 FPS cap)
    //   2. Update simulation by dt
    //   3. Render to terminal

    'main_loop: loop {
        // ── Input ──────────────────────────────────────────────────────────
        //
        // `event::poll(timeout)` returns Ok(true) if an event is ready before
        // the timeout, Ok(false) if the timeout elapsed with no event.
        // Either way we continue the loop — the timeout controls our frame rate.

        if event::poll(Duration::from_millis(16))? {
            // `event::read()` blocks until an event is available.
            // Since poll returned true, this returns immediately.
            match event::read()? {
                // ── Keyboard ──────────────────────────────────────────────
                //
                // `Event::Key(k)` matches any key event.
                // `if k.code == ...` is a *match guard* — an extra condition
                // that must be true for the arm to fire.
                Event::Key(k) if k.code == KeyCode::Char('q') => break 'main_loop,
                Event::Key(k)
                    if k.code == KeyCode::Char('c')
                        && k.modifiers
                            .contains(event::KeyModifiers::CONTROL) =>
                {
                    break 'main_loop;
                }
                // 'h' toggles harpoon mode on/off.
                Event::Key(k) if k.code == KeyCode::Char('h') => {
                    aquarium.toggle_harpoon();
                }
                // 'c' toggles cat mode: frame the tank into a living room with
                // a cat on the couch watching the fish.
                Event::Key(k) if k.code == KeyCode::Char('c') => {
                    aquarium.toggle_cat_mode();
                    renderer.force_redraw(); // tank moved — repaint every cell
                }

                // ── Mouse ─────────────────────────────────────────────────
                //
                // Left-click behaviour depends on the active mode:
                //   normal mode   → drop a food flake
                //   harpoon mode  → kill the fish under the cursor
                //
                // RUST CONCEPT: METHOD CALLS RETURNING BOOLEANS
                // `try_harpoon` returns `bool` (hit or miss) but we don't
                // use the result here — ignoring it with a bare call is fine.
                // The compiler would warn if `try_harpoon` were marked
                // `#[must_use]`, but it isn't.
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if aquarium.harpoon_mode {
                            aquarium.try_harpoon(m.column as f32, m.row as f32);
                        } else {
                            aquarium.spawn_flake(m.column as f32, m.row as f32);
                        }
                    }
                    _ => {}
                },

                // ── Terminal resize ───────────────────────────────────────
                //
                // When the user resizes the terminal window, crossterm sends
                // an Event::Resize with the new dimensions.  We reinitialise
                // both the aquarium and the renderer.
                Event::Resize(new_w, new_h) => {
                    aquarium.resize(new_w, new_h);
                    renderer.resize(new_w, new_h);
                    renderer.force_redraw(); // paint every cell on next flush
                }

                // Ignore everything else (mouse moves, key releases, etc.).
                _ => {}
            }
        }

        // ── Delta time ─────────────────────────────────────────────────────
        //
        // `Instant::now()` — a monotonic clock reading.  We use `elapsed()`
        // to measure how long since the last frame.
        //
        // `.as_secs_f32()` converts a Duration to seconds as f32.
        // `.min(0.05)` clamps to 50 ms max (20 FPS floor) to prevent
        // entities from teleporting if the frame takes too long.
        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f32().min(0.05);
        last_tick = now;

        // ── Update + render ────────────────────────────────────────────────

        aquarium.update(dt);
        aquarium.render_to(&mut renderer);
        renderer.flush(&mut stdout)?;
    }

    // Restore cursor before exiting (CleanupGuard handles the rest).
    renderer.shutdown(&mut stdout)?;

    Ok(())
    // `_guard` goes out of scope here → Drop runs → terminal is restored.
}
