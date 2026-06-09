// =============================================================================
// renderer.rs — Diff-based terminal renderer
// =============================================================================
//
// STRATEGY: DOUBLE BUFFERING
// Each frame we write into a "back buffer" (what we *want* on screen).
// When flush() is called, we compare it against the "front buffer" (what is
// *currently* on screen) and only emit terminal commands for cells that
// changed.  This is called a "dirty-cell diff" and it:
//   1. Dramatically reduces the number of bytes sent to the terminal
//   2. Eliminates the visible flicker you'd get from clearing the whole screen
//      each frame and redrawing everything.
//
// FLAT VEC AS 2D ARRAY
// We store both buffers as `Vec<Cell>` (one-dimensional).  To access the cell
// at column x, row y we compute: index = y * width + x.
// Why not `Vec<Vec<Cell>>`?
//   - A Vec<Vec<Cell>> allocates one heap block per row — bad for cache.
//   - A flat Vec<Cell> is one contiguous block of memory — the CPU prefetcher
//     loves it.
//
// `queue!` vs `execute!`
// Both are crossterm macros that emit terminal escape sequences.
//   execute!  — writes AND flushes immediately (one syscall per call)
//   queue!    — only writes to a buffer; you flush manually with out.flush()
// We use queue! to batch all cell writes into one flush() syscall at the end
// of each frame.  This is critical for smooth rendering — 2000 syscalls/frame
// would be painfully slow.
// =============================================================================

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::Write;

// =============================================================================
// Cell — one terminal cell (one character + its color)
// =============================================================================
//
// WHY PartialEq?
// We derive PartialEq so we can write `back[i] != front[i]` to detect changes.
// The compiler generates an implementation that compares each field.
//
// WHY Clone but not Copy?
// Color is not Copy (it could theoretically hold heap data in future crossterm
// versions), so Cell can't be Copy either.  Clone still lets us duplicate cells
// with .clone().
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}

impl Cell {
    // The "empty" cell: a space with no color styling.
    // Used to initialise both buffers and to "erase" a cell.
    pub fn empty() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
        }
    }
}

// =============================================================================
// Renderer
// =============================================================================

pub struct Renderer {
    pub width: u16,
    pub height: u16,
    // `Vec<Cell>` — a growable heap-allocated array.
    // Both buffers are the same length: width * height.
    back_buffer: Vec<Cell>,
    front_buffer: Vec<Cell>,
}

impl Renderer {
    // Associated function (constructor).
    // `w` and `h` come from crossterm::terminal::size() — the real terminal
    // dimensions at startup.
    pub fn new(w: u16, h: u16) -> Self {
        let size = (w as usize) * (h as usize);
        // vec![value; count] creates a Vec of `count` copies of `value`.
        // Cell::empty() doesn't implement Copy so the macro calls .clone()
        // for each element automatically.
        Self {
            width: w,
            height: h,
            back_buffer: vec![Cell::empty(); size],
            front_buffer: vec![Cell::empty(); size],
        }
    }

    // -------------------------------------------------------------------------
    // Write a single character to the back buffer at (col, row).
    // Nothing is sent to the terminal yet — this just updates our local state.
    //
    // BOUNDS CHECK: we use `saturating_sub` and explicit checks rather than
    // panicking on out-of-bounds access.  Fish can legitimately be partially
    // off-screen during transitions.
    // -------------------------------------------------------------------------
    pub fn put(&mut self, x: u16, y: u16, ch: char, fg: Color, bg: Color) {
        if x >= self.width || y >= self.height {
            return; // silently ignore out-of-bounds draws
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.back_buffer[idx] = Cell { ch, fg, bg };
    }

    // Convenience: draw a string left-to-right starting at (x, y).
    pub fn put_str(&mut self, x: u16, y: u16, s: &str, fg: Color, bg: Color) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i as u16, y, ch, fg, bg);
        }
    }

    // Clear the back buffer (fill with empty cells).
    // Called at the start of each frame before entities draw themselves.
    pub fn clear_back(&mut self) {
        for cell in self.back_buffer.iter_mut() {
            *cell = Cell::empty();
        }
    }

    // -------------------------------------------------------------------------
    // flush() — the hot path: compare buffers, emit only changed cells.
    //
    // `out: &mut impl Write`
    //   This is a *trait object* bound on a generic.  It means "any type that
    //   implements the Write trait" — in practice a BufWriter<Stdout>.
    //   `impl Trait` in argument position is syntactic sugar for a generic:
    //     fn flush<W: Write>(&mut self, out: &mut W) -> ...
    //   Both forms compile to the same thing (monomorphisation — the compiler
    //   generates one specialised version per concrete type used).
    //
    // `std::io::Result<()>`
    //   crossterm defines its own Result alias:
    //     type Result<T> = std::result::Result<T, std::io::Error>
    //   The `()` means "no value on success" (like void).
    // -------------------------------------------------------------------------
    pub fn flush(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        // Track cursor position so we can skip MoveTo when writing consecutive
        // cells on the same row (saves ~6 bytes per cell — significant at scale).
        let mut cursor_x: i32 = -1;
        let mut cursor_y: i32 = -1;
        // Track the last color we set to avoid redundant SetForegroundColor calls.
        let mut last_fg = Color::Reset;
        let mut last_bg = Color::Reset;

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y as usize) * (self.width as usize) + (x as usize);

                // THE DIFF: only act if this cell changed.
                if self.back_buffer[idx] == self.front_buffer[idx] {
                    // Cell is unchanged — skip.  This is why rendering is fast.
                    cursor_x = -1; // next write will need MoveTo
                    continue;
                }

                let cell = &self.back_buffer[idx];

                // Move cursor only if needed.
                if cursor_x != x as i32 || cursor_y != y as i32 {
                    queue!(out, MoveTo(x, y))?;
                    // `?` — if MoveTo fails, propagate the error to the caller.
                    // It's shorthand for: match result { Ok(v)=>v, Err(e)=>return Err(e) }
                }

                // Set colors only if they changed.
                if cell.fg != last_fg {
                    queue!(out, SetForegroundColor(cell.fg))?;
                    last_fg = cell.fg;
                }
                if cell.bg != last_bg {
                    queue!(out, SetBackgroundColor(cell.bg))?;
                    last_bg = cell.bg;
                }

                // Print the character.
                queue!(out, Print(cell.ch))?;

                cursor_x = x as i32 + 1;
                cursor_y = y as i32;
            }
        }

        // Flush all queued bytes to the OS in one syscall.
        out.flush()?;

        // Swap buffers: front_buffer becomes what's now on screen.
        // `clone_from` reuses the allocation instead of reallocating.
        self.front_buffer.clone_from(&self.back_buffer);

        Ok(()) // success — no value to return
    }

    // Force a full redraw next frame (used after terminal resize).
    // We invalidate the front buffer by filling it with a value that will
    // never match a real cell.
    pub fn force_redraw(&mut self) {
        // A char that won't appear in our output — guarantees every cell
        // looks "changed" on the next flush.
        let sentinel = Cell {
            ch: '\0',
            fg: Color::Reset,
            bg: Color::Reset,
        };
        for cell in self.front_buffer.iter_mut() {
            *cell = sentinel.clone();
        }
    }

    // Resize the renderer. Called when the terminal window changes size.
    pub fn resize(&mut self, w: u16, h: u16) {
        self.width = w;
        self.height = h;
        let size = (w as usize) * (h as usize);
        self.back_buffer = vec![Cell::empty(); size];
        self.front_buffer = vec![Cell::empty(); size];
    }

    // Draw the initial blank frame and hide the cursor.
    // Called once at startup.
    pub fn init(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        queue!(out, Hide, Clear(ClearType::All))?;
        out.flush()?;
        Ok(())
    }

    // Restore the cursor when the program exits.
    pub fn shutdown(&self, out: &mut impl Write) -> std::io::Result<()> {
        queue!(out, ResetColor, Show)?;
        out.flush()?;
        Ok(())
    }
}
