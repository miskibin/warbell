//! Parametric tree skeleton generator (simplified Weber–Penn). Pure geometry — the Bevy
//! side sweeps tubes along segments and pastes leaf cards at anchors. Deterministic per
//! (species, seed).
//!
//! Species read differently through STRUCTURE, not just texture:
//! - Pine: bare straight bole, irregular upswept crown near the top.
//! - Spruce: full cone — whorled branches from near the ground, longest at the base,
//!   drooping with upturned tips.
//! - Broadleaf (beech/oak): short bole splitting into scaffold limbs, rounded canopy.
//! - Birch: slender, slightly leaning bole, thin ascending branches with drooping tips.

use super::rng::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Species {
    Pine,
    Spruce,
    Broadleaf,
    Birch,
}

pub const ALL_SPECIES: [Species; 4] =
    [Species::Pine, Species::Spruce, Species::Broadleaf, Species::Birch];

#[derive(Clone, Copy)]
pub struct Segment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub ra: f32,
    pub rb: f32,
    /// 0 = trunk, 1 = scaffold/whorl branch, 2 = twig.
    pub level: u8,
}

#[derive(Clone, Copy)]
pub struct LeafAnchor {
    pub pos: [f32; 3],
    /// Outward normal-ish direction for the card.
    pub dir: [f32; 3],
    pub size: f32,
}

pub struct TreeSkeleton {
    pub species: Species,
    pub segments: Vec<Segment>,
    pub leaves: Vec<LeafAnchor>,
    pub height: f32,
    pub canopy_center: [f32; 3],
    pub canopy_radius: f32,
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn norm3(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6);
    scale3(a, 1.0 / l)
}

/// A direction at `angle` from `axis`, rotated `azimuth` around it.
fn cone_dir(axis: [f32; 3], angle: f32, azimuth: f32) -> [f32; 3] {
    let axis = norm3(axis);
    // Build any orthonormal frame around axis.
    let helper = if axis[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let u = norm3([
        axis[1] * helper[2] - axis[2] * helper[1],
        axis[2] * helper[0] - axis[0] * helper[2],
        axis[0] * helper[1] - axis[1] * helper[0],
    ]);
    let v = [
        axis[1] * u[2] - axis[2] * u[1],
        axis[2] * u[0] - axis[0] * u[2],
        axis[0] * u[1] - axis[1] * u[0],
    ];
    let (sa, ca) = angle.sin_cos();
    let (sz, cz) = azimuth.sin_cos();
    norm3(add3(
        scale3(axis, ca),
        add3(scale3(u, sa * cz), scale3(v, sa * sz)),
    ))
}

/// Walk an axis in `nseg` segments, applying per-segment random gnarl and a constant
/// tropism pull (up or down). Pushes segments, returns sample points (pos, dir, t).
#[allow(clippy::too_many_arguments)]
fn grow_axis(
    out: &mut Vec<Segment>,
    rng: &mut Rng,
    start: [f32; 3],
    dir: [f32; 3],
    len: f32,
    r0: f32,
    r1: f32,
    level: u8,
    nseg: usize,
    gnarl: f32,
    tropism: f32, // +up, -down, applied per segment
) -> Vec<([f32; 3], [f32; 3], f32)> {
    let mut pos = start;
    let mut d = norm3(dir);
    let step = len / nseg as f32;
    let mut samples = Vec::with_capacity(nseg + 1);
    for i in 0..nseg {
        let t0 = i as f32 / nseg as f32;
        let t1 = (i + 1) as f32 / nseg as f32;
        samples.push((pos, d, t0));
        // EZ-Tree trick: gnarl amplitude scales ~1/sqrt(radius), so thin twigs wander
        // hard while the trunk stays stately — a big chunk of "real tree" silhouette.
        let r_cur = (r0 + (r1 - r0) * t0).max(1e-3);
        let g = gnarl * (1.0 / r_cur.sqrt()).clamp(1.0, 3.0);
        d = norm3(add3(
            d,
            [rng.signed() * g, tropism + rng.signed() * g * 0.4, rng.signed() * g],
        ));
        let next = add3(pos, scale3(d, step));
        out.push(Segment {
            a: pos,
            b: next,
            ra: r0 + (r1 - r0) * t0,
            rb: r0 + (r1 - r0) * t1,
            level,
        });
        pos = next;
    }
    samples.push((pos, d, 1.0));
    samples
}

pub fn grow(species: Species, seed: u32) -> TreeSkeleton {
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9).wrapping_add(species as u32 * 7919));
    let mut segments = Vec::new();
    let mut leaves = Vec::new();

    match species {
        Species::Pine => {
            let h = rng.range(17.0, 25.0);
            let r = h * 0.021;
            let lean = [rng.signed() * 0.03, 1.0, rng.signed() * 0.03];
            let trunk =
                grow_axis(&mut segments, &mut rng, [0.0; 3], lean, h, r, r * 0.06, 0, 10, 0.05, 0.012);
            // Irregular crown: branches only on the top ~40% of the bole.
            let crown_start = rng.range(0.55, 0.68);
            let n_branches = rng.next_u32() % 5 + 9;
            for _ in 0..n_branches {
                let t = rng.range(crown_start, 0.97);
                let (bp, bd, _) = trunk[(t * 10.0) as usize];
                // High branches shorter + steeper up; low crown branches flatter.
                let up = (t - crown_start) / (1.0 - crown_start);
                let angle = (1.35 - up * 0.75) + rng.signed() * 0.15;
                let blen = h * (0.16 + (1.0 - up) * 0.10) * rng.range(0.8, 1.2);
                let bdir = cone_dir(bd, angle, rng.range(0.0, std::f32::consts::TAU));
                let br = r * 0.22 * (0.5 + 0.5 * (1.0 - up));
                let tips = grow_axis(
                    &mut segments, &mut rng, bp, bdir, blen, br, br * 0.15, 1, 4, 0.10, 0.020,
                );
                // Needle sprays on the outer half of each crown branch.
                for &(p, d, t) in &tips {
                    if t > 0.4 {
                        leaves.push(LeafAnchor {
                            pos: p,
                            dir: norm3(add3(d, [0.0, 0.35, 0.0])),
                            size: h * rng.range(0.09, 0.15),
                        });
                    }
                }
            }
        }
        Species::Spruce => {
            let h = rng.range(16.0, 24.0);
            let r = h * 0.020;
            let trunk = grow_axis(
                &mut segments, &mut rng, [0.0; 3], [0.0, 1.0, 0.0], h, r, r * 0.03, 0, 12, 0.025,
                0.015,
            );
            // Whorls from near the ground to the tip; branch length tapers to the cone.
            let n_whorls = 11 + (rng.next_u32() % 3) as usize;
            for w in 0..n_whorls {
                let t = 0.10 + 0.86 * w as f32 / (n_whorls - 1) as f32;
                let (bp, bd, _) = trunk[(t * 12.0) as usize];
                let cone = 1.0 - t; // 1 at base, 0 at tip
                let blen = h * 0.185 * cone.max(0.12) * rng.range(0.85, 1.15);
                let per = 5 + (rng.next_u32() % 3);
                for _ in 0..per {
                    // Droop: branch leaves the trunk near-horizontal, sags, tip turns up
                    // — approximated by a downward tropism + a slightly upward launch.
                    let bdir =
                        cone_dir(bd, 1.42 + rng.signed() * 0.08, rng.range(0.0, std::f32::consts::TAU));
                    let br = r * 0.16 * (0.4 + 0.6 * cone);
                    let tips = grow_axis(
                        &mut segments, &mut rng, bp, bdir, blen, br, br * 0.12, 1, 3, 0.06, -0.045,
                    );
                    for &(p, d, tt) in &tips {
                        if tt > 0.4 {
                            leaves.push(LeafAnchor {
                                pos: p,
                                dir: norm3(add3(d, [0.0, -0.25, 0.0])),
                                size: h * rng.range(0.06, 0.11) * (0.5 + 0.6 * cone),
                            });
                        }
                    }
                }
            }
        }
        Species::Broadleaf => {
            // Oak/beech habit: ONE continuous trunk with scaffold limbs distributed
            // along its upper 55% (the old bole-splits-into-candelabra read as fake),
            // lower limbs wide and long, upper steep and short → rounded crown.
            let h = rng.range(13.0, 19.0);
            let r = h * 0.028;
            let lean = [rng.signed() * 0.05, 1.0, rng.signed() * 0.05];
            let trunk = grow_axis(
                &mut segments, &mut rng, [0.0; 3], lean,
                h * 0.72, r, r * 0.10, 0, 8, 0.045, 0.012,
            );
            let n_scaffold = 6 + (rng.next_u32() % 4);
            for s in 0..n_scaffold {
                let t = 0.45 + 0.53 * (s as f32 + rng.f32() * 0.8) / n_scaffold as f32;
                let (bp, bd, _) = trunk[((t * 8.0) as usize).min(8)];
                let k = (t - 0.45) / 0.53; // 0 low … 1 top
                // Radial slot + jitter (decorrelated from height, EZ permutation idea).
                let az = s as f32 * 2.39996 + rng.signed() * 0.5; // golden angle
                let sdir = cone_dir(bd, 1.15 - 0.55 * k + rng.signed() * 0.12, az);
                let slen = h * 0.42 * (1.0 - 0.45 * k) * rng.range(0.85, 1.15);
                let sr = r * 0.42 * (1.0 - 0.35 * k);
                let limb = grow_axis(
                    &mut segments, &mut rng, bp, sdir, slen, sr, sr * 0.08, 1, 5, 0.10, 0.030,
                );
                for &(p, d, tt) in &limb {
                    if tt > 0.55 {
                        leaves.push(LeafAnchor {
                            pos: p,
                            dir: d,
                            size: h * rng.range(0.12, 0.22),
                        });
                    }
                }
                let n_twigs = 2 + (rng.next_u32() % 3);
                for _ in 0..n_twigs {
                    let tt = rng.range(0.45, 0.95);
                    let (tp, td, _) = limb[((tt * 5.0) as usize).min(5)];
                    let tdir2 =
                        cone_dir(td, rng.range(0.45, 0.95), rng.range(0.0, std::f32::consts::TAU));
                    let tlen = slen * rng.range(0.30, 0.50);
                    let tips = grow_axis(
                        &mut segments, &mut rng, tp, tdir2, tlen, sr * 0.25, sr * 0.03, 2, 3,
                        0.14, 0.035,
                    );
                    for &(p, d, t3) in &tips {
                        if t3 > 0.35 {
                            leaves.push(LeafAnchor {
                                pos: p,
                                dir: d,
                                size: h * rng.range(0.11, 0.21),
                            });
                        }
                    }
                }
            }
        }
        Species::Birch => {
            let h = rng.range(14.0, 20.0);
            let r = h * 0.016;
            let lean = [rng.signed() * 0.06, 1.0, rng.signed() * 0.06];
            let trunk = grow_axis(
                &mut segments, &mut rng, [0.0; 3], lean,
                h, r, r * 0.05, 0, 10, 0.045, 0.010,
            );
            let start = rng.range(0.35, 0.45);
            let n_branches = 10 + (rng.next_u32() % 5);
            for _ in 0..n_branches {
                let t = rng.range(start, 0.96);
                let (bp, bd, _) = trunk[(t * 10.0) as usize];
                let up = (t - start) / (1.0 - start);
                // Ascending launch, then strong droop — the birch "weeping tip" look.
                let bdir = cone_dir(bd, 0.85 - up * 0.35 + rng.signed() * 0.1,
                    rng.range(0.0, std::f32::consts::TAU));
                let blen = h * (0.20 - up * 0.08) * rng.range(0.8, 1.2);
                let br = r * 0.30 * (0.6 + 0.4 * (1.0 - up));
                let tips = grow_axis(
                    &mut segments, &mut rng, bp, bdir, blen, br, br * 0.10, 1, 4, 0.10, -0.060,
                );
                for &(p, d, tt) in &tips {
                    if tt > 0.55 {
                        leaves.push(LeafAnchor {
                            pos: p,
                            dir: d,
                            size: h * rng.range(0.08, 0.14),
                        });
                    }
                }
            }
        }
    }

    // NB: no anchor densification and no dir-bending here — each anchor now carries a
    // WHOLE photographic sprig (base-pivot card growing along the branch tangent), and
    // "soft volume" lighting comes from per-vertex rounded normals in the mesh builder.
    let (mut cx, mut cy, mut cz) = (0.0f32, 0.0f32, 0.0f32);
    for l in &leaves {
        cx += l.pos[0];
        cy += l.pos[1];
        cz += l.pos[2];
    }
    let n = leaves.len().max(1) as f32;
    let center = [cx / n, cy / n, cz / n];
    let mut radius = 0.0f32;
    for l in &leaves {
        let out =
            [l.pos[0] - center[0], l.pos[1] - center[1] + 0.5, l.pos[2] - center[2]];
        radius = radius.max((out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt());
    }

    let height = segments
        .iter()
        .map(|s| s.b[1])
        .fold(0.0f32, f32::max);

    TreeSkeleton { species, segments, leaves, height, canopy_center: center, canopy_radius: radius }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        for sp in ALL_SPECIES {
            let a = grow(sp, 5);
            let b = grow(sp, 5);
            assert_eq!(a.segments.len(), b.segments.len());
            assert_eq!(a.leaves.len(), b.leaves.len());
            for (x, y) in a.segments.iter().zip(&b.segments) {
                assert_eq!(x.a, y.a);
                assert_eq!(x.b, y.b);
            }
        }
    }

    #[test]
    fn radii_taper_and_finite() {
        for sp in ALL_SPECIES {
            for seed in 0..8 {
                let t = grow(sp, seed);
                for s in &t.segments {
                    assert!(s.ra.is_finite() && s.rb.is_finite());
                    assert!(s.ra > 0.0 && s.rb > 0.0, "{sp:?} radius <= 0");
                    assert!(s.rb <= s.ra + 1e-4, "{sp:?} radius grows down a segment");
                    for v in s.a.iter().chain(s.b.iter()) {
                        assert!(v.is_finite());
                    }
                }
            }
        }
    }

    #[test]
    fn species_shapes_sane() {
        for sp in ALL_SPECIES {
            for seed in 0..8 {
                let t = grow(sp, seed);
                assert!(t.height > 10.0 && t.height < 30.0, "{sp:?} height {}", t.height);
                // Sprig-card era: each anchor is a WHOLE photographic twig, so healthy
                // counts are tens, not hundreds (EZ-Tree ships full oaks at ~200 sprigs).
                assert!(t.leaves.len() > 18, "{sp:?} only {} leaves", t.leaves.len());
                assert!(t.leaves.len() < 800, "{sp:?} leaf explosion");
                assert!(t.segments.len() > 10 && t.segments.len() < 3000);
                assert!(t.canopy_radius > 0.5 && t.canopy_radius.is_finite());
            }
        }
    }

    #[test]
    fn spruce_cone_wider_at_base() {
        // Spruce: mean branch-tip distance from axis should be larger low than high.
        let t = grow(Species::Spruce, 3);
        let h = t.height;
        let (mut low, mut nlow, mut high, mut nhigh) = (0.0f32, 0, 0.0f32, 0);
        for l in &t.leaves {
            let d = (l.pos[0] * l.pos[0] + l.pos[2] * l.pos[2]).sqrt();
            if l.pos[1] < h * 0.4 {
                low += d;
                nlow += 1;
            } else if l.pos[1] > h * 0.6 {
                high += d;
                nhigh += 1;
            }
        }
        assert!(nlow > 0 && nhigh > 0);
        assert!(low / nlow as f32 > high / nhigh as f32, "spruce isn't conical");
    }
}
