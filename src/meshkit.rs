//! Shared low-poly mesh helpers — the "tint → merge → flat-shade" trio every prop/model builder
//! composes from. These were copy-pasted (byte-identical) into ~30 modules; consolidated here so
//! there is one home for the mesh-building contract.
//!
//! Build contract (verified-APIs doc §9): each part is a primitive [`tinted`] with a flat linear
//! `ATTRIBUTE_COLOR` BEFORE the merge (the scene's shared white `StandardMaterial` reads colour
//! straight off the vertices, so thousands of props auto-batch), merged via `Mesh::merge`, then
//! [`flat_shaded`] (`duplicate_vertices` → `compute_flat_normals`, in that order — the latter
//! panics on an indexed mesh). [`merged_flat`] is the common merge-then-flat-shade combo.

use crate::palette::lin;
use bevy::prelude::*;

/// Tag every vertex of a part with one flat linear RGBA colour. REQUIRED before a merge — all
/// merged parts must carry the same attribute set, and the shared white material reads colour
/// straight off `ATTRIBUTE_COLOR`.
pub fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// [`tinted`] from a packed `0xRRGGBB` hex, converted to linear via [`crate::palette::lin`].
pub fn tinted_hex(m: Mesh, hex: u32) -> Mesh {
    tinted(m, lin(hex))
}

/// Merge already-tinted parts into one mesh. Does NOT re-shade — call [`flat_shaded`] after, or
/// use [`merged_flat`] for the common combine-and-facet.
pub fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base
}

/// Un-index + recompute per-face normals → crisp flat-shaded low-poly facets.
/// `duplicate_vertices()` MUST run before `compute_flat_normals()`. Call LAST, on the merge.
pub fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// [`merged`] followed by [`flat_shaded`] — the common combine-and-facet combo.
pub fn merged_flat(parts: Vec<Mesh>) -> Mesh {
    flat_shaded(merged(parts))
}
