//! **Vendored, verbatim** procedural-tree generator from the sibling `bevy-world-editor` project
//! (`crates/worldgen/src/{tree,noise,rng}.rs`). Pure, deterministic, `std`-only — no Bevy, no I/O,
//! no external crates — which is exactly why it could be dropped in as three untouched files.
//!
//! The ONLY edits made on vendoring were the two intra-crate import paths (`crate::rng::…` →
//! `super::rng::…`). Keep it that way: if the upstream generator changes, re-copy rather than
//! hand-merge. Deliberately NOT added as a path dependency on `worldgen` — that crate also pulls
//! serde + ron + png for its project format, none of which the tree math needs.
//!
//! `rng.rs` is Mulberry32, i.e. the same algorithm as `tileworld_core::rng::mulberry32`. It is kept
//! as a separate copy on purpose: the generator's bit-level stream determines tree SHAPE, so
//! routing it through core's RNG would only be safe if `f32()` quantised identically
//! (`(next_u32() >> 8) as f32 / 16_777_216.0`). Not worth the coupling for zero gain.

#![allow(dead_code)]

// Vendored verbatim, so the unused surface is kept ON PURPOSE: `noise::{fbm, ridged, warped_fbm}`,
// `rng::chance` and several `TreeSkeleton` fields are dead here but are part of the upstream API.
// Pruning them would turn every future re-copy into a hand-merge — and `fbm`/`ridged` are exactly
// what a richer procedural bark would need. Allowed, not deleted.

pub mod noise;
pub mod rng;
pub mod tree;
