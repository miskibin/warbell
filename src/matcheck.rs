//! `FOREST_MATCHECK=1`: flags any mesh entity carrying MORE THAN ONE `MeshMaterial3d<T>` type.
//! Each material plugin queues the entity into the same render phase, so a doubled material is the
//! Bevy 0.20 "Binning main entity … when it was already binned" panic waiting to happen. Logs the
//! offender (name + parent chain + which types) the frame it appears.
//!
//! `FOREST_VERTREPORT=1`: a census of the index counts the main camera draws, by mesh kind (below).

use bevy::prelude::*;

pub struct MatCheckPlugin;

impl Plugin for MatCheckPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("FOREST_MATCHECK").is_some() {
            app.add_systems(Last, check);
        }
        if std::env::var_os("FOREST_WHOIS").is_some() {
            app.add_systems(Last, whois);
        }
        if std::env::var_os("FOREST_HIDEMAT").is_some() {
            app.add_systems(PostUpdate, hide_mat);
        }
        if std::env::var_os("FOREST_VERTREPORT").is_some() {
            app.init_resource::<VertCensus>().add_systems(PostUpdate, (census_added, census_report).chain())
                .add_systems(Last, (asset_churn, transform_churn))
                .add_systems(Last, tf_signatures);
        }
    }
}

type M<T> = MeshMaterial3d<T>;

#[allow(clippy::type_complexity)]
fn check(
    q: Query<(
        Entity,
        (
            Has<M<StandardMaterial>>,
            Has<M<crate::creature::CreatureMaterial>>,
            Has<M<crate::vista::FallsMaterial>>,
            Has<M<crate::combat_marks::MarkMaterial>>,
            Has<M<crate::realforest::reskin::ReskinMaterial>>,
            Has<M<crate::terrain::TerrainMaterial>>,
            Has<M<crate::water::WaterMaterial>>,
            Has<M<crate::realforest::foliage::FoliageMaterial>>,
            Has<M<crate::alpine::snowkit::SnowKitMaterial>>,
            Has<M<crate::clouds::CloudSkyMaterial>>,
            Has<M<crate::combat_fx::HpBarMaterial>>,
        ),
    ), With<Mesh3d>>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut seen: Local<bevy::platform::collections::HashSet<Entity>>,
) {
    const N: [&str; 11] = ["Standard", "Creature", "Falls", "Mark", "Reskin", "Terrain", "Water", "Foliage", "SnowKit", "CloudSky", "HpBar"];
    for (e, h) in &q {
        let f = [h.0, h.1, h.2, h.3, h.4, h.5, h.6, h.7, h.8, h.9, h.10];
        if f.iter().filter(|b| **b).count() < 2 || !seen.insert(e) {
            continue;
        }
        let types: Vec<&str> = f.iter().zip(N).filter(|(b, _)| **b).map(|(_, n)| n).collect();
        let mut chain = Vec::new();
        let mut cur = Some(e);
        while let Some(c) = cur {
            chain.push(names.get(c).map(|n| n.as_str().to_string()).unwrap_or_else(|_| format!("{c}")));
            cur = parents.get(c).ok().map(|p| p.parent());
            if chain.len() > 8 {
                break;
            }
        }
        warn!("MATCHECK: {e} has {types:?} — chain {}", chain.join(" < "));
    }
}

/// `FOREST_WHOIS=<entity index>`: log that mesh entity's name, parent chain and component list the
/// first frame it exists (to identify an entity named in a render-world panic message).
pub fn whois(world: &mut World, mut done: Local<bool>) {
    let Some(want) = std::env::var("FOREST_WHOIS").ok() else { return };
    if *done {
        return;
    }
    let prefix = format!("{want}v");
    let mut q = world.query::<Entity>();
    let Some(e) = q.iter(world).find(|e| format!("{e}").starts_with(&prefix)) else { return };
    *done = true;
    let comps: Vec<String> = world
        .inspect_entity(e)
        .map(|it| it.map(|(_, c)| { let n = format!("{:?}", c.name()); n.rsplit("::").next().unwrap_or("").trim_matches('"').to_string() }).collect())
        .unwrap_or_default();
    let mut chain = Vec::new();
    let mut cur = Some(e);
    while let Some(c) = cur {
        chain.push(world.get::<Name>(c).map(|n| n.as_str().to_string()).unwrap_or_else(|| format!("{c}")));
        cur = world.get::<ChildOf>(c).map(|p| p.parent());
    }
    let mesh = world.get::<Mesh3d>(e).map(|m| format!("{:?}", m.0.path()));
    let root = chain_root(world, e);
    let rc: Vec<String> = world
        .inspect_entity(root)
        .map(|it| it.map(|(_, c)| { let n = format!("{:?}", c.name()); n.rsplit("::").next().unwrap_or("").trim_matches('"').to_string() }).collect())
        .unwrap_or_default();
    warn!("WHOIS root {root}: comps {rc:?}");
    warn!("WHOIS {e}: chain {} | mesh {mesh:?} | comps {comps:?}", chain.join(" < "));
}

fn chain_root(world: &World, mut e: Entity) -> Entity {
    while let Some(p) = world.get::<ChildOf>(e) {
        e = p.parent();
    }
    e
}

/// `FOREST_VERTREPORT=1`: every 5 s, log the index count of all mesh entities the camera can see,
/// bucketed by what they are (material type, ground-cover flag, root entity name) — to find which
/// meshes the GPU's vertex work actually goes to. Counts are captured when each mesh entity appears
/// (GPU-only meshes are dropped from the main world after upload).
#[derive(Resource, Default)]
struct VertCensus {
    per: bevy::platform::collections::HashMap<Entity, (String, u32)>,
    last: f32,
}

#[allow(clippy::type_complexity)]
fn census_added(
    q: Query<(Entity, &Mesh3d, Has<M<StandardMaterial>>, Has<M<crate::realforest::foliage::FoliageMaterial>>, Has<M<crate::terrain::TerrainMaterial>>, Has<crate::biome::GroundCoverChunk>, Has<bevy::light::NotShadowCaster>, Option<&bevy::camera::visibility::VisibilityRange>, Has<bevy::mesh::skinning::SkinnedMesh>), Added<Mesh3d>>,
    meshes: Res<Assets<Mesh>>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    roots: Query<(Has<crate::verbs::ChopTree>, Has<crate::realforest::impostor::ImpostorTree>, Has<crate::wind::Sway>, &Transform)>,
    mut c: ResMut<VertCensus>,
) {
    for (e, m, std, fol, ter, cover, nsc, vr, skin) in &q {
        let Some(mesh) = meshes.get(&m.0) else { continue };
        // GPU-only meshes (merged chunks, impostors) may already be extracted: skip, don't panic.
        let n = match mesh.try_indices_option() {
            Ok(Some(i)) => i.len() as u32,
            Ok(None) => mesh.count_vertices() as u32,
            Err(_) => continue,
        };
        let mut root = e;
        while let Ok(p) = parents.get(root) {
            root = p.parent();
        }
        let rn = names.get(root).map(|n| n.as_str().split(['(', ' ', '#']).next().unwrap_or("").to_string()).unwrap_or_default();
        let mat = if std { "std" } else if fol { "foliage" } else if ter { "terrain" } else { "other" };
        let rng = vr.map_or("".to_string(), |r| format!(" R{:.0}", r.end_margin.end));
        let tag = format!("{mat}{}{}{}{rng} root={}", if skin { " SKIN" } else { "" }, if cover { " COVER" } else { "" }, if nsc { "" } else { " +shadow" }, if root == e { "-".into() } else { rn });
        // `FOREST_VERTSAMPLE=<tag substring>`: log a few entities of that bucket, for FOREST_WHOIS.
        if let Ok(want) = std::env::var("FOREST_VERTSAMPLE") {
            static LOGGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if tag.contains(want.as_str()) && LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
                info!("VERTSAMPLE {e} root {root} n={n} tag {tag} root(chop,imp,sway,tf)={:?}", roots.get(root).ok());
            }
        }
        c.per.insert(e, (tag, n));
    }
}

fn census_report(aplayers: Query<&AnimationPlayer, With<AnimationGraphHandle>>, players: Query<(Entity, Has<AnimationGraphHandle>), With<AnimationPlayer>>, targets: Query<&bevy::animation::AnimatedBy>, names: Query<&Name>, parents: Query<&ChildOf>, time: Res<Time<Real>>, vis: Query<(Entity, &ViewVisibility)>, gt: Query<&GlobalTransform>, cam: Query<&GlobalTransform, With<Camera3d>>, cam_vis: Query<&bevy::camera::visibility::VisibleEntities, With<Camera3d>>, mut c: ResMut<VertCensus>) {
    let main_vis: bevy::platform::collections::HashSet<Entity> = cam_vis.single().map(|v| v.iter(std::any::TypeId::of::<Mesh3d>()).copied().collect()).unwrap_or_default();
    let now = time.elapsed_secs();
    if now - c.last < 5.0 {
        return;
    }
    c.last = now;
    let mut sum: bevy::platform::collections::HashMap<String, (u64, u32)> = Default::default();
    let mut total = 0u64;
    c.per.retain(|e, _| vis.contains(*e));
    for (e, (tag, n)) in &c.per {
        if main_vis.contains(e) {
            let s = sum.entry(tag.clone()).or_default();
            s.0 += *n as u64;
            s.1 += 1;
            total += *n as u64;
        }
    }
    let mut v: Vec<_> = sum.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let top: Vec<String> = v.iter().take(14).map(|(t, (n, k))| format!("{:.2}M/{k} {t}", *n as f64 / 1e6)).collect();
    if let Ok(cp) = cam.single() {
        let mut hist = [0u32; 8];
        for (e, (tag, _)) in &c.per {
            if tag.starts_with("std COVER") && main_vis.contains(e) {
                if let Ok(g) = gt.get(*e) {
                    let d = g.translation().xz().distance(cp.translation().xz());
                    hist[((d / 20.0) as usize).min(7)] += 1;
                }
            }
        }
        info!("VERTS std COVER visible chunks by 20u distance band from camera {:?} (cam {:?})", hist, cp.translation());
    }
    let mut all: bevy::platform::collections::HashMap<String, u32> = Default::default();
    for (_, (tag, _)) in &c.per {
        *all.entry(tag.clone()).or_default() += 1;
    }
    let mut av: Vec<_> = all.into_iter().collect();
    av.sort_by(|a, b| b.1.cmp(&a.1));
    info!("MESHES all (entities by kind, {} total): {}", c.per.len(), av.iter().take(16).map(|(t, n)| format!("{n} {t}")).collect::<Vec<_>>().join(" | "));
    let mut tcount: bevy::platform::collections::HashMap<Entity, u32> = Default::default();
    for t in &targets {
        *tcount.entry(t.0).or_default() += 1;
    }
    let mut pl: bevy::platform::collections::HashMap<String, (u32, u32, u32)> = Default::default();
    for (pe, graph) in &players {
        let mut root = pe;
        while let Ok(p) = parents.get(root) {
            root = p.parent();
        }
        let rn = names.get(root).map(|n| n.as_str().chars().take(18).collect::<String>()).unwrap_or_else(|_| "?".into());
        let e = pl.entry(rn).or_default();
        e.0 += 1;
        if graph {
            e.1 += 1;
            e.2 += tcount.get(&pe).copied().unwrap_or(0);
        }
    }
    let mut pv: Vec<_> = pl.into_iter().collect();
    pv.sort_by(|a, b| b.1 .2.cmp(&a.1 .2));
    let mut hist = [0u32; 8];
    let mut zero_w = 0u32;
    for p in &aplayers {
        let n = p.playing_animations().count();
        hist[n.min(7)] += 1;
        zero_w += p.playing_animations().filter(|(_, a)| a.weight() <= 0.0).count() as u32;
    }
    info!("ANIM active-animation count per graphed player (0..7+): {hist:?}, zero-weight actives {zero_w}");
    info!("ANIM players (total/with graph/animated targets): {}", pv.iter().take(12).map(|(n, (a, g, t))| format!("{n}: {a}/{g}/{t}")).collect::<Vec<_>>().join(" | "));
    info!("VERTS visible {:.2}M idx | {}", total as f64 / 1e6, top.join(" | "));
}

/// `FOREST_HIDEMAT=std,reskin,…`: hide every mesh of the listed material types (profiling A/B:
/// what a material category costs on the GPU). Keys as in [`check`]'s list, lower-case.
#[allow(clippy::type_complexity)]
fn hide_mat(
    mut q: Query<(
        &mut Visibility,
        (
            Has<M<StandardMaterial>>,
            Has<M<crate::creature::CreatureMaterial>>,
            Has<M<crate::vista::FallsMaterial>>,
            Has<M<crate::combat_marks::MarkMaterial>>,
            Has<M<crate::realforest::reskin::ReskinMaterial>>,
            Has<M<crate::terrain::TerrainMaterial>>,
            Has<M<crate::water::WaterMaterial>>,
            Has<M<crate::realforest::foliage::FoliageMaterial>>,
            Has<M<crate::alpine::snowkit::SnowKitMaterial>>,
            Has<M<crate::clouds::CloudSkyMaterial>>,
        ),
        Has<bevy::mesh::skinning::SkinnedMesh>,
    ), With<Mesh3d>>,
) {
    const N: [&str; 10] = ["std", "creature", "falls", "mark", "reskin", "terrain", "water", "foliage", "snowkit", "cloudsky"];
    static WANT: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let want = WANT.get_or_init(|| std::env::var("FOREST_HIDEMAT").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect());
    for (mut vis, h, skin) in &mut q {
        let f = [h.0, h.1, h.2, h.3, h.4, h.5, h.6, h.7, h.8, h.9];
        let hit = f.iter().zip(N).any(|(b, n)| *b && want.iter().any(|w| w == n))
            || (skin && want.iter().any(|w| w == "skin"))
            || (h.0 && !skin && want.iter().any(|w| w == "stdnoskin"));
        if hit && *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
    }
}

/// Per-frame asset mutations: any `Modified` mesh / material in a frame makes Bevy's
/// `check_entities_needing_specialization` hash-check EVERY entity of every material type.
#[allow(clippy::too_many_arguments)]
fn asset_churn(
    mut mesh: MessageReader<AssetEvent<Mesh>>,
    mut std: MessageReader<AssetEvent<StandardMaterial>>,
    mut fol: MessageReader<AssetEvent<crate::realforest::foliage::FoliageMaterial>>,
    mut cre: MessageReader<AssetEvent<crate::creature::CreatureMaterial>>,
    mut ter: MessageReader<AssetEvent<crate::terrain::TerrainMaterial>>,
    mut img: MessageReader<AssetEvent<Image>>,
    time: Res<Time<Real>>,
    mut acc: Local<([u32; 6], u32, f32)>,
    mut ids: Local<bevy::platform::collections::HashMap<AssetId<StandardMaterial>, u32>>,
    mut mids: Local<bevy::platform::collections::HashMap<AssetId<Mesh>, u32>>,
    users: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
    smats: Res<Assets<StandardMaterial>>,
    musers: Query<(Entity, &Mesh3d)>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
) {
    let chain = |e: Entity| {
        let mut v = Vec::new();
        let mut cur = Some(e);
        while let Some(c) = cur {
            v.push(names.get(c).map(|n| n.as_str().to_string()).unwrap_or_else(|_| format!("{c}")));
            cur = parents.get(c).ok().map(|p| p.parent());
            if v.len() > 5 { break; }
        }
        v.join(" < ")
    };
    fn m<A: Asset>(r: &mut MessageReader<AssetEvent<A>>) -> u32 {
        r.read().filter(|e| matches!(e, AssetEvent::Modified { .. })).count() as u32
    }
    let mut n = [0, 0, m(&mut fol), m(&mut cre), m(&mut ter), m(&mut img)];
    for e in std.read() {
        if let AssetEvent::Modified { id } = e {
            n[1] += 1;
            *ids.entry(*id).or_default() += 1;
        }
    }
    for e in mesh.read() {
        if let AssetEvent::Modified { id } = e {
            n[0] += 1;
            *mids.entry(*id).or_default() += 1;
        }
    }
    for i in 0..6 {
        acc.0[i] += n[i];
    }
    acc.1 += 1;
    if time.elapsed_secs() - acc.2 > 5.0 {
        acc.2 = time.elapsed_secs();
        let f = acc.1.max(1) as f32;
        info!("CHURN modified/frame: mesh {:.1} std {:.1} foliage {:.1} creature {:.1} terrain {:.1} image {:.1}", acc.0[0] as f32 / f, acc.0[1] as f32 / f, acc.0[2] as f32 / f, acc.0[3] as f32 / f, acc.0[4] as f32 / f, acc.0[5] as f32 / f);
        let mut top: Vec<_> = ids.drain().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        for (id, k) in top.iter().take(8) {
            let us: Vec<Entity> = users.iter().filter(|(_, m)| m.0.id() == *id).map(|(e, _)| e).collect();
            let desc = smats.get(*id).map(|m| format!("base {:?} emis {:?} alpha {:?} unlit {} tex {}", m.base_color.to_srgba(), m.emissive, m.alpha_mode, m.unlit, m.base_color_texture.is_some())).unwrap_or_default();
            info!("CHURN std {id:?} x{k}: {} users, e.g. {} | {desc}", us.len(), us.first().map(|e| chain(*e)).unwrap_or_default());
        }
        let mut mt: Vec<_> = mids.drain().collect();
        mt.sort_by(|a, b| b.1.cmp(&a.1));
        for (id, k) in mt.iter().take(4) {
            let us: Vec<Entity> = musers.iter().filter(|(_, m)| m.0.id() == *id).map(|(e, _)| e).collect();
            info!("CHURN mesh {id:?} x{k}: {} users, e.g. {}", us.len(), us.first().map(|e| chain(*e)).unwrap_or_default());
        }
        *acc = ([0; 6], 0, acc.2);
    }
}

/// Entities whose `Transform` changed this frame, bucketed by a marker of their root (each changed
/// entity dirties its whole subtree for `propagate_parent_transforms`).
#[allow(clippy::type_complexity)]
fn transform_churn(
    q: Query<(Entity, Option<&Children>), Changed<Transform>>,
    parents: Query<&ChildOf>,
    kinds: Query<(Has<crate::wind::Sway>, Has<crate::verbs::ChopTree>, Has<bevy::animation::AnimationTargetId>, Has<Mesh3d>, Option<&Name>)>,
    desc: Query<&Children>,
    time: Res<Time<Real>>,
    mut acc: Local<(bevy::platform::collections::HashMap<String, (u32, u32)>, u32, f32)>,
) {
    for (e, ch) in &q {
        let mut root = e;
        while let Ok(p) = parents.get(root) {
            root = p.parent();
        }
        let tag = match kinds.get(e) {
            Ok((true, _, _, _, _)) => "sway".to_string(),
            Ok((_, true, _, _, _)) => "choptree".to_string(),
            Ok((_, _, true, _, _)) => "bone".to_string(),
            Ok((_, _, _, m, n)) => format!("{}{}", if m { "mesh:" } else { "node:" }, n.map(|n| n.as_str().chars().take(14).collect::<String>()).unwrap_or_default()),
            _ => "?".into(),
        };
        let sub = ch.map_or(0, |_| desc.iter_descendants(e).count() as u32);
        let s = acc.0.entry(tag).or_default();
        s.0 += 1;
        s.1 += sub;
    }
    acc.1 += 1;
    if time.elapsed_secs() - acc.2 > 5.0 {
        acc.2 = time.elapsed_secs();
        let f = acc.1.max(1) as f32;
        let mut v: Vec<_> = acc.0.drain().collect();
        v.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
        info!("TFCHURN per frame (changed / +descendants dirtied): {}", v.iter().take(12).map(|(t, (n, d))| format!("{t} {:.0}/+{:.0}", *n as f32 / f, *d as f32 / f)).collect::<Vec<_>>().join(" | "));
        acc.1 = 0;
    }
}

/// Once, ~15 s in: group the mesh / node entities whose `Transform` changed over one frame by their
/// component set, to name who rewrites transforms every frame.
fn tf_signatures(world: &mut World, mut st: Local<Option<QueryState<Entity, Changed<Transform>>>>, mut step: Local<u32>) {
    let t = world.resource::<Time<Real>>().elapsed_secs();
    if t < 15.0 || *step >= 2 {
        return;
    }
    let q = st.get_or_insert_with(|| world.query_filtered::<Entity, Changed<Transform>>());
    let es: Vec<Entity> = q.iter(world).collect();
    *step += 1;
    if *step < 2 {
        return; // first pass only primes the change ticks
    }
    // (entities, descendants dirtied) per component signature: a changed root dirties its subtree.
    let mut sig: bevy::platform::collections::HashMap<String, (u32, u32)> = Default::default();
    for e in es {
        let sub = world.get::<Children>(e).map_or(0, |_| {
            let mut n = 0u32;
            let mut stack = vec![e];
            while let Some(x) = stack.pop() {
                if let Some(ch) = world.get::<Children>(x) {
                    n += ch.len() as u32;
                    stack.extend(ch.iter());
                }
            }
            n
        });
        let Ok(it) = world.inspect_entity(e) else { continue };
        let mut names: Vec<String> = it
            .map(|(_, c)| { let n = format!("{:?}", c.name()); n.rsplit("::").next().unwrap_or("").trim_matches(['"', '>', ')']).to_string() })
            .filter(|n| !matches!(n.as_str(), "Transform" | "GlobalTransform" | "Visibility" | "InheritedVisibility" | "ViewVisibility" | "TransformTreeChanged" | "ChildOf" | "Children" | "VisibilityClass" | "Aabb" | "Name"))
            .collect();
        names.sort();
        let s = sig.entry(names.join(",")).or_default();
        s.0 += 1;
        s.1 += sub;
    }
    let mut v: Vec<_> = sig.into_iter().collect();
    v.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
    for (s, (n, d)) in v.iter().take(25) {
        info!("TFSIG {n:5} +{d:6}  {}", &s[..s.len().min(260)]);
    }
}
