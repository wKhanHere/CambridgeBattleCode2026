use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};

use cambc::{Bitboard, CampType, ChampionDraft, DistanceField, Result, Team, Vec2};

const MEMO_SIZE: usize = 1024;
const INTEL_OFFSET: usize = 992;
const MAGIC_0: u8 = 0xCA;
const MAGIC_1: u8 = 0xBC;
const UNREACHABLE_DIST: u16 = u16::MAX;
const PATCH_ADAPT_DRAFT_100X: bool = true;

const TERRAIN_ROWS_MAP: u8 = 1 << 0;
const TERRAIN_LARGE_MAP: u8 = 1 << 1;
const TERRAIN_COMPACT_MAP: u8 = 1 << 2;
const TERRAIN_HEAVY_JUNGLE: u8 = 1 << 3;
const TERRAIN_CHOKEPOINT: u8 = 1 << 4;
const TERRAIN_BUSH_HEAVY: u8 = 1 << 5;
const TERRAIN_HOLE_HEAVY: u8 = 1 << 6;
const TERRAIN_DISTANT_ALTARS: u8 = 1 << 7;

const COMP_HIGH_CC: u8 = 1 << 0;
const COMP_HIGH_BURST_FOCUS: u8 = 1 << 1;
const COMP_HIGH_MELEE_DIVE: u8 = 1 << 2;
const COMP_HIGH_RANGED_POKE: u8 = 1 << 3;
const COMP_HAS_SUSTAIN_SHIELD: u8 = 1 << 4;
const COMP_HAS_ECON: u8 = 1 << 5;
const COMP_HAS_RESET_OR_SWARM: u8 = 1 << 6;
const COMP_HAS_TANK_OR_THORNS: u8 = 1 << 7;

const BIT_CASSI: u16 = 1 << 0;
const BIT_ISSAC: u16 = 1 << 1;
const BIT_RYTH: u16 = 1 << 2;
const BIT_VESPER_VALE: u16 = 1 << 3;
const BIT_PRISM: u16 = 1 << 4;
const BIT_STELLAR: u16 = 1 << 5;
const BIT_VENDETTA: u16 = 1 << 6;
const BIT_MARCELLA: u16 = 1 << 7;
const BIT_TAVI: u16 = 1 << 8;
const BIT_WILLOW: u16 = 1 << 9;
const BIT_REN: u16 = 1 << 10;
const BIT_VARG: u16 = 1 << 11;
const BIT_HEART: u16 = 1 << 12;

thread_local! {
    static BASE_MEMO: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

fn draft_to_code(pick: ChampionDraft) -> u8 {
    match pick {
        ChampionDraft::CassiAndIssac => 1,
        ChampionDraft::Ryth => 2,
        ChampionDraft::VesperVale => 3,
        ChampionDraft::Prism => 4,
        ChampionDraft::Stellar => 5,
        ChampionDraft::Vendetta => 6,
        ChampionDraft::Marcella => 7,
        ChampionDraft::Tavi => 8,
        ChampionDraft::Willow => 9,
        ChampionDraft::Ren => 10,
        ChampionDraft::Varg => 11,
        ChampionDraft::HeartOfRuin => 12,
    }
}

fn draft_to_mask(pick: ChampionDraft) -> u16 {
    match pick {
        ChampionDraft::CassiAndIssac => BIT_CASSI | BIT_ISSAC,
        ChampionDraft::Ryth => BIT_RYTH,
        ChampionDraft::VesperVale => BIT_VESPER_VALE,
        ChampionDraft::Prism => BIT_PRISM,
        ChampionDraft::Stellar => BIT_STELLAR,
        ChampionDraft::Vendetta => BIT_VENDETTA,
        ChampionDraft::Marcella => BIT_MARCELLA,
        ChampionDraft::Tavi => BIT_TAVI,
        ChampionDraft::Willow => BIT_WILLOW,
        ChampionDraft::Ren => BIT_REN,
        ChampionDraft::Varg => BIT_VARG,
        ChampionDraft::HeartOfRuin => BIT_HEART,
    }
}

fn compute_comp_flags(mask: u16) -> u8 {
    let cc_mask =
        BIT_CASSI | BIT_RYTH | BIT_PRISM | BIT_MARCELLA | BIT_TAVI | BIT_REN | BIT_HEART;
    let burst_mask =
        BIT_CASSI | BIT_PRISM | BIT_STELLAR | BIT_MARCELLA | BIT_TAVI | BIT_REN | BIT_HEART;
    let melee_mask =
        BIT_ISSAC | BIT_RYTH | BIT_VENDETTA | BIT_MARCELLA | BIT_REN | BIT_VARG | BIT_HEART;
    let ranged_mask =
        BIT_CASSI | BIT_VESPER_VALE | BIT_PRISM | BIT_STELLAR | BIT_TAVI | BIT_WILLOW;

    let mut flags = 0u8;
    if (mask & cc_mask).count_ones() >= 2 {
        flags |= COMP_HIGH_CC;
    }
    if (mask & burst_mask).count_ones() >= 2 {
        flags |= COMP_HIGH_BURST_FOCUS;
    }
    if (mask & melee_mask).count_ones() >= 2 {
        flags |= COMP_HIGH_MELEE_DIVE;
    }
    if (mask & ranged_mask).count_ones() >= 2 {
        flags |= COMP_HIGH_RANGED_POKE;
    }
    if (mask & (BIT_WILLOW | BIT_VARG | BIT_HEART)) != 0 {
        flags |= COMP_HAS_SUSTAIN_SHIELD;
    }
    if (mask & (BIT_VESPER_VALE | BIT_MARCELLA | BIT_TAVI)) != 0 {
        flags |= COMP_HAS_ECON;
    }
    if (mask & (BIT_VENDETTA | BIT_CASSI | BIT_ISSAC)) != 0 {
        flags |= COMP_HAS_RESET_OR_SWARM;
    }
    if (mask & (BIT_REN | BIT_RYTH | BIT_MARCELLA | BIT_VARG | BIT_HEART)) != 0 {
        flags |= COMP_HAS_TANK_OR_THORNS;
    }
    flags
}

struct CampScanSummary {
    monoliths: u8,
    elks: u8,
    wolves: u8,
    clusters: u8,
    three_wolf_groups: usize,
}

fn scan_camps_for_draft(width: i32, height: i32) -> CampScanSummary {
    if let Ok(engine_groups) = cambc::map::camp_groups() {
        let mut monoliths = 0u8;
        let mut elks = 0u8;
        let mut wolves = 0u8;
        let mut clusters = 0u8;
        let mut three_wolf_groups = 0usize;

        for g in &engine_groups {
            let mut wolf_in_group = 0usize;
            let mut has_monolith = false;
            for c in g.iter() {
                if c.is_monolith() {
                    has_monolith = true;
                    monoliths = monoliths.saturating_add(1);
                } else if c.is_elk() || c.is_golem() {
                    elks = elks.saturating_add(1);
                } else if c.is_wolf() {
                    wolf_in_group += 1;
                    wolves = wolves.saturating_add(1);
                } else if c.is_gem_cluster() || c.is_gold_cluster() {
                    clusters = clusters.saturating_add(1);
                }
            }
            if wolf_in_group == 3 && !has_monolith {
                three_wolf_groups += 1;
            }
        }
        return CampScanSummary {
            monoliths,
            elks,
            wolves,
            clusters,
            three_wolf_groups,
        };
    }

    let mut skip = Bitboard::new().ok();
    if let Some(ref mut s) = skip {
        if let Ok(w) = cambc::map::walls() {
            let _ = s.union_assign(w);
        }
        if let Ok(h) = cambc::map::holes() {
            let _ = s.union_assign(h);
        }
        if let Ok(a) = cambc::map::altars() {
            let _ = s.union_assign(a);
        }
        if let Ok(sa) = cambc::map::spawn_region(Team::Ally) {
            let _ = s.union_assign(sa);
        }
        if let Ok(se) = cambc::map::spawn_region(Team::Enemy) {
            let _ = s.union_assign(se);
        }
    }

    let mut camp_tiles: HashSet<(i32, i32)> = HashSet::new();
    let mut camp_kinds: Vec<(Vec2, CampType)> = Vec::new();
    let mut monoliths = 0u8;
    let mut elks = 0u8;
    let mut wolves = 0u8;
    let mut clusters = 0u8;

    'outer: for y in 0..height {
        for x in 0..width {
            if cambc::fuel_remaining().unwrap_or(0) < 120_000 {
                break 'outer;
            }
            let p = Vec2::new(x, y);
            if skip.as_ref().and_then(|s| s.is_set(p).ok()).unwrap_or(false) {
                continue;
            }
            if let Ok(c) = cambc::map::camp(p) {
                let kind = c.kind();
                match kind {
                    CampType::Monolith => monoliths = monoliths.saturating_add(1),
                    CampType::Elk | CampType::Golem => elks = elks.saturating_add(1),
                    CampType::Wolf => wolves = wolves.saturating_add(1),
                    CampType::GemCluster | CampType::GoldCluster => {
                        clusters = clusters.saturating_add(1)
                    }
                }
                camp_tiles.insert((x, y));
                camp_kinds.push((p, kind));
            }
        }
    }

    let mut visited: HashSet<(i32, i32)> = HashSet::new();
    let mut three_wolf_groups = 0usize;

    for &(start, _) in &camp_kinds {
        let key = (start.x, start.y);
        if visited.contains(&key) {
            continue;
        }
        let mut wolf_in_group = 0usize;
        let mut has_monolith = false;
        let mut q = VecDeque::new();
        visited.insert(key);
        q.push_back(start);

        while let Some(cur) = q.pop_front() {
            for &(p, kind) in &camp_kinds {
                if p == cur {
                    if kind == CampType::Wolf {
                        wolf_in_group += 1;
                    } else if kind == CampType::Monolith {
                        has_monolith = true;
                    }
                }
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nx = cur.x + dx;
                let ny = cur.y + dy;
                if camp_tiles.contains(&(nx, ny)) && !visited.contains(&(nx, ny)) {
                    visited.insert((nx, ny));
                    q.push_back(Vec2::new(nx, ny));
                }
            }
        }
        if wolf_in_group == 3 && !has_monolith {
            three_wolf_groups += 1;
        }
    }

    CampScanSummary {
        monoliths,
        elks,
        wolves,
        clusters,
        three_wolf_groups,
    }
}

fn build_base_memo() -> Vec<u8> {
    let mut memo = vec![0u8; MEMO_SIZE];

    let width = cambc::map::width().unwrap_or(32);
    let height = cambc::map::height().unwrap_or(32);
    let total_tiles = (width * height).max(1) as usize;

    let scan = scan_camps_for_draft(width, height);

    let wall_count = cambc::map::walls().and_then(|b| b.count()).unwrap_or(0);
    let hole_count = cambc::map::holes().and_then(|b| b.count()).unwrap_or(0);
    let bush_count = cambc::map::bushes().and_then(|b| b.count()).unwrap_or(0);

    let wall_pct = ((wall_count * 100) / total_tiles).min(100) as u8;
    let hole_pct = ((hole_count * 100) / total_tiles).min(100) as u8;
    let bush_pct = ((bush_count * 100) / total_tiles).min(100) as u8;

    let ally_spawn_positions = cambc::map::spawn_region(Team::Ally)
        .and_then(|b| b.positions())
        .unwrap_or_default();
    let enemy_spawn_positions = cambc::map::spawn_region(Team::Enemy)
        .and_then(|b| b.positions())
        .unwrap_or_default();
    let altar_positions = cambc::map::altars()
        .and_then(|b| b.positions())
        .unwrap_or_default();

    let cheb_spawn_dist = ally_spawn_positions
        .iter()
        .take(6)
        .flat_map(|a| {
            enemy_spawn_positions
                .iter()
                .take(6)
                .map(move |b| a.chebyshev_distance(*b))
        })
        .min()
        .unwrap_or(30) as u16;

    let cheb_altar_dist = ally_spawn_positions
        .iter()
        .take(6)
        .flat_map(|a| {
            altar_positions
                .iter()
                .take(8)
                .map(move |b| a.chebyshev_distance(*b))
        })
        .min()
        .unwrap_or(15) as u16;

    let mut walk_spawn_dist = cheb_spawn_dist;
    let mut walk_altar_dist = cheb_altar_dist;

    let max_draft_bfs_tiles = if PATCH_ADAPT_DRAFT_100X { 10_000 } else { 3_600 };
    if total_tiles <= max_draft_bfs_tiles && cambc::fuel_remaining().unwrap_or(0) > 250_000 {
        if let (Ok(walls), Ok(holes), Ok(altars), Ok(ally_spawn), Ok(mut field), Ok(mut visited)) = (
            cambc::map::walls(),
            cambc::map::holes(),
            cambc::map::altars(),
            cambc::map::spawn_region(Team::Ally),
            DistanceField::new(),
            Bitboard::new(),
        ) {
            if let Ok(mut obstacles) = walls.union(holes) {
                let _ = obstacles.union_assign(altars);
                for &p in &ally_spawn_positions {
                    let _ = obstacles.set(p, false);
                }
                for &p in &enemy_spawn_positions {
                    let _ = obstacles.set(p, false);
                }
                if field
                    .compute(&obstacles, ally_spawn, &mut visited, UNREACHABLE_DIST)
                    .is_ok()
                {
                    if let Some(d) = enemy_spawn_positions
                        .iter()
                        .filter_map(|&p| field.get(p).ok())
                        .filter(|&d| d < UNREACHABLE_DIST)
                        .min()
                    {
                        walk_spawn_dist = d;
                    }
                    let mut best_altar = UNREACHABLE_DIST;
                    for &al in &altar_positions {
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                let adj = Vec2::new(al.x + dx, al.y + dy);
                                if cambc::map::is_in_bounds(adj).unwrap_or(false) {
                                    if let Ok(d) = field.get(adj) {
                                        if d < best_altar {
                                            best_altar = d;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if best_altar < UNREACHABLE_DIST {
                        walk_altar_dist = best_altar;
                    }
                }
            }
        }
    }

    let is_rows = width == 50
        && height == 50
        && scan.three_wolf_groups == 6
        && scan.monoliths >= 1;
    let is_large = total_tiles >= 1600 || walk_spawn_dist >= 36;
    let is_compact = total_tiles <= 900 || walk_spawn_dist <= 20;
    let is_heavy_jungle =
        (scan.monoliths >= 1 && (scan.elks >= 3 || scan.wolves >= 10)) || scan.elks >= 5;
    let detour_high = walk_spawn_dist as u32 * 100 >= (cheb_spawn_dist.max(1) as u32) * 135;
    let is_chokepoint = wall_pct.saturating_add(hole_pct) >= 22 || detour_high;
    let is_bush_heavy = bush_pct >= 12;
    let is_hole_heavy = hole_pct >= 8;
    let is_distant_altars = walk_altar_dist >= 18;

    let mut terrain_flags = 0u8;
    if is_rows {
        terrain_flags |= TERRAIN_ROWS_MAP;
    }
    if is_large {
        terrain_flags |= TERRAIN_LARGE_MAP;
    }
    if is_compact {
        terrain_flags |= TERRAIN_COMPACT_MAP;
    }
    if is_heavy_jungle {
        terrain_flags |= TERRAIN_HEAVY_JUNGLE;
    }
    if is_chokepoint {
        terrain_flags |= TERRAIN_CHOKEPOINT;
    }
    if is_bush_heavy {
        terrain_flags |= TERRAIN_BUSH_HEAVY;
    }
    if is_hole_heavy {
        terrain_flags |= TERRAIN_HOLE_HEAVY;
    }
    if is_distant_altars {
        terrain_flags |= TERRAIN_DISTANT_ALTARS;
    }

    memo[INTEL_OFFSET] = MAGIC_0;
    memo[INTEL_OFFSET + 1] = MAGIC_1;
    memo[INTEL_OFFSET + 2] = cambc::draft::team_size().unwrap_or(5).clamp(1, 5) as u8;
    memo[INTEL_OFFSET + 18] = width.clamp(0, 255) as u8;
    memo[INTEL_OFFSET + 19] = height.clamp(0, 255) as u8;
    memo[INTEL_OFFSET + 20] = wall_pct;
    memo[INTEL_OFFSET + 21] = hole_pct;
    memo[INTEL_OFFSET + 22] = bush_pct;
    memo[INTEL_OFFSET + 23] = walk_spawn_dist.min(255) as u8;
    memo[INTEL_OFFSET + 24] = walk_altar_dist.min(255) as u8;
    memo[INTEL_OFFSET + 25] = scan.monoliths;
    memo[INTEL_OFFSET + 26] = scan.elks;
    memo[INTEL_OFFSET + 27] = scan.wolves;
    memo[INTEL_OFFSET + 28] = scan.clusters;
    memo[INTEL_OFFSET + 29] = terrain_flags;

    memo
}

fn ensure_base_memo() -> Vec<u8> {
    BASE_MEMO.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(build_base_memo());
        }
        cell.borrow().as_ref().expect("initialized").clone()
    })
}

fn submit_updated_memo(
    base: &mut [u8],
    ally_picks: &[ChampionDraft],
    enemy_picks: &[ChampionDraft],
    pending_ally_pick: Option<ChampionDraft>,
) {
    let mut enemy_mask = 0u16;
    let mut ally_mask = 0u16;

    for slot in &mut base[(INTEL_OFFSET + 8)..(INTEL_OFFSET + 18)] {
        *slot = 0;
    }

    for (i, &ep) in enemy_picks.iter().take(5).enumerate() {
        enemy_mask |= draft_to_mask(ep);
        base[INTEL_OFFSET + 8 + i] = draft_to_code(ep);
    }

    let mut ally_count = 0usize;
    for &ap in ally_picks.iter().take(5) {
        ally_mask |= draft_to_mask(ap);
        base[INTEL_OFFSET + 13 + ally_count] = draft_to_code(ap);
        ally_count += 1;
    }
    if let Some(pending) = pending_ally_pick {
        if !ally_picks.contains(&pending) && ally_count < 5 {
            ally_mask |= draft_to_mask(pending);
            base[INTEL_OFFSET + 13 + ally_count] = draft_to_code(pending);
        }
    }

    base[INTEL_OFFSET + 3] = enemy_picks.len().min(255) as u8;
    let enemy_bytes = enemy_mask.to_le_bytes();
    base[INTEL_OFFSET + 4] = enemy_bytes[0];
    base[INTEL_OFFSET + 5] = enemy_bytes[1];

    let ally_bytes = ally_mask.to_le_bytes();
    base[INTEL_OFFSET + 6] = ally_bytes[0];
    base[INTEL_OFFSET + 7] = ally_bytes[1];

    base[INTEL_OFFSET + 30] = compute_comp_flags(enemy_mask);
    base[INTEL_OFFSET + 31] = compute_comp_flags(ally_mask);

    let _ = cambc::draft_memory().submit(base);
}

const PVE_VARG_PREFERENCE: &[ChampionDraft] = &[
    ChampionDraft::VesperVale,
    ChampionDraft::Vendetta,
    ChampionDraft::HeartOfRuin,
    ChampionDraft::Varg,
    ChampionDraft::Marcella,
    ChampionDraft::CassiAndIssac,
    ChampionDraft::Ryth,
    ChampionDraft::Prism,
    ChampionDraft::Ren,
    ChampionDraft::Tavi,
    ChampionDraft::Stellar,
    ChampionDraft::Willow,
];

const STANDARD_PREFERENCE: &[ChampionDraft] = &[
    ChampionDraft::VesperVale,
    ChampionDraft::Vendetta,
    ChampionDraft::HeartOfRuin,
    ChampionDraft::Marcella,
    ChampionDraft::Varg,
    ChampionDraft::CassiAndIssac,
    ChampionDraft::Ryth,
    ChampionDraft::Prism,
    ChampionDraft::Ren,
    ChampionDraft::Tavi,
    ChampionDraft::Stellar,
    ChampionDraft::Willow,
];

// ============================================================
// EVOLUTION 4: DRAFT COUNTER-SYSTEM
// Real-time enemy pick reaction, synergy denial, pick-order awareness
// ============================================================

/// Synergy pair definitions for counter-picking
const SYNERGY_PAIRS: &[(ChampionDraft, ChampionDraft, f32)] = &[
    // (champ1, champ2, synergy_strength) - higher = stronger synergy to deny
    (ChampionDraft::CassiAndIssac, ChampionDraft::CassiAndIssac, 1.0), // Already joint pick
    (ChampionDraft::Varg, ChampionDraft::CassiAndIssac, 0.95), // Varg+Cassi meta
    (ChampionDraft::Varg, ChampionDraft::Stellar, 0.90),       // Varg+Stellar dive
    (ChampionDraft::Varg, ChampionDraft::Ryth, 0.85),          // Varg+Ryth engage
    (ChampionDraft::CassiAndIssac, ChampionDraft::Stellar, 0.80), // Cassi+Stellar burst
    (ChampionDraft::VesperVale, ChampionDraft::Vendetta, 0.85), // Econ + Reset
    (ChampionDraft::HeartOfRuin, ChampionDraft::Marcella, 0.75), // Sustain + Tank
    (ChampionDraft::Vendetta, ChampionDraft::Varg, 0.70),      // Reset + Dive
    (ChampionDraft::Prism, ChampionDraft::Ryth, 0.75),         // AOE + Engage
    (ChampionDraft::Tavi, ChampionDraft::Marcella, 0.70),      // Shield + Tank
    (ChampionDraft::Willow, ChampionDraft::HeartOfRuin, 0.80), // Heal + Missing HP
    (ChampionDraft::Ren, ChampionDraft::Varg, 0.65),           // Thorns + Dive
];

/// Counter-pick matrix: (enemy_champ, counter_champ, effectiveness)
/// Effectiveness: 1.0 = hard counter, 0.5 = soft counter
const COUNTER_PICKS: &[(ChampionDraft, ChampionDraft, f32)] = &[
    // Enemy -> Counter
    (ChampionDraft::Varg, ChampionDraft::CassiAndIssac, 0.95),     // Cassi kites Varg
    (ChampionDraft::Varg, ChampionDraft::Stellar, 0.90),           // Stellar outranges
    (ChampionDraft::Varg, ChampionDraft::Prism, 0.85),             // Prism AOE zones
    (ChampionDraft::CassiAndIssac, ChampionDraft::Varg, 0.90),     // Varg dives Cassi
    (ChampionDraft::CassiAndIssac, ChampionDraft::Vendetta, 0.85), // Vendetta resets on kill
    (ChampionDraft::CassiAndIssac, ChampionDraft::Ryth, 0.80),     // Ryth engages through blind
    (ChampionDraft::Stellar, ChampionDraft::Varg, 0.90),           // Varg dives Stellar
    (ChampionDraft::Stellar, ChampionDraft::Vendetta, 0.85),       // Vendetta resets
    (ChampionDraft::Stellar, ChampionDraft::Marcella, 0.80),       // Marcella tanks shots
    (ChampionDraft::Vendetta, ChampionDraft::CassiAndIssac, 0.90), // Cassi outranges
    (ChampionDraft::Vendetta, ChampionDraft::Prism, 0.80),         // Prism zones Vendetta
    (ChampionDraft::HeartOfRuin, ChampionDraft::Varg, 0.85),       // Varg burst kills low HP
    (ChampionDraft::HeartOfRuin, ChampionDraft::Vendetta, 0.80),   // Vendetta executes
    (ChampionDraft::Marcella, ChampionDraft::Varg, 0.75),          // Varg AOE through shield
    (ChampionDraft::Marcella, ChampionDraft::CassiAndIssac, 0.70), // Cassi poke through shield
    (ChampionDraft::Willow, ChampionDraft::Varg, 0.80),            // Varg burst > heal
    (ChampionDraft::Willow, ChampionDraft::CassiAndIssac, 0.75),   // Cassi poke
    (ChampionDraft::Ryth, ChampionDraft::Varg, 0.85),              // Varg outranges
    (ChampionDraft::Ryth, ChampionDraft::Vendetta, 0.80),          // Vendetta resets on kill
    (ChampionDraft::Prism, ChampionDraft::Varg, 0.85),             // Varg dives Prism
    (ChampionDraft::Prism, ChampionDraft::Vendetta, 0.80),         // Vendetta resets
    (ChampionDraft::Tavi, ChampionDraft::Varg, 0.75),              // Varg dives Tavi
    (ChampionDraft::Ren, ChampionDraft::Varg, 0.70),               // Varg burst
];

/// Calculate synergy threat score for enemy composition
fn calculate_synergy_threat(enemy_mask: u16) -> f32 {
    let mut threat = 0.0f32;
    for &(a, b, strength) in SYNERGY_PAIRS {
        let has_a = (enemy_mask & draft_to_mask(a)) != 0;
        let has_b = (enemy_mask & draft_to_mask(b)) != 0;
        if has_a && has_b {
            threat += strength * 100.0;
        }
    }
    threat
}

/// Calculate counter-pick score for a candidate against enemy composition
fn calculate_counter_score(candidate: ChampionDraft, enemy_mask: u16) -> f32 {
    let mut score = 0.0f32;
    for &(enemy, counter, effectiveness) in COUNTER_PICKS {
        if candidate == counter && (enemy_mask & draft_to_mask(enemy)) != 0 {
            score += effectiveness * 50.0;
        }
    }
    score
}

/// Calculate synergy bonus for candidate with our current picks
fn calculate_synergy_bonus(candidate: ChampionDraft, ally_mask: u16) -> f32 {
    let mut bonus = 0.0f32;
    let cand_mask = draft_to_mask(candidate);
    for &(a, b, strength) in SYNERGY_PAIRS {
        let has_a = (ally_mask & draft_to_mask(a)) != 0;
        let has_b = (ally_mask & draft_to_mask(b)) != 0;
        let cand_is_a = draft_to_mask(a) == cand_mask;
        let cand_is_b = draft_to_mask(b) == cand_mask;
        
        if (has_a && cand_is_b) || (has_b && cand_is_a) {
            bonus += strength * 40.0;
        }
    }
    bonus
}

/// Determine if we should deny an enemy synergy by picking one of the pair
fn should_deny_synergy(candidate: ChampionDraft, enemy_mask: u16, enemy_picks_count: usize) -> bool {
    // Only deny if enemy has 1-2 picks (early enough to deny)
    if enemy_picks_count >= 3 {
        return false;
    }
    
    for &(a, b, strength) in SYNERGY_PAIRS {
        if strength < 0.8 { continue; } // Only deny strong synergies
        let has_a = (enemy_mask & draft_to_mask(a)) != 0;
        let has_b = (enemy_mask & draft_to_mask(b)) != 0;
        let cand_is_a = draft_to_mask(a) == draft_to_mask(candidate);
        let cand_is_b = draft_to_mask(b) == draft_to_mask(candidate);
        
        // Enemy has one, we can pick the other to deny completion
        if (has_a && cand_is_b) || (has_b && cand_is_a) {
            return true;
        }
    }
    false
}

/// Get priority adjustment for counter-picking
fn get_counter_priority_adjustment(candidate: ChampionDraft, enemy_mask: u16, ally_mask: u16) -> i32 {
    let mut adj = 0i32;
    
    // Counter enemy picks
    adj += calculate_counter_score(candidate, enemy_mask) as i32;
    
    // Synergy with our picks
    adj += calculate_synergy_bonus(candidate, ally_mask) as i32;
    
    // Deny enemy synergies
    if should_deny_synergy(candidate, enemy_mask, enemy_mask.count_ones() as usize) {
        adj += 200; // High priority for synergy denial
    }
    
    adj
}

const SPECIAL_PREFERENCES: [&[ChampionDraft]; 12] = [
    // 1: 6-Body Macro (00corners, 00quadrants, adgato-gemcombat3)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Marcella,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Ren,
        ChampionDraft::Tavi,
        ChampionDraft::Willow,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 2: 6-Body Huge PvE Varg (00expanse, 01big-world, 02geodes)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Varg,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Marcella,
        ChampionDraft::Prism,
        ChampionDraft::Ren,
        ChampionDraft::Tavi,
        ChampionDraft::Willow,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 3: Chokepoint Willow (01boss-battle, 01corridors)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Willow,
        ChampionDraft::Marcella,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Tavi,
        ChampionDraft::Ren,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 4: 00lost-world Swarm (00lost-world)
    &[
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Prism,
        ChampionDraft::Marcella,
        ChampionDraft::Willow,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::Varg,
        ChampionDraft::Tavi,
        ChampionDraft::Ren,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 5: Marcella 2nd Brawl (adgato-gemcombat2)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Marcella,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Vendetta,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Tavi,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Ren,
        ChampionDraft::Willow,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 6: Tavi-Marcella 5-Pick (adgato-gemcombat4)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Tavi,
        ChampionDraft::Marcella,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Ren,
        ChampionDraft::Willow,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 7: 5-Body Sustain Willow (02uncharted-lands - newer team 2/2 100%)
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Marcella,
        ChampionDraft::Willow,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Tavi,
        ChampionDraft::Ren,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 8: Contest/Arena Anti-CC (00contest, 01elk_arena, 01hooks) - we lose 22-33%
    // VEGA9 meta: VARG+CASSI+ISSAC+STELLAR+RYTH
    &[
        ChampionDraft::Varg,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Stellar,
        ChampionDraft::Ryth,
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Marcella,
        ChampionDraft::Prism,
        ChampionDraft::Ren,
        ChampionDraft::Tavi,
        ChampionDraft::Willow,
    ],
    // 9: Collision/Eight Map Control (00collision, 00eight, eight) - we lose 22-35%
    // Need sustain + burst: VESPER+MARCELLA+HEART+VENDETTA+PRISM
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Marcella,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Vendetta,
        ChampionDraft::Prism,
        ChampionDraft::Varg,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Stellar,
        ChampionDraft::Tavi,
        ChampionDraft::Ren,
        ChampionDraft::Willow,
        ChampionDraft::Ryth,
    ],
    // 10: Rows Varg Counter (rows) - we lose 100% (0-120)
    // VEGA9 uses VARG+CASSI+ISSAC+STELLAR+RYTH - counter with same
    &[
        ChampionDraft::Varg,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Stellar,
        ChampionDraft::Ryth,
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Marcella,
        ChampionDraft::Prism,
        ChampionDraft::Ren,
        ChampionDraft::Tavi,
        ChampionDraft::Willow,
    ],
    // 11: Fragment/Pilgrimage Sustain (01fragment, 01pilgrimage) - we lose ~38-41%
    // High sustain: VESPER+MARCELLA+WILLOW+HEART+REN
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Marcella,
        ChampionDraft::Willow,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Ren,
        ChampionDraft::Vendetta,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Tavi,
        ChampionDraft::CassiAndIssac,
        ChampionDraft::Ryth,
        ChampionDraft::Stellar,
    ],
    // 12: Anti-CassiIssac Joint Pick (ladder-wide meta trap - H-013)
    // CASSI+ISSAC joint pick loses 45% WR across ladder - avoid unless 5th pick
    &[
        ChampionDraft::VesperVale,
        ChampionDraft::Vendetta,
        ChampionDraft::HeartOfRuin,
        ChampionDraft::Marcella,
        ChampionDraft::Varg,
        ChampionDraft::Prism,
        ChampionDraft::Stellar,
        ChampionDraft::Ren,
        ChampionDraft::Tavi,
        ChampionDraft::Willow,
        ChampionDraft::Ryth,
        ChampionDraft::CassiAndIssac,
    ],
];

pub fn play() -> Result<()> {
    let draft_mode = usize::from(crate::profile::get().draft);
    let mut memo = ensure_base_memo();
    let chosen = cambc::draft::current_draft(Team::Ally).unwrap_or_default();
    let enemy_chosen = cambc::draft::current_draft(Team::Enemy).unwrap_or_default();

    // Build masks from current picks
    let mut enemy_mask = 0u16;
    for &ep in &enemy_chosen {
        enemy_mask |= draft_to_mask(ep);
    }
    let mut ally_mask = 0u16;
    for &ap in &chosen {
        ally_mask |= draft_to_mask(ap);
    }

    let terrain_flags = memo[INTEL_OFFSET + 29];
    let map_w = usize::from(memo[INTEL_OFFSET + 18].max(30));
    let map_h = usize::from(memo[INTEL_OFFSET + 19].max(30));
    let total_tiles = map_w * map_h;
    let monoliths = memo[INTEL_OFFSET + 25];
    let clusters = memo[INTEL_OFFSET + 28];
    let wall_pct = memo[INTEL_OFFSET + 20];
    let hole_pct = memo[INTEL_OFFSET + 21];
    let _spawn_dist = memo[INTEL_OFFSET + 23];
    let altar_dist = memo[INTEL_OFFSET + 24];

    let is_rows = (terrain_flags & TERRAIN_ROWS_MAP) != 0;
    let is_huge = total_tiles >= 6400;
    let is_dense_monolith_swarm = monoliths >= 1 && clusters >= 80;
    let pve_varg_map = is_rows || is_huge || is_dense_monolith_swarm;

    // H-006: Map-adaptive draft - select based on map characteristics where we lose
    // Map detection based on profile.rs map signatures
    let is_contest_arena = (map_w == 50 && map_h == 50 && clusters >= 20) // 00contest, 01elk_arena, 01hooks
        || (map_w == 40 && map_h == 40 && monoliths >= 1); // 01hooks
    let is_collision_eight = (map_w == 41 && map_h == 41) // 00collision, 00eight
        || (total_tiles >= 1600 && wall_pct >= 15); // eight
    let is_fragment_pilgrimage = (altar_dist >= 18) // 01fragment, 01pilgrimage
        || (clusters >= 40 && monoliths == 0); // fragment variants
    let is_chokepoint = (terrain_flags & TERRAIN_CHOKEPOINT) != 0; // 01boss-battle, 01corridors

    // Determine best draft mode for this map
    let draft_mode = usize::from(crate::profile::get().draft);
    let adaptive_draft_mode = if is_rows {
        10 // Rows Varg Counter (priority 10)
    } else if is_contest_arena {
        8 // Contest/Arena Anti-CC
    } else if is_collision_eight {
        9 // Collision/Eight Map Control
    } else if is_fragment_pilgrimage {
        11 // Fragment/Pilgrimage Sustain
    } else if is_chokepoint && draft_mode == 0 {
        3 // Chokepoint Willow
    } else if pve_varg_map && draft_mode == 0 {
        2 // Huge PvE Varg
    } else if wall_pct >= 12 && hole_pct >= 8 && draft_mode == 0 {
        12 // Anti-CassiIssac (hole-heavy maps where Cassi+Issac is weak)
    } else {
        draft_mode // Use profile default
    };

    let base_candidates: &[ChampionDraft] = if (1..=SPECIAL_PREFERENCES.len()).contains(&adaptive_draft_mode) {
        SPECIAL_PREFERENCES[adaptive_draft_mode - 1]
    } else if pve_varg_map {
        PVE_VARG_PREFERENCE
    } else {
        STANDARD_PREFERENCE
    };

    // ===== EVOLUTION 4: DRAFT COUNTER-SYSTEM =====
    // Score candidates with counter-pick, synergy, and synergy denial bonuses
    let enemy_picks_count = enemy_chosen.len();
    let synergy_threat = calculate_synergy_threat(enemy_mask);
    
    // Score all available candidates
    let mut scored_candidates: Vec<(i32, ChampionDraft)> = base_candidates
        .iter()
        .filter(|&&c| !chosen.contains(&c))
        .map(|&c| {
            let candidate = c;
            let mut score = 0i32;
            
            // Base position bonus (earlier in list = higher base)
            let pos_bonus = (base_candidates.len() - base_candidates.iter().position(|&x| x == candidate).unwrap_or(0)) as i32 * 10;
            score += pos_bonus;
            
            // Counter-pick bonuses
            score += get_counter_priority_adjustment(candidate, enemy_mask, ally_mask);
            
            // Synergy threat awareness - penalize picks that feed enemy synergies
            let candidate_mask = draft_to_mask(candidate);
            let new_enemy_mask = enemy_mask | candidate_mask; // Hypothetical
            let new_synergy_threat = calculate_synergy_threat(new_enemy_mask);
            if new_synergy_threat > synergy_threat + 50.0 {
                score -= 150; // Penalize feeding enemy synergies
            }
            
            (score, candidate)
        })
        .collect();

    // Sort by score descending
    scored_candidates.sort_by(|a, b| b.0.cmp(&a.0));

    // Try candidates in score order
    for (_, candidate) in scored_candidates {
        if !chosen.contains(&candidate) {
            submit_updated_memo(&mut memo, &chosen, &enemy_chosen, Some(candidate));
            if cambc::draft::pick(candidate).is_ok() {
                return Ok(());
            }
        }
    }

    // Fallback: standard priority order
    for &pick in ChampionDraft::ALL {
        if !chosen.contains(&pick) {
            submit_updated_memo(&mut memo, &chosen, &enemy_chosen, Some(pick));
            let _ = cambc::draft::pick(pick);
            break;
        }
    }
    Ok(())
}
