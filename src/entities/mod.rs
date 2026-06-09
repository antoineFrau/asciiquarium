// =============================================================================
// entities/mod.rs — Module declarations and re-exports
// =============================================================================
//
// RUST CONCEPT: THE MODULE SYSTEM
// ─────────────────────────────────
// In Rust, files don't automatically become part of your program.  You must
// declare each module explicitly, either in main.rs or in a parent mod.rs.
//
// This file IS the `entities` module.  By writing `pub mod fish;` here, we
// tell the compiler to look for `src/entities/fish.rs` and compile it as a
// submodule named `entities::fish`.
//
// The `pub use` lines re-export symbols from child modules so callers can
// write `entities::Fish` instead of `entities::fish::Fish`.
// =============================================================================

// Declare submodules (each one maps to a .rs file in this directory).
pub mod bubble;
pub mod fish;
pub mod flake;
pub mod seaweed;
pub mod visitor;

// Re-export the most-used types at the `entities` level for convenience.
// Without these, users would have to write `entities::fish::Fish`.
// With them, they can write `entities::Fish`.
pub use bubble::Bubble;
pub use fish::{compute_school_force, Fish, FishType};
pub use flake::Flake;
pub use seaweed::Seaweed;
pub use visitor::Visitor;
