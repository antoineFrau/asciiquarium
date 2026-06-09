// =============================================================================
// vec2.rs — 2D floating-point vector
// =============================================================================
//
// WHY NOT USE A CRATE?
// There are great math crates (nalgebra, glam, ultraviolet), but for a project
// this size writing Vec2 ourselves takes 50 lines and teaches how Rust traits
// and operator overloading work.
//
// WHAT IS `f32`?
// A 32-bit IEEE 754 floating-point number.  Fish positions, velocities and
// sine-wave phases don't need double precision (f64), and f32 is faster on
// most CPUs when you're doing thousands of operations per frame.
//
// THE `#[derive(...)]` ATTRIBUTE
// This is a compile-time code generator.  Writing `#[derive(Clone, Copy)]`
// tells the compiler to automatically implement those traits for Vec2.
//
//   Clone  — allows `.clone()` to create a copy
//   Copy   — a *marker* trait: values are bitwise-copied instead of moved.
//            Because Vec2 is just two f32s on the stack, copying is free.
//            After `let b = a;` with Copy, `a` is still usable — unlike
//            heap types like String or Vec.
//
//   Debug  — allows `{:?}` formatting in println! (useful for debugging)
//   Default — provides Vec2::default() → Vec2 { x: 0.0, y: 0.0 }
//             PartialEq — allows == and != comparisons between two Vec2s
// =============================================================================

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

// `impl Vec2` — this block defines methods that belong to the Vec2 type.
// In C++ these would be member functions inside the class body.
// Rust separates data (struct) from behaviour (impl) by design.
impl Vec2 {
    // Associated function (no `self`) — like a static method in C++.
    // Called as Vec2::new(1.0, 2.0).
    pub fn new(x: f32, y: f32) -> Self {
        // `Self` always means "the current type" — same as writing Vec2 here.
        Self { x, y }
        // Note: when a field name matches a local variable name, Rust lets you
        // write `{ x }` instead of `{ x: x }`.  This is called field init
        // shorthand.
    }

    // Euclidean length: √(x² + y²)
    // `self` (no `&`) would *consume* the value; `self` with Copy types is
    // fine to take by value because the caller still has their copy.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
        // `.sqrt()` is a method on f32 (in std).  No semicolon = this is the
        // return value (Rust's implicit return for the last expression).
    }

    // Length squared — avoids a sqrt when you only need to compare distances.
    // Comparing dist² < radius² is equivalent to dist < radius but cheaper.
    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    // Returns a vector pointing in the same direction but with length 1.
    // Safe: if the vector is zero-length we return zero rather than NaN.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 0.0 {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        } else {
            // `Self::default()` calls the Default impl we derived: { x:0, y:0 }
            Self::default()
        }
    }

    // Dot product: a·b = ax*bx + ay*by
    // Useful for: projecting one vector onto another, checking if two
    // vectors point in roughly the same direction (positive = same side).
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    // Clamp each component independently between lo and hi.
    pub fn clamp(self, lo: f32, hi: f32) -> Self {
        Self {
            x: self.x.clamp(lo, hi),
            y: self.y.clamp(lo, hi),
        }
    }

    // Linear interpolation between self and other by factor t ∈ [0, 1].
    // At t=0 returns self, at t=1 returns other.
    // Used to smoothly steer fish toward a target instead of snapping.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }

    // Euclidean distance between two points.
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
}

// =============================================================================
// OPERATOR OVERLOADING via traits
//
// In Rust, operators are traits.  `a + b` desugars to
// `std::ops::Add::add(a, b)`.  By implementing `Add` for Vec2, we teach the
// compiler what `+` means for our type.
//
// This is more verbose than C++ operator overloading, but it's explicit —
// you can see exactly which operations exist for a type by looking at which
// traits it implements.
// =============================================================================

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

// Vec2 + Vec2
impl Add for Vec2 {
    type Output = Self; // what type `+` produces
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

// Vec2 += Vec2
impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

// Vec2 - Vec2
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

// Vec2 -= Vec2
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

// Vec2 * f32  (scale by scalar)
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }
}

// f32 * Vec2  (commutative: same as above but with operands swapped)
// Without this, `2.0 * v` would not compile even though `v * 2.0` works.
impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        Vec2 {
            x: self * v.x,
            y: self * v.y,
        }
    }
}

// Vec2 / f32
impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, s: f32) -> Self {
        Self {
            x: self.x / s,
            y: self.y / s,
        }
    }
}

// -Vec2  (negate)
impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}
