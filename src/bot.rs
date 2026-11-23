use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashSet;

use cambc::{
    Bitboard, BuffKind, BuffPayload, CampGroupCaptureType, CampType, Champion, ChampionType,
    ConsumableType, DistanceField, ItemType as Item, PassiveType, Result, Team, Vec2,
};

const RECALL_THRESHOLD: f32 = 0.25;
const PILGRIM_THRESHOLD: f32 = 0.35;
const MAX_STRAIGHT_GOALS: u32 = 32;

const MIN_FUEL_FOR_SEARCH: u64 = 28_000;
const MIN_FUEL_FOR_WARD: u64 = 18_000;
const MIN_FUEL_FOR_TURN_TAIL: u64 = 12_000;
const UNREACHABLE_DIST: u16 = u16::MAX;

const PATCH_ADAPT_MONSTER_115: bool = true;
const PATCH_ADAPT_CAPTURE_STATE: bool = true;
const PATCH_ADAPT_FAST_ATTACK_GOALS: bool = true;
const PATCH_ADAPT_FUEL_GUARD: bool = true;

const ENDGAME_BANK_ROUND: u32 = 1850;

const SHARED_CAMP_SLOTS: usize = 6;

const INTEL_OFFSET: usize = 992;
const MAGIC_0: u8 = 0xCA;
const MAGIC_1: u8 = 0xBC;

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

const PASSIVE_TREASURE: u16 = 1 << 0;
const PASSIVE_MONSTER: u16 = 1 << 1;
const PASSIVE_JUNGLE: u16 = 1 << 2;
const PASSIVE_DURABILITY: u16 = 1 << 3;
const PASSIVE_HOME: u16 = 1 << 4;
const PASSIVE_CAPTURE: u16 = 1 << 5;
const PASSIVE_PROSPECTOR: u16 = 1 << 6;
const PASSIVE_TRIBUTE: u16 = 1 << 7;
const PASSIVE_HUNTER: u16 = 1 << 8;
const PASSIVE_SKIRMISH: u16 = 1 << 9;

thread_local! {
    static PREV_POS: Cell<Option<Vec2>> = const { Cell::new(None) };
    static MAP_DIMS: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
    static STATIC_BLOCKED: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static STATIC_WALKABLE: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static STATIC_DASH_BLOCKED: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static BFS_FIELD: RefCell<Option<DistanceField>> = const { RefCell::new(None) };
    static BFS_VISITED: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static BFS_DEST: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static ESCAPE_FIELD: RefCell<Option<DistanceField>> = const { RefCell::new(None) };
    static ESCAPE_VISITED: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static DANGER: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static CORE_DANGER: RefCell<Option<Bitboard>> = const { RefCell::new(None) };
    static COMBAT_ENGAGE_MODE: Cell<bool> = const { Cell::new(false) };
    static BFS_GOALS: RefCell<Option<(Bitboard, u32)>> = const { RefCell::new(None) };
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum EmblemFamily {
    Treasure,
    Monster,
    Jungle,
    Durability,
    Home,
    Bruiser,
    Power,
    Capture,
    Prospector,
    Tribute,
    Hunter,
    Skirmish,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum StatKind {
    Strength,
    Health,
    Focus,
}

impl EmblemFamily {
    fn passive_bit(self) -> u16 {
        match self {
            EmblemFamily::Treasure => PASSIVE_TREASURE,
            EmblemFamily::Monster => PASSIVE_MONSTER,
            EmblemFamily::Jungle => PASSIVE_JUNGLE,
            EmblemFamily::Durability => PASSIVE_DURABILITY,
            EmblemFamily::Home => PASSIVE_HOME,
            EmblemFamily::Capture => PASSIVE_CAPTURE,
            EmblemFamily::Prospector => PASSIVE_PROSPECTOR,
            EmblemFamily::Tribute => PASSIVE_TRIBUTE,
            EmblemFamily::Hunter => PASSIVE_HUNTER,
            EmblemFamily::Skirmish => PASSIVE_SKIRMISH,
            EmblemFamily::Bruiser | EmblemFamily::Power => 0,
        }
    }
}

fn shard_item(family: EmblemFamily, stat: StatKind) -> Item {
    match (family, stat) {
        (EmblemFamily::Monster, StatKind::Strength) => Item::MonsterShardOfStrength,
        (EmblemFamily::Monster, StatKind::Health) => Item::MonsterShardOfHealth,
        (EmblemFamily::Monster, StatKind::Focus) => Item::MonsterShardOfFocus,
        (EmblemFamily::Power, StatKind::Strength) => Item::PowerShardOfStrength,
        (EmblemFamily::Power, StatKind::Health) => Item::PowerShardOfHealth,
        (EmblemFamily::Power, StatKind::Focus) => Item::PowerShardOfFocus,
        (EmblemFamily::Durability, StatKind::Strength) => Item::DurabilityShardOfStrength,
        (EmblemFamily::Durability, StatKind::Health) => Item::DurabilityShardOfHealth,
        (EmblemFamily::Durability, StatKind::Focus) => Item::DurabilityShardOfFocus,
        (EmblemFamily::Treasure, StatKind::Strength) => Item::TreasureShardOfStrength,
        (EmblemFamily::Treasure, StatKind::Health) => Item::TreasureShardOfHealth,
        (EmblemFamily::Treasure, StatKind::Focus) => Item::TreasureShardOfFocus,
        (EmblemFamily::Jungle, StatKind::Strength) => Item::JungleShardOfStrength,
        (EmblemFamily::Jungle, StatKind::Health) => Item::JungleShardOfHealth,
        (EmblemFamily::Jungle, StatKind::Focus) => Item::JungleShardOfFocus,
        (EmblemFamily::Home, StatKind::Strength) => Item::HomeShardOfStrength,
        (EmblemFamily::Home, StatKind::Health) => Item::HomeShardOfHealth,
        (EmblemFamily::Home, StatKind::Focus) => Item::HomeShardOfFocus,
        (EmblemFamily::Bruiser, StatKind::Strength) => Item::BruiserShardOfStrength,
        (EmblemFamily::Bruiser, StatKind::Health) => Item::BruiserShardOfHealth,
        (EmblemFamily::Bruiser, StatKind::Focus) => Item::BruiserShardOfFocus,
        (EmblemFamily::Capture, StatKind::Strength) => Item::CaptureShardOfStrength,
        (EmblemFamily::Capture, StatKind::Health) => Item::CaptureShardOfHealth,
        (EmblemFamily::Capture, StatKind::Focus) => Item::CaptureShardOfFocus,
        (EmblemFamily::Prospector, StatKind::Strength) => Item::ProspectorShardOfStrength,
        (EmblemFamily::Prospector, StatKind::Health) => Item::ProspectorShardOfHealth,
        (EmblemFamily::Prospector, StatKind::Focus) => Item::ProspectorShardOfFocus,
        (EmblemFamily::Tribute, StatKind::Strength) => Item::TributeShardOfStrength,
        (EmblemFamily::Tribute, StatKind::Health) => Item::TributeShardOfHealth,
        (EmblemFamily::Tribute, StatKind::Focus) => Item::TributeShardOfFocus,
        (EmblemFamily::Hunter, StatKind::Strength) => Item::HunterShardOfStrength,
        (EmblemFamily::Hunter, StatKind::Health) => Item::HunterShardOfHealth,
        (EmblemFamily::Hunter, StatKind::Focus) => Item::HunterShardOfFocus,
        (EmblemFamily::Skirmish, StatKind::Strength) => Item::SkirmishShardOfStrength,
        (EmblemFamily::Skirmish, StatKind::Health) => Item::SkirmishShardOfHealth,
        (EmblemFamily::Skirmish, StatKind::Focus) => Item::SkirmishShardOfFocus,
    }
}

fn item_family_and_stat(item: Item) -> (EmblemFamily, StatKind) {
    match item {
        Item::MonsterShardOfStrength => (EmblemFamily::Monster, StatKind::Strength),
        Item::MonsterShardOfHealth => (EmblemFamily::Monster, StatKind::Health),
        Item::MonsterShardOfFocus => (EmblemFamily::Monster, StatKind::Focus),
        Item::PowerShardOfStrength => (EmblemFamily::Power, StatKind::Strength),
        Item::PowerShardOfHealth => (EmblemFamily::Power, StatKind::Health),
        Item::PowerShardOfFocus => (EmblemFamily::Power, StatKind::Focus),
        Item::DurabilityShardOfStrength => (EmblemFamily::Durability, StatKind::Strength),
        Item::DurabilityShardOfHealth => (EmblemFamily::Durability, StatKind::Health),
        Item::DurabilityShardOfFocus => (EmblemFamily::Durability, StatKind::Focus),
        Item::TreasureShardOfStrength => (EmblemFamily::Treasure, StatKind::Strength),
        Item::TreasureShardOfHealth => (EmblemFamily::Treasure, StatKind::Health),
        Item::TreasureShardOfFocus => (EmblemFamily::Treasure, StatKind::Focus),
        Item::JungleShardOfStrength => (EmblemFamily::Jungle, StatKind::Strength),
        Item::JungleShardOfHealth => (EmblemFamily::Jungle, StatKind::Health),
        Item::JungleShardOfFocus => (EmblemFamily::Jungle, StatKind::Focus),
        Item::HomeShardOfStrength => (EmblemFamily::Home, StatKind::Strength),
        Item::HomeShardOfHealth => (EmblemFamily::Home, StatKind::Health),
        Item::HomeShardOfFocus => (EmblemFamily::Home, StatKind::Focus),
        Item::BruiserShardOfStrength => (EmblemFamily::Bruiser, StatKind::Strength),
        Item::BruiserShardOfHealth => (EmblemFamily::Bruiser, StatKind::Health),
        Item::BruiserShardOfFocus => (EmblemFamily::Bruiser, StatKind::Focus),
        Item::CaptureShardOfStrength => (EmblemFamily::Capture, StatKind::Strength),
        Item::CaptureShardOfHealth => (EmblemFamily::Capture, StatKind::Health),
        Item::CaptureShardOfFocus => (EmblemFamily::Capture, StatKind::Focus),
        Item::ProspectorShardOfStrength => (EmblemFamily::Prospector, StatKind::Strength),
        Item::ProspectorShardOfHealth => (EmblemFamily::Prospector, StatKind::Health),
        Item::ProspectorShardOfFocus => (EmblemFamily::Prospector, StatKind::Focus),
        Item::TributeShardOfStrength => (EmblemFamily::Tribute, StatKind::Strength),
        Item::TributeShardOfHealth => (EmblemFamily::Tribute, StatKind::Health),
        Item::TributeShardOfFocus => (EmblemFamily::Tribute, StatKind::Focus),
        Item::HunterShardOfStrength => (EmblemFamily::Hunter, StatKind::Strength),
        Item::HunterShardOfHealth => (EmblemFamily::Hunter, StatKind::Health),
        Item::HunterShardOfFocus => (EmblemFamily::Hunter, StatKind::Focus),
        Item::SkirmishShardOfStrength => (EmblemFamily::Skirmish, StatKind::Strength),
        Item::SkirmishShardOfHealth => (EmblemFamily::Skirmish, StatKind::Health),
        Item::SkirmishShardOfFocus => (EmblemFamily::Skirmish, StatKind::Focus),
        _ => (EmblemFamily::Power, StatKind::Focus),
    }
}

fn champion_type_bit(ct: ChampionType) -> u16 {
    match ct {
        ChampionType::Cassi => BIT_CASSI,
        ChampionType::Issac => BIT_ISSAC,
        ChampionType::Ryth => BIT_RYTH,
        ChampionType::VesperVale => BIT_VESPER_VALE,
        ChampionType::Prism => BIT_PRISM,
        ChampionType::Stellar => BIT_STELLAR,
        ChampionType::Vendetta => BIT_VENDETTA,
        ChampionType::Marcella => BIT_MARCELLA,
        ChampionType::Tavi => BIT_TAVI,
        ChampionType::Willow => BIT_WILLOW,
        ChampionType::Ren => BIT_REN,
        ChampionType::Varg => BIT_VARG,
        ChampionType::HeartOfRuin => BIT_HEART,
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

#[inline]
fn is_heart_of_ruin(unit: &Champion) -> bool {
    matches!(unit.champion_name(), Ok(ChampionType::HeartOfRuin))
        || matches!(unit.passive(), Ok(PassiveType::MissingHealthDamage))
}

#[inline]
fn is_vendetta(unit: &Champion) -> bool {
    matches!(unit.champion_name(), Ok(ChampionType::Vendetta))
        || matches!(unit.passive(), Ok(PassiveType::ResetCooldownsOnKill))
}

#[inline]
fn unit_missing_hp_damage_mult(unit: &Champion) -> f32 {
    if is_heart_of_ruin(unit) {
        let hp = unit.health().unwrap_or(1000.0).max(0.0);
        let max_hp = unit.max_health().unwrap_or(1000.0).max(1.0);
        let missing_ratio = ((max_hp - hp) / max_hp).clamp(0.0, 1.0);
        1.0 + 0.5 * missing_ratio
    } else {
        1.0
    }
}

#[inline]
fn effective_basic_attack_damage(unit: &Champion) -> f32 {
    unit.strength().unwrap_or(40.0) * unit_missing_hp_damage_mult(unit)
}

#[inline]
fn active_shield_value(unit: &Champion) -> f32 {
    unit.buffs()
        .ok()
        .and_then(|b| b.get(BuffKind::Shield))
        .map_or(0.0, |payload| match payload {
            BuffPayload::Shield { value, .. } => value,
            _ => 0.0,
        })
}

fn potential_shield_value(unit: &Champion, is_self: bool) -> f32 {
    let active = active_shield_value(unit);
    if active > 0.0 {
        return active;
    }
    if is_self {
        for slot in unit.abilities() {
            if slot.is_shield() && slot.is_ready().unwrap_or(false) {
                return slot
                    .shield_amount()
                    .unwrap_or_else(|_| 1.5 * unit.focus().unwrap_or(100.0));
            }
        }
        0.0
    } else {
        match unit.champion_name() {
            Ok(ChampionType::Varg) => 2.5 * unit.focus().unwrap_or(100.0),
            Ok(ChampionType::HeartOfRuin) => 1.5 * unit.focus().unwrap_or(100.0),
            _ => 0.0,
        }
    }
}

#[inline]
fn is_combat_dash(me: &Champion, slot: &cambc::Ability) -> bool {
    if !slot.is_dash() {
        return false;
    }
    if let Ok(Some(cambc::AbilityInfo::Dash(d))) = slot.info() {
        return d.modifier > 0.0 || d.duration > 0;
    }
    is_heart_of_ruin(me)
}

#[inline]
fn camp_design_health(kind: CampType) -> f32 {
    match kind {
        CampType::Wolf => 1350.0,
        CampType::Elk => 2400.0,
        CampType::Golem => 5000.0,
        CampType::Monolith => 20000.0,
        CampType::GemCluster | CampType::GoldCluster => 120.0,
    }
}

#[inline]
fn camp_design_strength(kind: CampType) -> f32 {
    match kind {
        CampType::Wolf => 20.0,
        CampType::Elk => 60.0,
        CampType::Golem => 120.0,
        CampType::Monolith => 200.0,
        CampType::GemCluster | CampType::GoldCluster => 0.0,
    }
}

#[inline]
fn camp_attack_range(kind: CampType) -> i32 {
    match kind {
        CampType::Wolf => 3,
        CampType::Elk | CampType::Golem => 4,
        CampType::Monolith => 5,
        CampType::GemCluster | CampType::GoldCluster => 0,
    }
}

fn sync_priority(
    me: &Champion,
    champ_type: Option<ChampionType>,
    enemies: &[EnemyInfo],
    channeling_or_low: bool,
) {
    let desired = if !me.is_alive().unwrap_or(false) {
        0
    } else {
        let combat_enemies: Vec<&EnemyInfo> = enemies
            .iter()
            .filter(|e| e.dist <= 6 || e.submitting)
            .collect();
        if !combat_enemies.is_empty() {
            if matches!(champ_type, Some(ChampionType::Willow)) {
                let heal_ready = me.abilities()[0].is_ready().unwrap_or(false);
                let reset_ready = me.abilities()[1].is_ready().unwrap_or(false);
                let ally_needs_heal = cambc::vision::champions(Some(Team::Ally))
                    .map(|allies| {
                        allies.iter().any(|a| {
                            let hp = a.health().unwrap_or(0.0);
                            let max_hp = a.max_health().unwrap_or(1.0).max(1.0);
                            hp > 0.0 && hp < max_hp * 0.88
                        })
                    })
                    .unwrap_or(false);
                if heal_ready && ally_needs_heal {
                    20300
                } else if reset_ready {
                    20150
                } else {
                    let min_ratio = combat_enemies
                        .iter()
                        .map(|e| (e.hp / e.max_hp.max(1.0)).clamp(0.0, 0.99))
                        .fold(0.99f32, f32::min);
                    10 - (min_ratio * 10.0).floor() as i32
                }
            } else {
                let min_ratio = combat_enemies
                    .iter()
                    .map(|e| (e.hp / e.max_hp.max(1.0)).clamp(0.0, 0.99))
                    .fold(0.99f32, f32::min);
                10 - (min_ratio * 10.0).floor() as i32
            }
        } else {
            let hp = me.health().unwrap_or(1000.0) + active_shield_value(me) * 0.5;
            let max_hp = me.max_health().unwrap_or(1000.0).max(1.0);
            let eff_ratio = (hp / max_hp).clamp(0.0, 0.99);
            let mut k = 9 - (eff_ratio * 10.0).floor().clamp(0.0, 9.0) as i32;
            if channeling_or_low && k < 6 {
                k = 6;
            }
            -k
        }
    };

    if me.priority().ok() != Some(desired) {
        let _ = me.set_priority(desired);
    }
}

#[derive(Clone, Copy, Default)]
pub struct DraftIntel {
    pub initialized: bool,
    pub team_size: u8,
    pub enemy_mask: u16,
    pub ally_mask: u16,
    pub enemy_picks: [u8; 5],
    pub ally_picks: [u8; 5],
    pub map_width: u8,
    pub map_height: u8,
    pub wall_pct: u8,
    pub hole_pct: u8,
    pub bush_pct: u8,
    pub spawn_to_spawn_dist: u8,
    pub spawn_to_altar_dist: u8,
    pub monolith_count: u8,
    pub elk_count: u8,
    pub wolf_count: u8,
    pub cluster_count: u8,
    pub terrain_flags: u8,
    pub enemy_comp_flags: u8,
    pub ally_comp_flags: u8,
}

impl DraftIntel {
    pub fn ensure_loaded(&mut self, planner: &CampPlanner) {
        if self.initialized {
            return;
        }
        let memo = cambc::draft_memory().bytes();
        if memo.len() >= INTEL_OFFSET + 32
            && memo[INTEL_OFFSET] == MAGIC_0
            && memo[INTEL_OFFSET + 1] == MAGIC_1
        {
            self.team_size = memo[INTEL_OFFSET + 2];
            self.enemy_mask =
                u16::from_le_bytes([memo[INTEL_OFFSET + 4], memo[INTEL_OFFSET + 5]]);
            self.ally_mask =
                u16::from_le_bytes([memo[INTEL_OFFSET + 6], memo[INTEL_OFFSET + 7]]);
            self.enemy_picks
                .copy_from_slice(&memo[(INTEL_OFFSET + 8)..(INTEL_OFFSET + 13)]);
            self.ally_picks
                .copy_from_slice(&memo[(INTEL_OFFSET + 13)..(INTEL_OFFSET + 18)]);
            self.map_width = memo[INTEL_OFFSET + 18];
            self.map_height = memo[INTEL_OFFSET + 19];
            self.wall_pct = memo[INTEL_OFFSET + 20];
            self.hole_pct = memo[INTEL_OFFSET + 21];
            self.bush_pct = memo[INTEL_OFFSET + 22];
            self.spawn_to_spawn_dist = memo[INTEL_OFFSET + 23];
            self.spawn_to_altar_dist = memo[INTEL_OFFSET + 24];
            self.monolith_count = memo[INTEL_OFFSET + 25];
            self.elk_count = memo[INTEL_OFFSET + 26];
            self.wolf_count = memo[INTEL_OFFSET + 27];
            self.cluster_count = memo[INTEL_OFFSET + 28];
            self.terrain_flags = memo[INTEL_OFFSET + 29];
            self.enemy_comp_flags = memo[INTEL_OFFSET + 30];
            self.ally_comp_flags = memo[INTEL_OFFSET + 31];
            self.initialized = true;
        } else {
            self.compute_fallback_from_map(planner);
            self.initialized = true;
        }

        if planner.loaded && self.monolith_count == 0 && self.elk_count == 0 && self.wolf_count == 0
        {
            self.refresh_camp_counts_from_planner(planner);
        }
        if !planner.row.is_empty() {
            self.terrain_flags |= TERRAIN_ROWS_MAP;
        }
    }

    fn refresh_camp_counts_from_planner(&mut self, planner: &CampPlanner) {
        let mut monoliths = 0u8;
        let mut elks = 0u8;
        let mut wolves = 0u8;
        let mut clusters = 0u8;
        for g in &planner.groups {
            for c in g.group.iter() {
                if c.is_monolith() {
                    monoliths = monoliths.saturating_add(1);
                } else if c.is_elk() || c.is_golem() {
                    elks = elks.saturating_add(1);
                } else if c.is_wolf() {
                    wolves = wolves.saturating_add(1);
                } else if c.is_gem_cluster() || c.is_gold_cluster() {
                    clusters = clusters.saturating_add(1);
                }
            }
        }
        self.monolith_count = monoliths;
        self.elk_count = elks;
        self.wolf_count = wolves;
        self.cluster_count = clusters;
        if (monoliths >= 1 && (elks >= 3 || wolves >= 10)) || elks >= 5 {
            self.terrain_flags |= TERRAIN_HEAVY_JUNGLE;
        }
        if !planner.row.is_empty() {
            self.terrain_flags |= TERRAIN_ROWS_MAP;
        }
    }

    fn compute_fallback_from_map(&mut self, planner: &CampPlanner) {
        let (width, height) = map_dims();
        let total_tiles = (width * height).max(1) as usize;

        let wall_count = cambc::map::walls().and_then(|b| b.count()).unwrap_or(0);
        let hole_count = cambc::map::holes().and_then(|b| b.count()).unwrap_or(0);
        let bush_count = cambc::map::bushes().and_then(|b| b.count()).unwrap_or(0);

        self.map_width = width.clamp(0, 255) as u8;
        self.map_height = height.clamp(0, 255) as u8;
        self.wall_pct = ((wall_count * 100) / total_tiles).min(100) as u8;
        self.hole_pct = ((hole_count * 100) / total_tiles).min(100) as u8;
        self.bush_pct = ((bush_count * 100) / total_tiles).min(100) as u8;

        let ally_spawn = if !planner.ally_spawn_positions.is_empty() {
            planner.ally_spawn_positions.clone()
        } else {
            cambc::map::spawn_region(Team::Ally)
                .and_then(|b| b.positions())
                .unwrap_or_default()
        };
        let enemy_spawn = cambc::map::spawn_region(Team::Enemy)
            .and_then(|b| b.positions())
            .unwrap_or_default();
        let altars = if !planner.altar_positions.is_empty() {
            planner.altar_positions.clone()
        } else {
            cambc::map::altars()
                .and_then(|b| b.positions())
                .unwrap_or_default()
        };

        let spawn_dist = ally_spawn
            .iter()
            .take(6)
            .flat_map(|a| {
                enemy_spawn
                    .iter()
                    .take(6)
                    .map(move |b| a.chebyshev_distance(*b))
            })
            .min()
            .unwrap_or(30)
            .clamp(0, 255) as u8;
        let altar_dist = ally_spawn
            .iter()
            .take(6)
            .flat_map(|a| altars.iter().take(8).map(move |b| a.chebyshev_distance(*b)))
            .min()
            .unwrap_or(15)
            .clamp(0, 255) as u8;

        self.spawn_to_spawn_dist = spawn_dist;
        self.spawn_to_altar_dist = altar_dist;

        let mut flags = 0u8;
        if total_tiles >= 1600 || spawn_dist >= 36 {
            flags |= TERRAIN_LARGE_MAP;
        }
        if total_tiles <= 900 || spawn_dist <= 20 {
            flags |= TERRAIN_COMPACT_MAP;
        }
        if self.wall_pct.saturating_add(self.hole_pct) >= 22 {
            flags |= TERRAIN_CHOKEPOINT;
        }
        if self.bush_pct >= 12 {
            flags |= TERRAIN_BUSH_HEAVY;
        }
        if self.hole_pct >= 8 {
            flags |= TERRAIN_HOLE_HEAVY;
        }
        if altar_dist >= 18 {
            flags |= TERRAIN_DISTANT_ALTARS;
        }
        self.terrain_flags = flags;
        if planner.loaded {
            self.refresh_camp_counts_from_planner(planner);
        }
    }

    fn observe_live_rosters(&mut self, scanned_enemies: &[EnemyInfo], visible_allies: &[Champion]) {
        let mut updated_enemy = self.enemy_mask;
        for e in scanned_enemies {
            if let Ok(ct) = e.unit.champion_name() {
                let bit = champion_type_bit(ct);
                updated_enemy |= bit;
                if matches!(ct, ChampionType::Cassi | ChampionType::Issac) {
                    updated_enemy |= BIT_CASSI | BIT_ISSAC;
                }
            }
        }
        if updated_enemy != self.enemy_mask {
            self.enemy_mask = updated_enemy;
            self.enemy_comp_flags = compute_comp_flags(updated_enemy);
        }

        let mut updated_ally = self.ally_mask;
        for a in visible_allies {
            if let Ok(ct) = a.champion_name() {
                updated_ally |= champion_type_bit(ct);
            }
        }
        if updated_ally != self.ally_mask {
            self.ally_mask = updated_ally;
            self.ally_comp_flags = compute_comp_flags(updated_ally);
        }
    }

    #[inline]
    pub fn is_rows_map(&self) -> bool {
        (self.terrain_flags & TERRAIN_ROWS_MAP) != 0
    }
    #[inline]
    pub fn is_large_map(&self) -> bool {
        (self.terrain_flags & TERRAIN_LARGE_MAP) != 0
    }
    #[inline]
    pub fn is_huge_map(&self) -> bool {
        usize::from(self.map_width) * usize::from(self.map_height) >= 6400
    }
    #[inline]
    pub fn is_pve_monster_build_map(&self) -> bool {
        self.is_rows_map()
            || self.is_huge_map()
            || (self.monolith_count >= 1 && self.cluster_count >= 80)
    }
    #[inline]
    pub fn is_heavy_jungle(&self) -> bool {
        (self.terrain_flags & TERRAIN_HEAVY_JUNGLE) != 0
    }
    #[inline]
    pub fn is_chokepoint(&self) -> bool {
        (self.terrain_flags & TERRAIN_CHOKEPOINT) != 0
    }
    #[inline]
    pub fn is_hole_heavy(&self) -> bool {
        (self.terrain_flags & TERRAIN_HOLE_HEAVY) != 0
    }
    #[inline]
    pub fn is_distant_altars(&self) -> bool {
        (self.terrain_flags & TERRAIN_DISTANT_ALTARS) != 0
    }
}

#[derive(Clone)]
struct EnemyInfo {
    unit: Champion,
    pos: Vec2,
    hp: f32,
    max_hp: f32,
    gems: u32,
    dist: i32,
    atk_range: i32,
    threat_radius: i32,
    interrupt_radius: i32,
    burst_damage: f32,
    all_abilities_on_cd: bool,
    submitting: bool,
    recalling: bool,
    stunned: bool,
    priority_score: f32,
}

#[derive(Clone)]
struct NearCamp {
    unit: cambc::Camp,
    pos: Vec2,
    hp: f32,
    strength: f32,
    dist: i32,
    awake: bool,
}

#[inline]
fn me() -> Result<&'static Champion> {
    Ok(cambc::self_champion()?.expect("play starts after the draft"))
}

#[inline]
fn distance(a: Vec2, b: Vec2) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[inline]
fn has_fuel(min_fuel: u64) -> bool {
    cambc::fuel_remaining().unwrap_or(u64::MAX) >= min_fuel
}

#[inline]
fn map_dims() -> (i32, i32) {
    MAP_DIMS.with(|cell| {
        if let Some(dims) = cell.get() {
            return dims;
        }
        let w = cambc::map::width().unwrap_or(32);
        let h = cambc::map::height().unwrap_or(32);
        cell.set(Some((w, h)));
        (w, h)
    })
}

#[inline]
fn in_bounds_fast(p: Vec2) -> bool {
    let (w, h) = map_dims();
    p.x >= 0 && p.x < w && p.y >= 0 && p.y < h
}

fn oriented(mut tiles: Vec<Vec2>) -> Vec<Vec2> {
    let orient = crate::profile::get().orient;
    if orient > 7 {
        return tiles;
    }
    let flip_x = if orient & 1 != 0 { -1 } else { 1 };
    let flip_y = if orient & 2 != 0 { -1 } else { 1 };
    if orient & 4 != 0 {
        tiles.sort_by_key(|p| (p.x * flip_x, p.y * flip_y));
    } else {
        tiles.sort_by_key(|p| (p.y * flip_y, p.x * flip_x));
    }
    tiles
}

fn tiles_around(position: Vec2, radius: i32) -> Result<Bitboard> {
    let mut tiles = Bitboard::new()?;
    tiles.draw_chebyshev(position, radius as f32)?;
    Ok(tiles)
}

fn ensure_static_blocked() -> Result<Bitboard> {
    STATIC_BLOCKED.with(|cell| {
        if let Some(ref b) = *cell.borrow() {
            return Ok(b.clone());
        }
        let mut b = cambc::map::walls()?.clone();
        b.union_assign(cambc::map::holes()?)?;
        b.union_assign(cambc::map::altars()?)?;
        b.union_assign(cambc::map::spawn_region(Team::Enemy)?)?;
        *cell.borrow_mut() = Some(b.clone());
        Ok(b)
    })
}

fn ensure_static_walkable() -> Result<Bitboard> {
    STATIC_WALKABLE.with(|cell| {
        if let Some(ref b) = *cell.borrow() {
            return Ok(b.clone());
        }
        let blocked = ensure_static_blocked()?;
        let walkable = blocked.complement()?;
        *cell.borrow_mut() = Some(walkable.clone());
        Ok(walkable)
    })
}

fn ensure_static_dash_blocked() -> Result<Bitboard> {
    STATIC_DASH_BLOCKED.with(|cell| {
        if let Some(ref b) = *cell.borrow() {
            return Ok(b.clone());
        }
        let mut b = cambc::map::walls()?.clone();
        b.union_assign(cambc::map::altars()?)?;
        b.union_assign(cambc::map::spawn_region(Team::Enemy)?)?;
        *cell.borrow_mut() = Some(b.clone());
        Ok(b)
    })
}

fn raw_blocked_tiles(my_pos: Vec2) -> Result<Bitboard> {
    let mut blocked = ensure_static_blocked()?;
    blocked.union_assign(&cambc::vision::champion_tiles(None)?)?;
    blocked.union_assign(&cambc::vision::camp_tiles()?)?;
    blocked.union_assign(&cambc::vision::teleport_tiles()?)?;
    blocked.set(my_pos, false)?;
    Ok(blocked)
}

fn blocked_tiles(my_pos: Vec2) -> Result<Bitboard> {
    let mut blocked = raw_blocked_tiles(my_pos)?;
    if !COMBAT_ENGAGE_MODE.with(|m| m.get()) {
        let mut applied_full = false;
        DANGER.with(|d| {
            if let Some(ref zone) = *d.borrow() {
                if !zone.is_set(my_pos).unwrap_or(true) {
                    let _ = blocked.union_assign(zone);
                    applied_full = true;
                }
            }
        });
        if !applied_full {
            CORE_DANGER.with(|cd| {
                if let Some(ref czone) = *cd.borrow() {
                    if !czone.is_set(my_pos).unwrap_or(true) {
                        let _ = blocked.union_assign(czone);
                    }
                }
            });
        }
    }
    blocked.set(my_pos, false)?;
    Ok(blocked)
}

fn legal_step_neighbors(origin: Vec2, blocked: &Bitboard) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(8);
    for dx in -1..=1 {
        for dy in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let p = Vec2::new(origin.x + dx, origin.y + dy);
            if in_bounds_fast(p) && !blocked.is_set(p).unwrap_or(true) {
                out.push(p);
            }
        }
    }
    oriented(out)
}

fn legal_dash_tiles(origin: Vec2, blocked: &Bitboard) -> Result<Vec<Vec2>> {
    let dash_step_blocked = ensure_static_dash_blocked()?;
    let mut passable_mid = [[false; 3]; 3];
    for dx in -1..=1 {
        for dy in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let mid = Vec2::new(origin.x + dx, origin.y + dy);
            if in_bounds_fast(mid) && !dash_step_blocked.is_set(mid).unwrap_or(true) {
                passable_mid[(dx + 1) as usize][(dy + 1) as usize] = true;
            }
        }
    }

    let mut out = Vec::with_capacity(24);
    for dx in -2..=2 {
        for dy in -2..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let p = Vec2::new(origin.x + dx, origin.y + dy);
            if !in_bounds_fast(p) {
                continue;
            }
            let dist = dx.abs().max(dy.abs());
            if dist == 2 {
                let mut has_mid = false;
                'mid_search: for mx in -1..=1 {
                    for my in -1..=1 {
                        if (mx != 0 || my != 0)
                            && passable_mid[(mx + 1) as usize][(my + 1) as usize]
                            && (dx - mx).abs().max((dy - my).abs()) == 1
                        {
                            has_mid = true;
                            break 'mid_search;
                        }
                    }
                }
                if !has_mid {
                    continue;
                }
            }
            if !blocked.is_set(p).unwrap_or(true) {
                out.push(p);
            }
        }
    }
    Ok(oriented(out))
}

fn try_use_tp_crystal_along_field(
    me: &Champion,
    origin: Vec2,
    here: u16,
    field: &DistanceField,
    blocked: &Bitboard,
) -> Result<bool> {
    if !has_owned_teleport_crystal(me) || here < 10 {
        return Ok(false);
    }
    if let Ok(enemies) = cambc::vision::champions(Some(Team::Enemy)) {
        if enemies.iter().any(|e| {
            e.health().unwrap_or(0.0) > 0.0
                && e.position().map_or(false, |p| distance(origin, p) <= 5)
        }) {
            return Ok(false);
        }
    }

    let mut candidates: Vec<(u16, i32, Vec2)> = Vec::new();
    for dx in -3..=3 {
        for dy in -3..=3 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let tile = Vec2::new(origin.x + dx, origin.y + dy);
            if !in_bounds_fast(tile) {
                continue;
            }
            let d = field.get(tile).unwrap_or(UNREACHABLE_DIST);
            if d >= UNREACHABLE_DIST {
                continue;
            }
            let saved = here.saturating_sub(d);
            if saved >= 8 && !blocked.is_set(tile).unwrap_or(true) {
                candidates.push((d, -(saved as i32), tile));
            }
        }
    }
    if candidates.is_empty() {
        return Ok(false);
    }
    candidates.sort_by_key(|c| (c.0, c.1));

    for item in me.consumables().iter() {
        if item.kind() == ConsumableType::TpCrystal && item.count().unwrap_or(0) > 0 {
            for (_, _, dest) in candidates.iter().take(6) {
                if item.use_at(*dest).unwrap_or(false) {
                    PREV_POS.with(|p| p.set(Some(origin)));
                    return Ok(true);
                }
            }
            break;
        }
    }
    Ok(false)
}

fn try_dash_along_field(
    me: &Champion,
    origin: Vec2,
    here: u16,
    field: &DistanceField,
    blocked: &Bitboard,
) -> Result<bool> {
    if here < 2 || !me.can_cast().unwrap_or(false) {
        return Ok(false);
    }
    let Some(idx) = dash_ready(me) else {
        return Ok(false);
    };
    let is_combat = is_combat_dash(me, &me.abilities()[idx]);
    let escaping_danger = !COMBAT_ENGAGE_MODE.with(|m| m.get())
        && DANGER.with(|d| {
            d.borrow()
                .as_ref()
                .is_some_and(|z| z.is_set(origin).unwrap_or(false))
        });
    if is_combat && !escaping_danger {
        let nearby_enemy = cambc::vision::champions(Some(Team::Enemy))
            .map(|es| {
                es.iter().any(|e| {
                    e.health().unwrap_or(0.0) > 0.0
                        && e.position().map_or(false, |p| distance(origin, p) <= 5)
                })
            })
            .unwrap_or(false);
        let nearby_camp = cambc::vision::camp_tiles()
            .ok()
            .map(|ct| {
                let mut found = false;
                'search: for dx in -4..=4 {
                    for dy in -4..=4 {
                        let p = Vec2::new(origin.x + dx, origin.y + dy);
                        if in_bounds_fast(p) && ct.is_set(p).unwrap_or(false) {
                            found = true;
                            break 'search;
                        }
                    }
                }
                found
            })
            .unwrap_or(false);
        if nearby_enemy || nearby_camp {
            return Ok(false);
        }
    }

    let dash_step_blocked = ensure_static_dash_blocked()?;
    let mut passable_mid = [[false; 3]; 3];
    for dx in -1..=1 {
        for dy in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let mid = Vec2::new(origin.x + dx, origin.y + dy);
            if in_bounds_fast(mid) && !dash_step_blocked.is_set(mid).unwrap_or(true) {
                passable_mid[(dx + 1) as usize][(dy + 1) as usize] = true;
            }
        }
    }

    let bushes = cambc::map::bushes().ok();
    let mut candidates = Vec::with_capacity(24);
    for dx in -2..=2 {
        for dy in -2..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let tile = Vec2::new(origin.x + dx, origin.y + dy);
            if !in_bounds_fast(tile) {
                continue;
            }
            let d = field.get(tile).unwrap_or(UNREACHABLE_DIST);
            if d >= UNREACHABLE_DIST || d.saturating_add(2) > here {
                continue;
            }
            if dx.abs().max(dy.abs()) == 2 {
                let mut has_mid = false;
                'mid_check: for mx in -1..=1 {
                    for my in -1..=1 {
                        if (mx != 0 || my != 0)
                            && passable_mid[(mx + 1) as usize][(my + 1) as usize]
                            && (dx - mx).abs().max((dy - my).abs()) == 1
                        {
                            has_mid = true;
                            break 'mid_check;
                        }
                    }
                }
                if !has_mid {
                    continue;
                }
            }
            if !blocked.is_set(tile).unwrap_or(true) {
                candidates.push(tile);
            }
        }
    }
    if candidates.is_empty() {
        return Ok(false);
    }

    let mut ranked: Vec<(u16, bool, Vec2)> = Vec::with_capacity(candidates.len());
    for tile in oriented(candidates) {
        let d = field.get(tile).unwrap_or(UNREACHABLE_DIST);
        let in_bush = bushes
            .and_then(|b| b.is_set(tile).ok())
            .unwrap_or(false);
        ranked.push((d, !in_bush, tile));
    }
    ranked.sort_by_key(|r| (r.0, r.1));
    let order: Vec<Vec2> = ranked.into_iter().map(|(_, _, t)| t).collect();
    if cast_dash_to_first_valid(me, idx, &order) {
        PREV_POS.with(|p| p.set(Some(origin)));
        return Ok(true);
    }
    Ok(false)
}

fn try_greedy_dash_and_step(
    me: &Champion,
    origin: Vec2,
    goals: &Bitboard,
    blocked: &Bitboard,
) -> Result<bool> {
    let Ok(goal_positions) = goals.positions() else {
        return Ok(false);
    };
    let Some(goal) = oriented(goal_positions)
        .into_iter()
        .min_by_key(|&g| distance(origin, g))
    else {
        return Ok(false);
    };

    let mut cur_pos = origin;
    let mut cur_dist = distance(cur_pos, goal);
    let mut acted = false;

    if cur_dist >= 2 && me.can_cast().unwrap_or(false) {
        if let Some(idx) = dash_ready(me) {
            if !is_combat_dash(me, &me.abilities()[idx]) {
                if let Ok(dash_tiles) = legal_dash_tiles(cur_pos, blocked) {
                    let mut ranked: Vec<(i32, Vec2)> = dash_tiles
                        .into_iter()
                        .filter_map(|t| {
                            let d = distance(t, goal);
                            (d + 1 < cur_dist).then_some((d, t))
                        })
                        .collect();
                    ranked.sort_by_key(|r| r.0);
                    let order: Vec<Vec2> = ranked.into_iter().map(|(_, t)| t).collect();
                    if cast_dash_to_first_valid(me, idx, &order) {
                        PREV_POS.with(|p| p.set(Some(cur_pos)));
                        cur_pos = me.position().unwrap_or(cur_pos);
                        cur_dist = distance(cur_pos, goal);
                        acted = true;
                    }
                }
            }
        }
    }

    if cur_dist >= 1 && me.can_move().unwrap_or(false) {
        let prev = PREV_POS.with(|p| p.get());
        let mut steps: Vec<(bool, i32, Vec2)> = legal_step_neighbors(cur_pos, blocked)
            .into_iter()
            .filter_map(|t| {
                let d = distance(t, goal);
                (d < cur_dist).then_some((Some(t) == prev, d, t))
            })
            .collect();
        steps.sort_by_key(|s| (s.0, s.1));
        for (_, _, tile) in steps.into_iter().take(4) {
            if me.move_absolute(tile).unwrap_or(false) {
                PREV_POS.with(|p| p.set(Some(cur_pos)));
                acted = true;
                break;
            }
        }
    }
    Ok(acted)
}

fn ensure_escape_buffers() {
    ESCAPE_FIELD.with(|f_cell| {
        if f_cell.borrow().is_none() {
            *f_cell.borrow_mut() = DistanceField::new().ok();
        }
    });
    ESCAPE_VISITED.with(|v_cell| {
        if v_cell.borrow().is_none() {
            *v_cell.borrow_mut() = Bitboard::new().ok();
        }
    });
}

fn build_escape_goals_out_of_range(
    me: &Champion,
    origin: Vec2,
    ultimate_goals: &Bitboard,
    raw_blocked: &Bitboard,
    danger_zone: &Bitboard,
) -> Result<Option<Bitboard>> {
    let static_walkable = ensure_static_walkable()?;
    let mut passable = static_walkable.intersect(&raw_blocked.complement()?)?;
    let safe_walkable = passable.intersect(&danger_zone.complement()?)?;
    if safe_walkable.is_empty()? {
        return Ok(None);
    }

    passable.set(origin, true)?;
    let local_reach = passable.reachable_tiles(origin, 7)?;
    let local_safe = local_reach.intersect(&safe_walkable)?;
    if local_safe.is_empty()? {
        return Ok(None);
    }

    ensure_escape_buffers();
    let mut obstacles_with_danger = raw_blocked.union(danger_zone)?;
    let mut escape_roots = ultimate_goals.intersect(&safe_walkable)?;
    if escape_roots.is_empty()? {
        if let Ok(spawn) = cambc::map::spawn_region(Team::Ally) {
            escape_roots = spawn.intersect(&danger_zone.complement()?)?;
        }
    }
    if !escape_roots.is_empty()? {
        obstacles_with_danger.intersect_assign(&escape_roots.complement()?)?;
    }

    let global_ok = if !escape_roots.is_empty()? {
        ESCAPE_FIELD.with(|f_cell| {
            let mut f_borrow = f_cell.borrow_mut();
            let Some(ref mut field) = *f_borrow else {
                return false;
            };
            ESCAPE_VISITED.with(|v_cell| {
                let mut v_borrow = v_cell.borrow_mut();
                let Some(ref mut visited) = *v_borrow else {
                    return false;
                };
                field
                    .compute(&obstacles_with_danger, &escape_roots, visited, UNREACHABLE_DIST)
                    .is_ok()
            })
        })
    } else {
        false
    };

    let has_dash = dash_ready(me).is_some();
    let bushes = cambc::map::bushes().ok();

    if global_ok {
        let connected_safe = ESCAPE_VISITED.with(|v_cell| {
            v_cell
                .borrow()
                .as_ref()
                .and_then(|v| local_safe.intersect(v).ok())
        });
        if let Some(conn) = connected_safe {
            if !conn.is_empty()? {
                let positions = conn.positions()?;
                let min_d = positions
                    .iter()
                    .map(|&p| distance(origin, p))
                    .min()
                    .unwrap_or(99);
                let max_allow_d = if has_dash {
                    min_d.max(2)
                } else {
                    min_d + 1
                };
                let mut scored: Vec<(i32, bool, u16, Vec2)> = Vec::new();
                ESCAPE_FIELD.with(|f_cell| {
                    if let Some(ref field) = *f_cell.borrow() {
                        for p in positions {
                            let d = distance(origin, p);
                            if d <= max_allow_d {
                                let goal_d = field.get(p).unwrap_or(UNREACHABLE_DIST);
                                let in_bush = bushes
                                    .and_then(|b| b.is_set(p).ok())
                                    .unwrap_or(false);
                                scored.push((d, !in_bush, goal_d, p));
                            }
                        }
                    }
                });
                scored.sort_by_key(|s| (s.0, s.1, s.2));
                if !scored.is_empty() {
                    let best_d = scored[0].0;
                    let cutoff = if has_dash && best_d == 1 { 2 } else { best_d };
                    let mut out = Bitboard::new()?;
                    for (d, _, _, p) in scored.into_iter().take(10) {
                        if d <= cutoff {
                            out.set(p, true)?;
                        }
                    }
                    if !out.is_empty()? {
                        return Ok(Some(out));
                    }
                }
            }
        }
    }

    let mut local_positions = local_safe.positions()?;
    local_positions.sort_by_key(|&p| distance(origin, p));

    let mut non_enclosed: Vec<(i32, bool, usize, Vec2)> = Vec::new();
    for p in local_positions.into_iter().take(12) {
        let comp_size = safe_walkable
            .reachable_tiles(p, 5)
            .and_then(|r| r.count())
            .unwrap_or(0);
        if comp_size >= 10 {
            let d = distance(origin, p);
            let in_bush = bushes
                .and_then(|b| b.is_set(p).ok())
                .unwrap_or(false);
            non_enclosed.push((d, !in_bush, usize::MAX - comp_size, p));
        }
    }
    if non_enclosed.is_empty() {
        return Ok(None);
    }
    non_enclosed.sort_by_key(|s| (s.0, s.1, s.2));
    let best_d = non_enclosed[0].0;
    let cutoff = if has_dash && best_d == 1 { 2 } else { best_d };
    let mut out = Bitboard::new()?;
    for (d, _, _, p) in non_enclosed.into_iter().take(8) {
        if d <= cutoff {
            out.set(p, true)?;
        }
    }
    if out.is_empty()? {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn find_safe_holding_goals_towards(
    origin: Vec2,
    ultimate_goals: &Bitboard,
    raw_blocked: &Bitboard,
    danger_zone: &Bitboard,
) -> Result<Option<Bitboard>> {
    let static_walkable = ensure_static_walkable()?;
    let safe_walkable = static_walkable
        .intersect(&raw_blocked.complement()?)?
        .intersect(&danger_zone.complement()?)?;
    if safe_walkable.is_empty()? {
        return Ok(None);
    }
    let mut safe_from_origin = safe_walkable.clone();
    safe_from_origin.set(origin, true)?;
    let reachable_safe = safe_from_origin.reachable_tiles(origin, 7)?;
    let positions = reachable_safe.positions()?;
    if positions.len() < 10 {
        return Ok(None);
    }

    let goal_positions = ultimate_goals.positions().unwrap_or_default();
    if goal_positions.is_empty() {
        return Ok(None);
    }
    let approx_dist = |p: Vec2| -> i32 {
        goal_positions
            .iter()
            .take(8)
            .map(|&g| distance(p, g))
            .min()
            .unwrap_or(99)
    };
    let origin_goal_dist = approx_dist(origin);
    let bushes = cambc::map::bushes().ok();

    let mut best_tiles: Vec<(i32, bool, Vec2)> = Vec::new();
    for p in positions {
        if p == origin || danger_zone.is_set(p).unwrap_or(true) {
            continue;
        }
        let gd = approx_dist(p);
        if gd < origin_goal_dist {
            let in_bush = bushes
                .and_then(|b| b.is_set(p).ok())
                .unwrap_or(false);
            best_tiles.push((gd, !in_bush, p));
        }
    }
    if best_tiles.is_empty() {
        let mut hold = Bitboard::new()?;
        hold.set(origin, true)?;
        return Ok(Some(hold));
    }
    best_tiles.sort_by_key(|b| (b.0, b.1));
    let best_gd = best_tiles[0].0;
    let mut out = Bitboard::new()?;
    for (gd, _, p) in best_tiles.into_iter().take(6) {
        if gd <= best_gd + 1 {
            out.set(p, true)?;
        }
    }
    Ok(Some(out))
}

fn move_towards_closest_with_blocked(
    me: &Champion,
    origin: Vec2,
    mut goals: Bitboard,
    blocked: &Bitboard,
) -> Result<bool> {
    if (!me.can_move()? && dash_ready(me).is_none()) || goals.is_set(origin)? {
        return Ok(false);
    }

    let static_walkable = ensure_static_walkable()?;
    goals.intersect_assign(&static_walkable)?;
    if goals.is_empty()? {
        return Ok(false);
    }

    let engage_mode = COMBAT_ENGAGE_MODE.with(|m| m.get());
    let in_full_danger = !engage_mode
        && DANGER.with(|d| {
            d.borrow()
                .as_ref()
                .is_some_and(|z| z.is_set(origin).unwrap_or(false))
        });
    let has_any_danger = !engage_mode && DANGER.with(|d| d.borrow().is_some());

    let mut active_blocked = blocked.clone();

    if has_fuel(MIN_FUEL_FOR_SEARCH) && has_any_danger {
        let raw_blk = raw_blocked_tiles(origin)?;
        if in_full_danger {
            let escape_opt = DANGER.with(|d| -> Result<Option<Bitboard>> {
                if let Some(ref zone) = *d.borrow() {
                    build_escape_goals_out_of_range(me, origin, &goals, &raw_blk, zone)
                } else {
                    Ok(None)
                }
            })?;
            if let Some(escape_goals) = escape_opt {
                goals = escape_goals;
                active_blocked = raw_blk;
            } else {
                let core_escape_opt = CORE_DANGER.with(|cd| -> Result<Option<Bitboard>> {
                    if let Some(ref czone) = *cd.borrow() {
                        if czone.is_set(origin).unwrap_or(false) {
                            build_escape_goals_out_of_range(me, origin, &goals, &raw_blk, czone)
                        } else {
                            Ok(None)
                        }
                    } else {
                        Ok(None)
                    }
                })?;
                if let Some(core_escape_goals) = core_escape_opt {
                    goals = core_escape_goals;
                    active_blocked = raw_blk;
                } else {
                    active_blocked = raw_blk;
                }
            }
        } else {
            let safe_goals_opt = DANGER.with(|d| -> Result<Option<Bitboard>> {
                if let Some(ref zone) = *d.borrow() {
                    Ok(Some(goals.intersect(&zone.complement()?)?))
                } else {
                    Ok(None)
                }
            })?;
            if let Some(sg) = safe_goals_opt {
                if !sg.is_empty()? {
                    goals = sg;
                } else {
                    let hold_opt = DANGER.with(|d| -> Result<Option<Bitboard>> {
                        if let Some(ref zone) = *d.borrow() {
                            find_safe_holding_goals_towards(origin, &goals, &raw_blk, zone)
                        } else {
                            Ok(None)
                        }
                    })?;
                    if let Some(hold_goals) = hold_opt {
                        if hold_goals.is_set(origin).unwrap_or(false) {
                            return Ok(false);
                        }
                        goals = hold_goals;
                    } else {
                        active_blocked = raw_blk.clone();
                        CORE_DANGER.with(|cd| {
                            if let Some(ref czone) = *cd.borrow() {
                                if !czone.is_set(origin).unwrap_or(true) {
                                    let _ = active_blocked.union_assign(czone);
                                }
                            }
                        });
                        active_blocked.set(origin, false)?;
                    }
                }
            }
        }
    }

    if !has_fuel(MIN_FUEL_FOR_SEARCH) {
        if me.can_move()? && try_straight(me, origin, &goals, &active_blocked)? {
            PREV_POS.with(|p| p.set(Some(origin)));
            return Ok(true);
        }
        return try_greedy_dash_and_step(me, origin, &goals, &active_blocked);
    }

    BFS_FIELD.with(|f_cell| {
        if f_cell.borrow().is_none() {
            *f_cell.borrow_mut() = DistanceField::new().ok();
        }
    });
    BFS_VISITED.with(|v_cell| {
        if v_cell.borrow().is_none() {
            *v_cell.borrow_mut() = Bitboard::new().ok();
        }
    });
    BFS_DEST.with(|d_cell| {
        if d_cell.borrow().is_none() {
            *d_cell.borrow_mut() = Bitboard::new().ok();
        }
    });

    let round = cambc::round_number().unwrap_or(0) as u32;
    let reuse = !has_any_danger
        && BFS_GOALS.with(|g| {
            g.borrow()
                .as_ref()
                .is_some_and(|(b, r)| round < r + 8 && b.is_equal(&goals).unwrap_or(false))
        })
        && BFS_FIELD.with(|f| {
            f.borrow()
                .as_ref()
                .and_then(|f| f.get(origin).ok())
                .is_some_and(|d| d > 0 && d < UNREACHABLE_DIST)
        });
    let retry_goals = if reuse { Some(goals.clone()) } else { None };

    let run_bfs = |obs: &Bitboard, gls: &Bitboard| -> Result<()> {
        let bfs_obstacles = obs.intersect(&gls.complement()?)?;
        BFS_FIELD.with(|f_cell| {
            let mut f_borrow = f_cell.borrow_mut();
            let Some(ref mut field) = *f_borrow else {
                return Err(cambc::Error::MissingIdentity);
            };
            BFS_VISITED.with(|v_cell| {
                let mut v_borrow = v_cell.borrow_mut();
                let Some(ref mut visited) = *v_borrow else {
                    return Err(cambc::Error::MissingIdentity);
                };
                BFS_DEST.with(|d_cell| {
                    let mut d_borrow = d_cell.borrow_mut();
                    let Some(ref mut destinations) = *d_borrow else {
                        return Err(cambc::Error::MissingIdentity);
                    };
                    destinations.clear();
                    destinations.set(origin, true)?;
                    field.compute_to(
                        &bfs_obstacles,
                        gls,
                        destinations,
                        visited,
                        UNREACHABLE_DIST,
                    )
                })
            })
        })
    };

    let compute_ok = if reuse {
        Ok(())
    } else {
        let done = run_bfs(&active_blocked, &goals);
        if done.is_ok() && !has_any_danger {
            BFS_GOALS.with(|g| *g.borrow_mut() = Some((goals.clone(), round)));
        }
        done
    };
    if compute_ok.is_err() {
        return try_greedy_dash_and_step(me, origin, &goals, &active_blocked);
    }

    let mut cur_pos = origin;
    let mut here = BFS_FIELD.with(|f_cell| {
        f_cell
            .borrow()
            .as_ref()
            .and_then(|f| f.get(cur_pos).ok())
            .unwrap_or(UNREACHABLE_DIST)
    });

    if here >= UNREACHABLE_DIST && has_any_danger && !in_full_danger {
        let raw_blk = raw_blocked_tiles(origin)?;
        let hold_opt = DANGER.with(|d| -> Result<Option<Bitboard>> {
            if let Some(ref zone) = *d.borrow() {
                find_safe_holding_goals_towards(origin, &goals, &raw_blk, zone)
            } else {
                Ok(None)
            }
        })?;
        if let Some(hold_goals) = hold_opt {
            if hold_goals.is_set(origin).unwrap_or(false) {
                return Ok(false);
            }
            goals = hold_goals;
            if run_bfs(&active_blocked, &goals).is_ok() {
                here = BFS_FIELD.with(|f_cell| {
                    f_cell
                        .borrow()
                        .as_ref()
                        .and_then(|f| f.get(cur_pos).ok())
                        .unwrap_or(UNREACHABLE_DIST)
                });
            }
        } else {
            let mut core_blk = raw_blk.clone();
            CORE_DANGER.with(|cd| {
                if let Some(ref czone) = *cd.borrow() {
                    if !czone.is_set(origin).unwrap_or(true) {
                        let _ = core_blk.union_assign(czone);
                    }
                }
            });
            core_blk.set(origin, false)?;
            active_blocked = core_blk;
            if run_bfs(&active_blocked, &goals).is_ok() {
                here = BFS_FIELD.with(|f_cell| {
                    f_cell
                        .borrow()
                        .as_ref()
                        .and_then(|f| f.get(cur_pos).ok())
                        .unwrap_or(UNREACHABLE_DIST)
                });
            }
            if here >= UNREACHABLE_DIST {
                active_blocked = raw_blk;
                if run_bfs(&active_blocked, &goals).is_ok() {
                    here = BFS_FIELD.with(|f_cell| {
                        f_cell
                            .borrow()
                            .as_ref()
                            .and_then(|f| f.get(cur_pos).ok())
                            .unwrap_or(UNREACHABLE_DIST)
                    });
                }
            }
        }
    }

    if here >= UNREACHABLE_DIST {
        return Ok(false);
    }

    let used_tp = BFS_FIELD.with(|f_cell| {
        if let Some(ref field) = *f_cell.borrow() {
            try_use_tp_crystal_along_field(me, cur_pos, here, field, &active_blocked)
        } else {
            Ok(false)
        }
    })?;
    if used_tp {
        return Ok(true);
    }

    let mut dashed = false;
    let did_dash = BFS_FIELD.with(|f_cell| {
        if let Some(ref field) = *f_cell.borrow() {
            try_dash_along_field(me, cur_pos, here, field, &active_blocked)
        } else {
            Ok(false)
        }
    })?;
    if did_dash {
        dashed = true;
        cur_pos = me.position().unwrap_or(origin);
        if goals.is_set(cur_pos).unwrap_or(false) || !me.can_move().unwrap_or(false) {
            return Ok(true);
        }
        here = BFS_FIELD.with(|f_cell| {
            f_cell
                .borrow()
                .as_ref()
                .and_then(|f| f.get(cur_pos).ok())
                .unwrap_or(UNREACHABLE_DIST)
        });
    }

    if here >= UNREACHABLE_DIST || !me.can_move().unwrap_or(false) {
        return Ok(dashed);
    }

    let bushes = cambc::map::bushes().ok();
    let step_neighbors = legal_step_neighbors(cur_pos, &active_blocked);

    let mut candidates: Vec<(u16, bool, bool, bool, Vec2)> = Vec::with_capacity(8);
    BFS_FIELD.with(|f_cell| {
        if let Some(ref field) = *f_cell.borrow() {
            for tile in step_neighbors {
                if let Ok(d) = field.get(tile) {
                    if d < here {
                        let tile_in_danger = if has_any_danger {
                            DANGER.with(|dz| {
                                dz.borrow()
                                    .as_ref()
                                    .is_some_and(|z| z.is_set(tile).unwrap_or(false))
                            })
                        } else {
                            false
                        };
                        let tile_in_core = if has_any_danger {
                            CORE_DANGER.with(|cd| {
                                cd.borrow()
                                    .as_ref()
                                    .is_some_and(|z| z.is_set(tile).unwrap_or(false))
                            })
                        } else {
                            false
                        };
                        let in_bush = bushes
                            .and_then(|b| b.is_set(tile).ok())
                            .unwrap_or(false);
                        candidates.push((d, tile_in_danger, tile_in_core, !in_bush, tile));
                    }
                }
            }
        }
    });

    let prev = PREV_POS.with(|p| p.get());
    candidates.sort_by_key(|c| (c.0, c.1, c.2, Some(c.4) == prev, c.3));

    for (_, _, _, _, tile) in candidates.into_iter().take(6) {
        if me.move_absolute(tile).unwrap_or(false) {
            PREV_POS.with(|p| p.set(Some(cur_pos)));
            return Ok(true);
        }
    }
    if let Some(goals) = retry_goals {
        if !dashed {
            BFS_GOALS.with(|g| *g.borrow_mut() = None);
            return move_towards_closest_with_blocked(me, origin, goals, blocked);
        }
    }
    Ok(dashed)
}

fn move_towards_closest(goals: Bitboard) -> Result<bool> {
    let me = me()?;
    let origin = me.position()?;
    if (!me.can_move()? && dash_ready(me).is_none()) || goals.is_set(origin)? {
        return Ok(false);
    }
    let blocked = blocked_tiles(origin)?;
    move_towards_closest_with_blocked(me, origin, goals, &blocked)
}

pub fn create_line(a: Vec2, b: Vec2) -> HashSet<Vec2> {
    let mut locs = HashSet::new();
    let mut x = a.x;
    let mut y = a.y;
    let mut dx = b.x - a.x;
    let mut dy = b.y - a.y;

    let sx = dx.signum();
    let sy = dy.signum();
    dx = dx.abs();
    dy = dy.abs();

    let d = dx.max(dy);
    let mut r = d / 2;

    if dx > dy {
        for _ in 0..d {
            locs.insert(Vec2::new(x, y));
            x += sx;
            r += dy;
            if r >= dx {
                locs.insert(Vec2::new(x, y));
                y += sy;
                r -= dx;
            }
        }
    } else {
        for _ in 0..d {
            locs.insert(Vec2::new(x, y));
            y += sy;
            r += dx;
            if r >= dy {
                locs.insert(Vec2::new(x, y));
                x += sx;
                r -= dy;
            }
        }
    }
    locs.insert(Vec2::new(x, y));
    locs
}

fn enemy_ability_and_attack_ranges(
    enemy: &Champion,
    stunned: bool,
) -> (i32, i32, i32, f32, bool) {
    let atk_range = enemy.attack_range().unwrap_or(1).max(1);
    let enemy_focus = enemy.focus().unwrap_or(100.0);
    let enemy_str = effective_basic_attack_damage(enemy);
    let missing_mult = unit_missing_hp_damage_mult(enemy);

    let mut max_ability_range = 0i32;
    let mut max_interrupt_range = if stunned {
        atk_range
    } else if atk_range == 1 {
        2
    } else {
        atk_range
    };
    let mut best_ability_burst = 0.0f32;
    let mut all_on_cd = true;

    for slot in enemy.abilities() {
        let on_cd = slot.is_on_cooldown().unwrap_or(false);
        if on_cd {
            continue;
        }
        all_on_cd = false;

        match slot.info() {
            Ok(Some(cambc::AbilityInfo::Damage(d))) => {
                let r = d.range.ceil() as i32;
                max_ability_range = max_ability_range.max(r);
                let int_r = if !d.properties.exclusive_with_movement && !stunned {
                    r + 1
                } else {
                    r
                };
                max_interrupt_range = max_interrupt_range.max(int_r);
                let dmg = slot
                    .damage_amount()
                    .unwrap_or(d.modifier * enemy_focus * missing_mult);
                if dmg > best_ability_burst {
                    best_ability_burst = dmg;
                }
            }
            Ok(Some(cambc::AbilityInfo::AoeDamage(aoe))) => {
                let r = aoe.range.ceil() as i32;
                let splash_r = (aoe.range + aoe.radius).ceil() as i32;
                max_ability_range = max_ability_range.max(r);
                max_interrupt_range = max_interrupt_range.max(splash_r);
                let dmg = slot
                    .damage_amount()
                    .unwrap_or(aoe.modifier * enemy_focus * missing_mult);
                if dmg > best_ability_burst {
                    best_ability_burst = dmg;
                }
            }
            Ok(Some(cambc::AbilityInfo::Cc(cc))) => {
                let r = cc.range.ceil() as i32;
                let splash_r = if matches!(enemy.champion_name(), Ok(ChampionType::Ryth)) {
                    r + 1
                } else {
                    r
                };
                max_ability_range = max_ability_range.max(r);
                let int_r = if !cc.properties.exclusive_with_movement && !stunned {
                    splash_r + 1
                } else {
                    splash_r
                };
                max_interrupt_range = max_interrupt_range.max(int_r);
                let dmg = slot
                    .damage_amount()
                    .unwrap_or(cc.modifier * enemy_focus * missing_mult);
                if dmg > best_ability_burst {
                    best_ability_burst = dmg;
                }
            }
            Ok(Some(cambc::AbilityInfo::Dash(dash))) => {
                let dash_dist = dash.range.ceil() as i32;
                let hit_rad = dash.radius.ceil().max(1.0) as i32;
                let r = (dash_dist + hit_rad).max(atk_range);
                max_ability_range = max_ability_range.max(r);
                let int_r = if atk_range > 1 {
                    (dash_dist + atk_range).min(5)
                } else {
                    dash_dist + hit_rad
                };
                max_interrupt_range = max_interrupt_range.max(int_r);
                if dash.modifier > 0.0 {
                    let dmg = slot
                        .damage_amount()
                        .unwrap_or(dash.modifier * enemy_focus * missing_mult);
                    if dmg > best_ability_burst {
                        best_ability_burst = dmg;
                    }
                }
            }
            Ok(Some(cambc::AbilityInfo::AttackReset(ar))) => {
                let r = if !stunned { atk_range + 1 } else { atk_range };
                if ar.enhanced_attack || atk_range > 1 {
                    max_ability_range = max_ability_range.max(r);
                }
                max_interrupt_range = max_interrupt_range.max(r);
                if enemy_str > best_ability_burst {
                    best_ability_burst = enemy_str;
                }
            }
            Ok(Some(cambc::AbilityInfo::Heal(_))) | Ok(Some(cambc::AbilityInfo::Shield(_))) => {}
            _ => {
                if slot.can_target_enemies().unwrap_or(false) {
                    if let Ok(Some(info)) = slot.info() {
                        if let Ok(r_f32) = info.as_dyn().range() {
                            let r = r_f32.ceil() as i32;
                            max_ability_range = max_ability_range.max(r);
                            max_interrupt_range = max_interrupt_range.max(r);
                        }
                    }
                }
            }
        }
    }

    let mobile_melee = if !stunned && atk_range == 1 { 2 } else { atk_range };
    let threat_range = atk_range.max(mobile_melee).max(max_ability_range);
    let interrupt_range = max_interrupt_range.max(threat_range);
    let total_burst = if stunned {
        0.0
    } else {
        enemy_str + best_ability_burst
    };

    (atk_range, threat_range, interrupt_range, total_burst, all_on_cd)
}

fn scan_enemies(my_pos: Vec2, max_dist: i32) -> Result<Vec<EnemyInfo>> {
    let mut result = Vec::new();
    for enemy in cambc::vision::champions(Some(Team::Enemy))? {
        let Ok(epos) = enemy.position() else { continue };
        let dist = distance(my_pos, epos);
        if dist > max_dist {
            continue;
        }
        let hp = enemy.health().unwrap_or(0.0);
        if hp <= 0.0 {
            continue;
        }
        let max_hp = enemy.max_health().unwrap_or(hp.max(1.0));
        let gems = enemy.gem_count().unwrap_or(0);
        let submitting = enemy.is_submitting_gems().unwrap_or(false);
        let recalling = enemy.is_recalling().unwrap_or(false)
            && enemy.remaining_channel_time().unwrap_or(i32::MAX) <= 2;
        let stunned = enemy.buffs().map(|b| b.has_stun()).unwrap_or(false);
        let (atk_range, threat_radius, interrupt_radius, burst_damage, all_abilities_on_cd) =
            enemy_ability_and_attack_ranges(&enemy, stunned);

        let mut priority_score = hp + active_shield_value(&enemy) * 0.5;
        if submitting {
            priority_score -= 650.0;
        }
        if recalling {
            priority_score -= 280.0;
        }
        if stunned {
            priority_score -= 110.0;
        }
        if all_abilities_on_cd {
            priority_score -= 70.0;
        }
        priority_score -= (gems as f32) * 18.0;
        if is_heart_of_ruin(&enemy) && hp / max_hp.max(1.0) < 0.45 {
            priority_score -= 120.0;
        }
        if matches!(enemy.champion_name(), Ok(ChampionType::Cassi)) {
            priority_score -= 340.0;
        } else if matches!(enemy.champion_name(), Ok(ChampionType::Issac)) {
            priority_score -= 260.0;
        }

        result.push(EnemyInfo {
            unit: enemy,
            pos: epos,
            hp,
            max_hp,
            gems,
            dist,
            atk_range,
            threat_radius,
            interrupt_radius,
            burst_damage,
            all_abilities_on_cd,
            submitting,
            recalling,
            stunned,
            priority_score,
        });
    }
    result.sort_by(|a, b| {
        a.priority_score
            .partial_cmp(&b.priority_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.hp.partial_cmp(&b.hp).unwrap_or(Ordering::Equal))
    });
    Ok(result)
}

#[inline]
fn cast_self_ability(me: &Champion, slot: &cambc::Ability) -> bool {
    slot.cast_untargeted().unwrap_or(false) || slot.cast_on(me).unwrap_or(false)
}

fn best_aoe_ground_tile_for_enemies(
    my_pos: Vec2,
    targets: &[EnemyInfo],
) -> Option<Vec2> {
    let mut best: Option<((usize, i32, i32), Vec2)> = None;
    for primary in targets {
        for dx in -1..=1 {
            for dy in -1..=1 {
                let candidate = Vec2::new(primary.pos.x + dx, primary.pos.y + dy);
                if distance(my_pos, candidate) > 4 || !in_bounds_fast(candidate) {
                    continue;
                }
                let mut hits = 0usize;
                let mut value_bonus = 0i32;
                for other in targets {
                    if distance(candidate, other.pos) <= 1 {
                        hits += 1;
                        value_bonus += (other.gems as i32) * 10
                            + if other.submitting { 240 } else { 0 }
                            + if other.recalling { 100 } else { 0 }
                            + if !other.stunned { 50 } else { 0 }
                            + ((1000.0 - other.hp.min(1000.0)) as i32 / 10);
                    }
                }
                if hits == 0 {
                    continue;
                }
                let key = (hits, value_bonus, -distance(my_pos, candidate));
                if best.as_ref().map_or(true, |(b_key, _)| key > *b_key) {
                    best = Some((key, candidate));
                }
            }
        }
    }
    best.map(|(_, tile)| tile)
}

fn try_cast_combat_dash_on_enemies(
    me: &Champion,
    my_pos: Vec2,
    targets: &[EnemyInfo],
) -> Result<bool> {
    if !me.can_cast().unwrap_or(false) || targets.is_empty() {
        return Ok(false);
    }
    let Some(idx) = me
        .abilities()
        .iter()
        .position(|s| s.is_ready().unwrap_or(false) && is_combat_dash(me, s))
    else {
        return Ok(false);
    };
    let slot = &me.abilities()[idx];
    let blocked = raw_blocked_tiles(my_pos)?;
    let candidates = legal_dash_tiles(my_pos, &blocked)?;
    if candidates.is_empty() {
        return Ok(false);
    }

    let can_basic_after = me.can_attack().unwrap_or(false);
    let my_str = effective_basic_attack_damage(me);
    let base_dash_dmg = slot
        .damage_amount()
        .unwrap_or_else(|_| 1.5 * me.focus().unwrap_or(100.0) * unit_missing_hp_damage_mult(me));
    let bushes = cambc::map::bushes().ok();

    let enemy_dmgs: Vec<f32> = targets
        .iter()
        .map(|e| slot.damage_amount_against(&e.unit).unwrap_or(base_dash_dmg))
        .collect();
    let mut ranked: Vec<((i32, usize, i32, i32), Vec2)> = Vec::new();
    for tile in candidates {
        let mut hits = 0usize;
        let mut best_tier = 0i32;
        let mut value_score = 0i32;

        for (ei, e) in targets.iter().enumerate() {
            if distance(tile, e.pos) <= 1 {
                hits += 1;
                let dmg = enemy_dmgs[ei];
                let follow_up = if can_basic_after { my_str } else { 0.0 };
                let killable = dmg + follow_up >= e.hp;
                let tier = if e.submitting {
                    5
                } else if killable {
                    4
                } else if e.recalling {
                    3
                } else if !e.stunned {
                    2
                } else {
                    1
                };
                if tier > best_tier {
                    best_tier = tier;
                }
                value_score += (dmg as i32)
                    + (e.gems as i32) * 16
                    + if killable { 350 } else { 0 }
                    + if e.submitting { 400 } else { 0 }
                    + if e.recalling { 180 } else { 0 }
                    + if !e.stunned { 90 } else { 0 }
                    + ((1000.0 - e.hp.min(1000.0)) as i32 / 8);
            }
        }
        if hits == 0 {
            continue;
        }
        let in_bush = bushes
            .and_then(|b| b.is_set(tile).ok())
            .unwrap_or(false);
        let primary_adj = i32::from(distance(tile, targets[0].pos) == 1);
        let key = (
            best_tier,
            hits,
            value_score + if in_bush { 20 } else { 0 },
            primary_adj,
        );
        ranked.push((key, tile));
    }

    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    let order: Vec<Vec2> = ranked.into_iter().map(|(_, t)| t).collect();
    Ok(cast_dash_to_first_valid(me, idx, &order))
}

fn estimated_my_burst_against(me: &Champion, my_pos: Vec2, target: &EnemyInfo) -> f32 {
    let my_atk_range = me.attack_range().unwrap_or(1);
    let my_str = effective_basic_attack_damage(me);
    let mut total = 0.0f32;
    let d = distance(my_pos, target.pos);
    let can_basic = me.can_attack().unwrap_or(false) && d <= my_atk_range + 1;
    if can_basic {
        total += my_str;
    }
    if me.can_cast().unwrap_or(false) {
        let mut best_spell = 0.0f32;
        for slot in me.abilities() {
            if !slot.is_ready().unwrap_or(false) {
                continue;
            }
            if slot.is_attack_reset() && d <= my_atk_range + 1 {
                best_spell = best_spell.max(my_str);
            } else if is_combat_dash(me, slot) && d <= 3 {
                let dmg = slot
                    .damage_amount_against(&target.unit)
                    .unwrap_or_else(|_| 1.5 * me.focus().unwrap_or(100.0) * unit_missing_hp_damage_mult(me));
                best_spell = best_spell.max(dmg);
            } else if d <= 4 {
                let dmg = slot.damage_amount_against(&target.unit).unwrap_or(0.0);
                best_spell = best_spell.max(dmg);
            }
        }
        total += best_spell;
    }
    total
}

fn try_combat_actions(
    me: &Champion,
    my_pos: Vec2,
    targets: &[EnemyInfo],
    is_varg: bool,
) -> Result<bool> {
    if targets.is_empty() {
        return Ok(false);
    }
    let mut acted = false;
    let is_heart = is_heart_of_ruin(me);
    let is_marcella = matches!(me.champion_name(), Ok(ChampionType::Marcella));
    let my_atk_range = me.attack_range().unwrap_or(1);
    let self_engage_range = if is_varg || is_heart {
        4
    } else {
        my_atk_range.max(3)
    };

    let hp = me.health().unwrap_or(1.0);
    let max_hp = me.max_health().unwrap_or(1.0).max(1.0);
    let hp_ratio = hp / max_hp;
    let at_ally_spawn = cambc::map::spawn_region(Team::Ally)
        .and_then(|s| s.is_set(my_pos))
        .unwrap_or(false);
    let allow_dash_leave = !(at_ally_spawn && hp_ratio < 0.95);

    let has_combat_dash_ready = allow_dash_leave
        && me
            .abilities()
            .iter()
            .any(|s| s.is_ready().unwrap_or(false) && is_combat_dash(me, s));
    let any_enemy_within_3 = targets.iter().any(|e| distance(my_pos, e.pos) <= 3);
    let critical_shield_needed =
        is_heart && hp_ratio < 0.32 && active_shield_value(me) <= 0.0;

    let enemy_close = targets
        .iter()
        .any(|e| distance(my_pos, e.pos) <= self_engage_range);

    let hold_shield_for_dash = is_heart
        && has_combat_dash_ready
        && !critical_shield_needed
        && targets.iter().any(|e| distance(my_pos, e.pos) <= 4);

    if me.can_cast().unwrap_or(false) && enemy_close && !hold_shield_for_dash {
        for slot in me.abilities() {
            if !slot.is_ready().unwrap_or(false) || slot.is_attack_reset() || slot.is_dash() {
                continue;
            }
            if slot.is_shield() || slot.can_target_self().unwrap_or(false) {
                if cast_self_ability(me, slot) {
                    acted = true;
                    break;
                }
            }
        }
    }

    let mut attack_landed = false;
    if me.can_attack().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        let my_str = effective_basic_attack_damage(me);
        let mut in_range_targets: Vec<&EnemyInfo> = targets
            .iter()
            .filter(|t| distance(cur_pos, t.pos) <= my_atk_range)
            .collect();

        let vend_reset_ready = is_vendetta(me)
            && me.can_cast().unwrap_or(false)
            && me.abilities().iter().any(|s| s.is_ready().unwrap_or(false) && s.is_attack_reset());
        let has_kill_and_other = vend_reset_ready
            && in_range_targets.len() >= 2
            && in_range_targets.iter().any(|t| t.hp <= my_str)
            && in_range_targets.iter().any(|t| t.hp > my_str);

        in_range_targets.sort_by(|a, b| {
            let a_kill = a.hp <= my_str;
            let b_kill = b.hp <= my_str;
            let a_two_kill = vend_reset_ready && a.hp > my_str && a.hp <= my_str * 2.0;
            let b_two_kill = vend_reset_ready && b.hp > my_str && b.hp <= my_str * 2.0;
            let a_splash = if is_varg {
                targets.iter().filter(|o| distance(a.pos, o.pos) <= 1).count()
            } else {
                0
            };
            let b_splash = if is_varg {
                targets.iter().filter(|o| distance(b.pos, o.pos) <= 1).count()
            } else {
                0
            };
            let kill_order = if has_kill_and_other {
                a_kill.cmp(&b_kill)
            } else {
                b_kill.cmp(&a_kill)
            };
            b_two_kill
                .cmp(&a_two_kill)
                .then(kill_order)
                .then_with(|| b.submitting.cmp(&a.submitting))
                .then_with(|| b.recalling.cmp(&a.recalling))
                .then_with(|| b_splash.cmp(&a_splash))
                .then_with(|| {
                    a.priority_score
                        .partial_cmp(&b.priority_score)
                        .unwrap_or(Ordering::Equal)
                })
        });

        for target in in_range_targets {
            if me.attack(&target.unit).is_ok() {
                acted = true;
                attack_landed = true;
                break;
            }
        }
    }

    if me.can_cast().unwrap_or(false) && has_combat_dash_ready && any_enemy_within_3 {
        let cur_pos = me.position().unwrap_or(my_pos);
        if try_cast_combat_dash_on_enemies(me, cur_pos, targets)? {
            acted = true;
        }
    }

    if me.can_cast().unwrap_or(false) && is_heart && enemy_close {
        let cur_pos = me.position().unwrap_or(my_pos);
        let in_melee_now = targets.iter().any(|e| distance(cur_pos, e.pos) <= 1);
        if in_melee_now || !has_combat_dash_ready {
            for slot in me.abilities() {
                if slot.is_ready().unwrap_or(false) && slot.is_shield() {
                    if cast_self_ability(me, slot) {
                        acted = true;
                        break;
                    }
                }
            }
        }
    }

    let spell_can_execute_in_range = if me.can_cast().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        me.abilities().iter().any(|slot| {
            if !slot.is_ready().unwrap_or(false) || slot.is_dash() || slot.is_attack_reset() {
                return false;
            }
            if !slot.can_target_enemies().unwrap_or(false) && !slot.can_target_ground().unwrap_or(false) {
                return false;
            }
            targets.iter().any(|e| {
                distance(cur_pos, e.pos) <= 4
                    && e.unit.health().unwrap_or(0.0) > 0.0
                    && slot.damage_amount_against(&e.unit).unwrap_or(0.0)
                        >= e.unit.health().unwrap_or(9999.0)
            })
        })
    } else {
        false
    };

    if me.can_cast().unwrap_or(false) && (!spell_can_execute_in_range || is_marcella) {
        let cur_pos = me.position().unwrap_or(my_pos);
        let enemy_in_basic_range = targets.iter().any(|e| {
            distance(cur_pos, e.pos) <= my_atk_range && e.unit.health().unwrap_or(0.0) > 0.0
        });

        let prefer_marcella_execute = is_marcella
            && spell_can_execute_in_range
            && targets
                .iter()
                .filter(|e| distance(cur_pos, e.pos) <= 1 && e.unit.health().unwrap_or(0.0) > 0.0)
                .all(|e| e.unit.health().unwrap_or(9999.0) > effective_basic_attack_damage(me));

        if enemy_in_basic_range
            && !prefer_marcella_execute
            && (attack_landed || !me.can_attack().unwrap_or(false))
        {
            for slot in me.abilities() {
                if slot.is_ready().unwrap_or(false) && slot.is_attack_reset() {
                    if cast_self_ability(me, slot) {
                        acted = true;
                        if me.can_attack().unwrap_or(false) {
                            let my_str = effective_basic_attack_damage(me);
                            let mut follow_targets: Vec<&EnemyInfo> = targets
                                .iter()
                                .filter(|t| {
                                    distance(cur_pos, t.pos) <= my_atk_range
                                        && t.unit.health().unwrap_or(0.0) > 0.0
                                })
                                .collect();
                            follow_targets.sort_by(|a, b| {
                                let a_hp = a.unit.health().unwrap_or(a.hp);
                                let b_hp = b.unit.health().unwrap_or(b.hp);
                                let a_kill = a_hp <= my_str;
                                let b_kill = b_hp <= my_str;
                                b_kill
                                    .cmp(&a_kill)
                                    .then_with(|| a.stunned.cmp(&b.stunned))
                                    .then_with(|| {
                                        a.priority_score
                                            .partial_cmp(&b.priority_score)
                                            .unwrap_or(Ordering::Equal)
                                    })
                            });
                            for target in follow_targets {
                                if me.attack(&target.unit).is_ok() {
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    if me.can_cast().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        for slot in me.abilities() {
            if !slot.is_ready().unwrap_or(false) || slot.is_dash() || slot.is_attack_reset() {
                continue;
            }

            let is_cc = slot.is_cc();
            let is_aoe = slot.is_aoe_damage();
            let can_enemy = slot.can_target_enemies().unwrap_or(false);
            let can_ground = slot.can_target_ground().unwrap_or(false);

            if can_ground && (is_aoe || !can_enemy) {
                if let Some(best_tile) = best_aoe_ground_tile_for_enemies(cur_pos, targets) {
                    if slot.cast_at(best_tile).unwrap_or(false) {
                        acted = true;
                        break;
                    }
                }
            }

            if can_enemy {
                let mut scored: Vec<(i32, f32, i32, &EnemyInfo)> = targets
                    .iter()
                    .filter(|e| distance(cur_pos, e.pos) <= 4 && e.unit.health().unwrap_or(0.0) > 0.0)
                    .map(|e| {
                        let cur_ehp = e.unit.health().unwrap_or(e.hp);
                        let dmg = slot.damage_amount_against(&e.unit).unwrap_or(0.0);
                        let killable = dmg >= cur_ehp && dmg > 0.0;
                        let interrupt_tribute = e.submitting && (is_cc || killable);
                        let interrupt_recall = e.recalling && (is_cc || dmg > 0.0);
                        let avoid_redundant_cc = is_cc && dmg == 0.0 && e.stunned;
                        let tier = if avoid_redundant_cc {
                            -1
                        } else if interrupt_tribute {
                            5
                        } else if killable {
                            4
                        } else if interrupt_recall {
                            3
                        } else if is_cc && !e.stunned {
                            2
                        } else {
                            1
                        };
                        let score =
                            dmg + (e.gems as f32) * 10.0 + (1000.0 - cur_ehp.min(1000.0)) * 0.12;
                        (tier, score, -distance(cur_pos, e.pos), e)
                    })
                    .filter(|(tier, _, _, _)| *tier > 0)
                    .collect();

                scored.sort_by(|a, b| {
                    b.0.cmp(&a.0)
                        .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal))
                        .then_with(|| b.2.cmp(&a.2))
                });

                let mut cast_done = false;
                for (_, _, _, target) in scored {
                    if slot.cast_on(&target.unit).unwrap_or(false) {
                        acted = true;
                        cast_done = true;
                        break;
                    }
                    if can_ground && slot.cast_at(target.pos).unwrap_or(false) {
                        acted = true;
                        cast_done = true;
                        break;
                    }
                }
                if cast_done {
                    break;
                }
            }
        }
    }

    if me.can_attack().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        for target in targets {
            if distance(cur_pos, target.pos) <= my_atk_range
                && target.unit.health().unwrap_or(0.0) > 0.0
                && me.attack(&target.unit).is_ok()
            {
                acted = true;
                break;
            }
        }
    }

    Ok(acted)
}

fn rough_dps(unit: &Champion) -> f32 {
    let base = unit.strength().unwrap_or(40.0) * 1.125 + unit.focus().unwrap_or(100.0) * 0.25;
    if is_heart_of_ruin(unit) {
        let mult = unit_missing_hp_damage_mult(unit).max(1.18);
        (base + unit.focus().unwrap_or(100.0) * 0.08) * mult
    } else if is_vendetta(unit) {
        base * 1.25
    } else {
        base
    }
}

fn losing_matchup(
    me: &Champion,
    my_pos: Vec2,
    enemies: &[EnemyInfo],
    allies: &[Champion],
    max_enemy_dist: i32,
) -> bool {
    let close: Vec<&EnemyInfo> = enemies
        .iter()
        .filter(|e| e.dist <= max_enemy_dist)
        .collect();
    if close.is_empty() {
        return false;
    }

    let my_hp = me.health().unwrap_or(1.0);
    let my_ehp = my_hp + potential_shield_value(me, true);
    let my_burst = estimated_my_burst_against(me, my_pos, close[0]);

    if close.len() == 1
        && (close[0].hp + active_shield_value(&close[0].unit) <= my_burst
            || close[0].hp / close[0].max_hp.max(1.0) < 0.28)
    {
        return false;
    }

    let incoming_burst: f32 = close
        .iter()
        .filter(|e| !e.stunned && !e.submitting && !e.recalling && e.dist <= e.interrupt_radius)
        .map(|e| e.burst_damage)
        .sum();
    if incoming_burst >= my_ehp * 0.95 && my_burst < close[0].hp + active_shield_value(&close[0].unit) {
        return true;
    }

    let mut team_hp = my_ehp;
    let mut team_dps = rough_dps(me);
    for ally in allies {
        let Ok(p) = ally.position() else { continue };
        let hp = ally.health().unwrap_or(0.0);
        if p != my_pos && hp > 0.0 && distance(my_pos, p) <= max_enemy_dist + 1 {
            team_hp += hp + potential_shield_value(ally, false);
            team_dps += rough_dps(ally);
        }
    }

    let twins = crate::profile::has(crate::profile::TWINS);
    let enemy_hp: f32 = close
        .iter()
        .map(|e| {
            let ehp = e.hp + potential_shield_value(&e.unit, false);
            if twins
                && matches!(
                    e.unit.champion_name(),
                    Ok(ChampionType::Cassi | ChampionType::Issac)
                )
            {
                0.5 * ehp
            } else {
                ehp
            }
        })
        .sum();
    let enemy_dps: f32 = close
        .iter()
        .map(|e| {
            let raw = rough_dps(&e.unit);
            if e.all_abilities_on_cd {
                raw * 0.72
            } else {
                raw
            }
        })
        .sum();

    let my_gems = me.gem_count().unwrap_or(0);
    let risk_factor = if my_gems >= 35 && close[0].gems < 15 {
        0.78
    } else if close[0].gems >= 25 && my_gems <= 10 {
        1.08
    } else {
        0.90
    };

    let turns_to_win = enemy_hp / team_dps.max(1.0);
    let turns_to_lose = team_hp / enemy_dps.max(1.0);
    turns_to_lose * risk_factor < turns_to_win
}

fn spawn_distance(pos: Vec2, spawn_positions: &[Vec2]) -> i32 {
    spawn_positions
        .iter()
        .map(|&p| distance(pos, p))
        .min()
        .unwrap_or(99)
}

fn try_kite_away(me: &Champion, my_pos: Vec2, enemies: &[EnemyInfo]) -> Result<bool> {
    let atk_range = me.attack_range().unwrap_or(1);
    if atk_range <= 1 {
        return Ok(false);
    }
    let min_cur_dist = enemies
        .iter()
        .map(|e| distance(my_pos, e.pos))
        .min()
        .unwrap_or(99);
    let melee_closing_in = enemies
        .iter()
        .any(|e| e.atk_range == 1 && !e.stunned && distance(my_pos, e.pos) <= 2);
    if min_cur_dist > 1 && !melee_closing_in {
        return Ok(false);
    }

    let raw_blk = raw_blocked_tiles(my_pos)?;
    let bushes = cambc::map::bushes().ok();
    let holes = cambc::map::holes().ok();

    if let Some(idx) = dash_ready(me) {
        if let Ok(dash_tiles) = legal_dash_tiles(my_pos, &raw_blk) {
            let mut dash_cands: Vec<(i32, bool, i32, Vec2)> = Vec::new();
            for tile in dash_tiles {
                let min_d = enemies
                    .iter()
                    .map(|e| distance(tile, e.pos))
                    .min()
                    .unwrap_or(99);
                if min_d > min_cur_dist && min_d >= 2 {
                    let in_sweet = if min_d == atk_range {
                        2
                    } else if (2..=atk_range).contains(&min_d) {
                        1
                    } else {
                        0
                    };
                    let out_of_danger = !DANGER.with(|d| {
                        d.borrow()
                            .as_ref()
                            .is_some_and(|z| z.is_set(tile).unwrap_or(false))
                    });
                    let in_bush = bushes
                        .and_then(|b| b.is_set(tile).ok())
                        .unwrap_or(false);
                    dash_cands.push((
                        in_sweet,
                        out_of_danger,
                        min_d * 4 + if in_bush { 6 } else { 0 },
                        tile,
                    ));
                }
            }
            dash_cands.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| b.1.cmp(&a.1))
                    .then_with(|| b.2.cmp(&a.2))
            });
            let order: Vec<Vec2> = dash_cands.into_iter().map(|(_, _, _, t)| t).collect();
            if cast_dash_to_first_valid(me, idx, &order) {
                PREV_POS.with(|p| p.set(Some(my_pos)));
                return Ok(true);
            }
        }
    }

    if !me.can_move().unwrap_or(false) {
        return Ok(false);
    }

    let mut candidates: Vec<(i32, bool, i32, Vec2)> = Vec::new();
    for tile in legal_step_neighbors(my_pos, &raw_blk) {
        let min_enemy_dist = enemies
            .iter()
            .map(|e| distance(tile, e.pos))
            .min()
            .unwrap_or(99);
        if min_enemy_dist > min_cur_dist && min_enemy_dist >= 2 {
            let in_sweet_spot = if min_enemy_dist == atk_range {
                2
            } else if (2..=atk_range).contains(&min_enemy_dist) {
                1
            } else {
                0
            };
            let out_of_danger = !DANGER.with(|d| {
                d.borrow()
                    .as_ref()
                    .is_some_and(|z| z.is_set(tile).unwrap_or(false))
            });
            let in_bush = bushes
                .and_then(|b| b.is_set(tile).ok())
                .unwrap_or(false);
            let hole_screen = holes
                .map(|h| {
                    enemies.iter().any(|e| {
                        let mid = Vec2::new((tile.x + e.pos.x) / 2, (tile.y + e.pos.y) / 2);
                        h.is_set(mid).unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            let terrain_bonus = min_enemy_dist * 4
                + if in_bush { 6 } else { 0 }
                + if hole_screen { 7 } else { 0 };
            candidates.push((in_sweet_spot, out_of_danger, terrain_bonus, tile));
        }
    }
    candidates.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
    });
    for (_, _, _, tile) in candidates {
        if me.move_absolute(tile).unwrap_or(false) {
            PREV_POS.with(|p| p.set(Some(my_pos)));
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_ready_exclusive_offensive_spell_in_range(
    me: &Champion,
    my_pos: Vec2,
    enemies: &[EnemyInfo],
) -> bool {
    if !me.can_cast().unwrap_or(false) {
        return false;
    }
    for slot in me.abilities() {
        if !slot.is_ready().unwrap_or(false) || slot.is_dash() || slot.is_attack_reset() {
            continue;
        }
        let Ok(Some(info)) = slot.info() else {
            continue;
        };
        if !info.as_dyn().is_exclusive_with_movement() {
            continue;
        }
        let max_r = if slot.is_aoe_damage() || matches!(me.champion_name(), Ok(ChampionType::Ryth)) {
            5
        } else {
            4
        };
        if enemies
            .iter()
            .any(|e| distance(my_pos, e.pos) <= max_r && e.unit.health().unwrap_or(0.0) > 0.0)
        {
            return true;
        }
    }
    false
}

fn engage_enemies(
    me: &Champion,
    my_pos: Vec2,
    hp_ratio: f32,
    enemies: &[EnemyInfo],
    allies: &[Champion],
    is_varg: bool,
    altars: &[Vec2],
    near_camps: &[NearCamp],
) -> Result<bool> {
    if enemies.is_empty() {
        return Ok(false);
    }
    let is_heart = is_heart_of_ruin(me);
    let atk_range = me.attack_range().unwrap_or(1);

    if enemies.iter().any(|e| e.submitting) {
        for &altar in altars {
            if distance(my_pos, altar) <= 2
                && !cambc::vision::has_altar_lock(altar).unwrap_or(true)
            {
                for item in me.consumables().iter() {
                    if item.kind() == ConsumableType::AltarLock
                        && item.count().unwrap_or(0) > 0
                        && item.use_at(altar).unwrap_or(false)
                    {
                        break;
                    }
                }
            }
        }
    }

    let camp_in_range = near_camps.iter().any(|c| c.dist <= 3 && c.hp > 0.0);
    let target_vulnerable = enemies[0].hp / enemies[0].max_hp.max(1.0) < 0.42
        || enemies[0].submitting
        || enemies[0].recalling
        || enemies[0].gems >= 15;

    if camp_in_range && enemies[0].dist > 3 && !target_vulnerable {
        return Ok(false);
    }

    let has_exclusive_spell_now =
        has_ready_exclusive_offensive_spell_in_range(me, my_pos, enemies);
    let has_reset_or_walk_spell = me.abilities().iter().any(|s| {
        s.is_ready().unwrap_or(false)
            && (s.is_attack_reset()
                || (s.is_cc()
                    && !s.is_dash()
                    && s.info()
                        .ok()
                        .flatten()
                        .is_some_and(|i| !i.as_dyn().is_exclusive_with_movement())))
    });

    if atk_range > 1
        && !has_exclusive_spell_now
        && enemies.iter().any(|e| distance(my_pos, e.pos) <= 1 || (e.atk_range == 1 && !e.stunned && distance(my_pos, e.pos) <= 2))
        && (dash_ready(me).is_some() || has_reset_or_walk_spell || hp_ratio < 0.72)
    {
        if try_kite_away(me, my_pos, enemies)? {
            let new_pos = me.position().unwrap_or(my_pos);
            let _ = try_combat_actions(me, new_pos, enemies, is_varg)?;
            return Ok(true);
        }
    }

    let mut acted = try_combat_actions(me, my_pos, enemies, is_varg)?;
    let cur_pos = me.position().unwrap_or(my_pos);

    if !acted && try_kite_away(me, cur_pos, enemies)? {
        let new_pos = me.position().unwrap_or(cur_pos);
        let _ = try_combat_actions(me, new_pos, enemies, is_varg)?;
        return Ok(true);
    }

    if acted && atk_range > 1 && me.can_move().unwrap_or(false) {
        let _ = try_kite_away(me, cur_pos, enemies);
    }

    let engage_hp_floor = if target_vulnerable {
        PILGRIM_THRESHOLD
    } else if is_varg || is_heart {
        0.48
    } else {
        0.55
    };

    if (me.can_move()? || dash_ready(me).is_some())
        && hp_ratio > engage_hp_floor
        && (!camp_in_range || target_vulnerable || enemies[0].dist <= 2)
        && !losing_matchup(me, cur_pos, enemies, allies, 5)
    {
        let any_in_basic_range = enemies
            .iter()
            .any(|e| distance(cur_pos, e.pos) <= atk_range && e.unit.health().unwrap_or(0.0) > 0.0);

        if !any_in_basic_range {
            COMBAT_ENGAGE_MODE.with(|m| m.set(true));
            let engaged_via_dash = dash_engage(me, cur_pos, enemies, is_varg).unwrap_or(false);
            if engaged_via_dash {
                COMBAT_ENGAGE_MODE.with(|m| m.set(false));
                return Ok(true);
            }

            if me.can_move()? {
                let desired_range = atk_range.max(1);
                let mut approach_tiles = tiles_around(enemies[0].pos, desired_range)?;
                if desired_range >= 3 && enemies[0].atk_range == 1 {
                    if let Ok(inner) = tiles_around(enemies[0].pos, desired_range - 1) {
                        if let Ok(inner_comp) = inner.complement() {
                            if let Ok(outer_ring) = approach_tiles.intersect(&inner_comp) {
                                if !outer_ring.is_empty().unwrap_or(true) {
                                    approach_tiles = outer_ring;
                                }
                            }
                        }
                    }
                }
                if move_towards_closest(approach_tiles).unwrap_or(false) {
                    acted = true;
                    let new_pos = me.position().unwrap_or(cur_pos);
                    let _ = try_combat_actions(me, new_pos, enemies, is_varg);
                }
            }
            COMBAT_ENGAGE_MODE.with(|m| m.set(false));
        }
    }

    Ok(acted)
}

pub fn try_straight(
    me: &Champion,
    origin: Vec2,
    goals: &Bitboard,
    blocked: &Bitboard,
) -> Result<bool> {
    if goals.count()? > MAX_STRAIGHT_GOALS as usize {
        return Ok(false);
    }
    let Some(goal) = oriented(goals.positions()?)
        .into_iter()
        .min_by_key(|g| distance(origin, *g))
    else {
        return Ok(false);
    };
    let Some(step) = straight_step(origin, goal, blocked)? else {
        return Ok(false);
    };
    if blocked.is_set(step)? {
        return Ok(false);
    }
    Ok(me.move_absolute(step).unwrap_or(false))
}

fn straight_step(origin: Vec2, goal: Vec2, blocked: &Bitboard) -> Result<Option<Vec2>> {
    let line = create_line(origin, goal);
    for t in &line {
        if *t != origin && *t != goal && blocked.is_set(*t)? {
            return Ok(None);
        }
    }
    let step = line
        .iter()
        .copied()
        .filter(|t| distance(origin, *t) == 1)
        .min_by_key(|t| distance(*t, goal));
    Ok(step.filter(|s| !blocked.is_set(*s).unwrap_or(true)))
}

fn heal_lowest_ally(me: &Champion, my_pos: Vec2, allies: &[Champion]) -> Result<bool> {
    let Some(ready_slot) = me.abilities().iter().find(|s| {
        s.is_ready().unwrap_or(false) && s.can_target_allies().unwrap_or(false)
    }) else {
        return Ok(false);
    };

    let ally_spawn = cambc::map::spawn_region(Team::Ally).ok();
    let mut best_in_range: Option<(f32, &Champion)> = None;
    let mut best_out_of_range: Option<(f32, Vec2)> = None;

    for ally in allies {
        let (Ok(hp), Ok(max), Ok(pos)) = (ally.health(), ally.max_health(), ally.position())
        else {
            continue;
        };
        if pos == my_pos || hp <= 0.0 || hp >= max * 0.86 {
            continue;
        }
        if ally.is_recalling().unwrap_or(false)
            || ally_spawn.and_then(|s| s.is_set(pos).ok()).unwrap_or(false)
        {
            continue;
        }
        let ratio = hp / max.max(1.0);
        let effective_score = ratio - (ally.gem_count().unwrap_or(0) as f32) * 0.003;
        let d = distance(my_pos, pos);
        if d <= 4 {
            if best_in_range.as_ref().map_or(true, |(r, _)| effective_score < *r) {
                best_in_range = Some((effective_score, ally));
            }
        } else if ratio < 0.70
            && best_out_of_range
                .as_ref()
                .map_or(true, |(r, _)| effective_score < *r)
        {
            best_out_of_range = Some((effective_score, pos));
        }
    }

    if let Some((_, ally)) = best_in_range {
        if ready_slot.cast_on(ally).unwrap_or(false) {
            return Ok(true);
        }
    }
    if let Some((_, ally_pos)) = best_out_of_range {
        if me.can_move().unwrap_or(false) {
            if let Ok(ring) = tiles_around(ally_pos, 4) {
                if move_towards_closest(ring)? {
                    let new_pos = me.position()?;
                    for ally in allies {
                        if let Ok(pos) = ally.position() {
                            if distance(new_pos, pos) <= 4 {
                                let _ = ready_slot.cast_on(ally);
                                break;
                            }
                        }
                    }
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn try_cast_combat_dash_on_camps(
    me: &Champion,
    my_pos: Vec2,
    live_camps: &[&NearCamp],
    my_str: f32,
) -> bool {
    if !me.can_cast().unwrap_or(false) || live_camps.is_empty() {
        return false;
    }
    let hp_ratio = me.health().unwrap_or(1.0) / me.max_health().unwrap_or(1.0).max(1.0);
    if hp_ratio <= RECALL_THRESHOLD + 0.02 {
        return false;
    }
    let Some(idx) = me
        .abilities()
        .iter()
        .position(|s| s.is_ready().unwrap_or(false) && is_combat_dash(me, s))
    else {
        return false;
    };

    let worth_dashing = live_camps.len() >= 2
        || live_camps
        .iter()
        .any(|c| (c.strength > 0.0 && c.hp > my_str) || distance(my_pos, c.pos) > 1);
    if !worth_dashing {
        return false;
    }

    let Ok(blocked) = blocked_tiles(my_pos) else {
        return false;
    };
    let Ok(positions) = legal_dash_tiles(my_pos, &blocked) else {
        return false;
    };

    let primary_pos = live_camps[0].pos;
    let mut ranked: Vec<((usize, usize, i32, i32), Vec2)> = Vec::new();
    for tile in positions {
        let mut hits = 0usize;
        let mut threat_hits = 0usize;
        let mut total_hp_score = 0i32;

        for c in live_camps {
            if distance(tile, c.pos) <= 1 {
                hits += 1;
                if c.strength > 0.0 {
                    threat_hits += 1;
                }
                total_hp_score += (c.hp.min(2000.0) as i32) / 10;
            }
        }
        if hits == 0 {
            continue;
        }
        let adj_primary = i32::from(distance(tile, primary_pos) == 1);
        ranked.push(((hits, threat_hits, adj_primary, total_hp_score), tile));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    let order: Vec<Vec2> = ranked.into_iter().map(|(_, t)| t).collect();
    if !cast_dash_to_first_valid(me, idx, &order) {
        return false;
    }
    if let Ok(new_pos) = me.position() {
        let in_melee = live_camps
            .iter()
            .any(|c| c.unit.health().unwrap_or(0.0) > 0.0 && distance(new_pos, c.pos) <= 1);
        if !in_melee && me.can_move().unwrap_or(false) {
            if let Ok(mut ring) = tiles_around(primary_pos, 1) {
                let _ = ring.set(primary_pos, false);
                let _ = move_towards_closest(ring);
            }
        }
    }
    true
}

fn try_attack_camps_in_range(me: &Champion, camps: &[&NearCamp]) -> bool {
    if camps.is_empty() {
        return false;
    }
    let Ok(my_pos) = me.position() else {
        return false;
    };
    let is_varg = matches!(me.champion_name(), Ok(ChampionType::Varg));
    let is_heart = is_heart_of_ruin(me);
    let is_vend = is_vendetta(me);
    let my_atk_range = me.attack_range().unwrap_or(1);

    let mut live_camps: Vec<&NearCamp> = camps
        .iter()
        .copied()
        .filter(|c| distance(my_pos, c.pos) <= 5 && c.hp > 0.0)
        .collect();

    if live_camps.is_empty() {
        return false;
    }
    live_camps.sort_by_key(|c| distance(my_pos, c.pos));
    live_camps.truncate(6);

    let my_str = effective_basic_attack_damage(me);
    let vend_reset_ready = is_vend
        && me.can_cast().unwrap_or(false)
        && me.abilities().iter().any(|s| s.is_ready().unwrap_or(false) && s.is_attack_reset());
    let in_basic_camps: Vec<&NearCamp> = live_camps
        .iter()
        .copied()
        .filter(|c| distance(my_pos, c.pos) <= my_atk_range)
        .collect();
    let vend_defer_one_shot = vend_reset_ready
        && in_basic_camps.len() >= 2
        && in_basic_camps.iter().any(|c| c.hp <= my_str)
        && in_basic_camps.iter().any(|c| c.hp > my_str);

    let camp_positions: Vec<Vec2> = live_camps.iter().map(|c| c.pos).collect();
    live_camps.sort_by(|a, b| {
        let a_one_shot = a.hp <= my_str;
        let b_one_shot = b.hp <= my_str;
        let a_two_shot = is_vend && a.hp > my_str && a.hp <= my_str * 2.0;
        let b_two_shot = is_vend && b.hp > my_str && b.hp <= my_str * 2.0;
        let a_neighbors = if is_varg || is_heart {
            camp_positions
                .iter()
                .filter(|&&p| distance(a.pos, p) <= 1)
                .count()
        } else {
            0
        };
        let b_neighbors = if is_varg || is_heart {
            camp_positions
                .iter()
                .filter(|&&p| distance(b.pos, p) <= 1)
                .count()
        } else {
            0
        };
        let one_shot_order = if vend_defer_one_shot {
            a_one_shot.cmp(&b_one_shot)
        } else {
            b_one_shot.cmp(&a_one_shot)
        };
        b_two_shot
            .cmp(&a_two_shot)
            .then(one_shot_order)
            .then_with(|| b_neighbors.cmp(&a_neighbors))
            .then_with(|| a.hp.partial_cmp(&b.hp).unwrap_or(Ordering::Equal))
            .then_with(|| distance(my_pos, a.pos).cmp(&distance(my_pos, b.pos)))
    });

    let mut acted = false;
    let hp_ratio = me.health().unwrap_or(1.0) / me.max_health().unwrap_or(1.0).max(1.0);
    let has_combat_dash_ready = me
        .abilities()
        .iter()
        .any(|s| s.is_ready().unwrap_or(false) && is_combat_dash(me, s));

    let taking_camp_damage = live_camps
        .iter()
        .any(|c| c.strength > 0.0 && distance(my_pos, c.pos) <= 4);
    let prefer_dash_over_shield =
        is_heart && has_combat_dash_ready && hp_ratio >= 0.34 && has_fuel(MIN_FUEL_FOR_SEARCH);

    if taking_camp_damage && me.can_cast().unwrap_or(false) && !prefer_dash_over_shield {
        for slot in me.abilities() {
            if slot.is_ready().unwrap_or(false) && slot.is_shield() {
                if cast_self_ability(me, slot) {
                    acted = true;
                    break;
                }
            }
        }
    }

    let mut basic_landed = false;
    if me.can_attack().unwrap_or(false) {
        for c in &live_camps {
            if distance(my_pos, c.pos) <= my_atk_range && me.attack(&c.unit).is_ok() {
                acted = true;
                basic_landed = true;
                break;
            }
        }
    }

    if me.can_cast().unwrap_or(false) && has_combat_dash_ready && has_fuel(MIN_FUEL_FOR_SEARCH) {
        let cur_pos = me.position().unwrap_or(my_pos);
        if try_cast_combat_dash_on_camps(me, cur_pos, &live_camps, my_str) {
            acted = true;
        }
    }

    if taking_camp_damage && me.can_cast().unwrap_or(false) {
        for slot in me.abilities() {
            if slot.is_ready().unwrap_or(false) && slot.is_shield() {
                if cast_self_ability(me, slot) {
                    acted = true;
                    break;
                }
            }
        }
    }

    if me.can_cast().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        let any_in_basic = live_camps.iter().any(|c| {
            distance(cur_pos, c.pos) <= my_atk_range && c.unit.health().unwrap_or(0.0) > 0.0
        });
        if any_in_basic && (basic_landed || !me.can_attack().unwrap_or(false)) {
            for slot in me.abilities() {
                if slot.is_ready().unwrap_or(false) && slot.is_attack_reset() {
                    if cast_self_ability(me, slot) {
                        acted = true;
                        if me.can_attack().unwrap_or(false) {
                            let mut follow_camps: Vec<&NearCamp> = live_camps
                                .iter()
                                .copied()
                                .filter(|c| {
                                    distance(cur_pos, c.pos) <= my_atk_range
                                        && c.unit.health().unwrap_or(0.0) > 0.0
                                })
                                .collect();
                            follow_camps.sort_by(|a, b| {
                                let ahp = a.unit.health().unwrap_or(a.hp);
                                let bhp = b.unit.health().unwrap_or(b.hp);
                                let akill = ahp <= my_str;
                                let bkill = bhp <= my_str;
                                bkill
                                    .cmp(&akill)
                                    .then_with(|| ahp.partial_cmp(&bhp).unwrap_or(Ordering::Equal))
                            });
                            for c in follow_camps {
                                if me.attack(&c.unit).is_ok() {
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    if me.can_cast().unwrap_or(false) && has_fuel(MIN_FUEL_FOR_SEARCH) {
        let cur_pos = me.position().unwrap_or(my_pos);
        for slot in me.abilities() {
            if !slot.is_ready().unwrap_or(false) || slot.is_dash() || slot.is_attack_reset() {
                continue;
            }
            let can_camp = slot.can_target_camps().unwrap_or(false);
            let can_ground = slot.can_target_ground().unwrap_or(false);
            let is_aoe = slot.is_aoe_damage();

            if can_ground && (is_aoe || !can_camp) && (is_aoe || slot.is_damage() || slot.is_cc()) {
                let mut best_tile: Option<(usize, Vec2)> = None;
                for c in &live_camps {
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            let tile = Vec2::new(c.pos.x + dx, c.pos.y + dy);
                            if distance(cur_pos, tile) > 4 || !in_bounds_fast(tile) {
                                continue;
                            }
                            let hits = live_camps
                                .iter()
                                .filter(|o| {
                                    distance(tile, o.pos) <= 1
                                        && o.unit.health().unwrap_or(0.0) > 0.0
                                })
                                .count();
                            if hits > 0
                                && best_tile.as_ref().map_or(true, |(bh, _)| hits > *bh)
                            {
                                best_tile = Some((hits, tile));
                            }
                        }
                    }
                }
                if let Some((_, tile)) = best_tile {
                    if slot.cast_at(tile).unwrap_or(false) {
                        acted = true;
                        break;
                    }
                }
            }

            if can_camp {
                let mut cast_done = false;
                for c in live_camps
                    .iter()
                    .filter(|c| distance(cur_pos, c.pos) <= 4)
                    .take(3)
                {
                    if c.unit.health().unwrap_or(0.0) > 0.0 {
                        if slot.cast_on(&c.unit).unwrap_or(false) {
                            acted = true;
                            cast_done = true;
                            break;
                        }
                        if can_ground && slot.cast_at(c.pos).unwrap_or(false) {
                            acted = true;
                            cast_done = true;
                            break;
                        }
                    }
                }
                if cast_done {
                    break;
                }
            }
        }
    }

    if me.can_attack().unwrap_or(false) {
        let cur_pos = me.position().unwrap_or(my_pos);
        for c in &live_camps {
            if distance(cur_pos, c.pos) <= my_atk_range
                && c.unit.health().unwrap_or(0.0) > 0.0
                && me.attack(&c.unit).is_ok()
            {
                acted = true;
                break;
            }
        }
    }

    acted
}

fn losing_camp_fight(
    me: &Champion,
    my_pos: Vec2,
    completed_passives: u16,
    near_camps: &[NearCamp],
    allies: &[Champion],
) -> Result<Option<Vec<Vec2>>> {
    let mut threats = Vec::new();
    let (mut camp_hp, mut camp_dps) = (0.0f32, 0.0f32);
    for camp in near_camps {
        if camp.hp <= 0.0 || distance(my_pos, camp.pos) > 4 {
            continue;
        }
        camp_hp += camp.hp;
        if camp.strength > 0.0 {
            camp_dps += camp.strength;
            threats.push(camp.pos);
        }
    }
    if threats.is_empty() {
        return Ok(None);
    }
    let ally_count = allies
        .iter()
        .filter(|a| a.position().map_or(false, |p| p != my_pos && distance(my_pos, p) <= 4))
        .count() as f32;
    let camp_dmg_mult = if (completed_passives & PASSIVE_MONSTER) != 0 {
        if PATCH_ADAPT_MONSTER_115 { 1.15 } else { 1.10 }
    } else {
        1.0
    };
    let mut taken_mult = 1.0f32;
    if (completed_passives & PASSIVE_JUNGLE) != 0 {
        taken_mult *= 0.70;
    }
    if (completed_passives & PASSIVE_DURABILITY) != 0 {
        taken_mult *= 0.90;
    }
    let my_dps = rough_dps(me) * camp_dmg_mult;
    let effective_camp_dps = camp_dps * taken_mult;
    let my_ehp = me.health()? + potential_shield_value(me, true);
    let turns_to_kill = camp_hp / (my_dps * (1.0 + ally_count)).max(1.0);
    let turns_to_die = my_ehp / (effective_camp_dps / (1.0 + ally_count)).max(1.0);
    let nearest = threats.iter().map(|&t| distance(my_pos, t)).min().unwrap_or(4);
    let escape_turns = (4 - nearest).max(0) as f32 + 1.0;
    if turns_to_die < turns_to_kill && turns_to_die <= 5.0 && turns_to_die > escape_turns {
        Ok(Some(threats))
    } else {
        Ok(None)
    }
}

fn flee_from(me: &Champion, my_pos: Vec2, threats: &[Vec2]) -> Result<bool> {
    let mut goals = tiles_around(my_pos, 8)?;
    for &t in threats {
        goals.intersect_assign(&tiles_around(t, 4)?.complement()?)?;
    }
    let blocked = blocked_tiles(my_pos)?;
    goals.intersect_assign(&blocked.complement()?)?;
    if goals.is_empty()? {
        return Ok(false);
    }
    if dash_retreat(me, my_pos, &goals, &blocked, &[])? {
        return Ok(true);
    }
    if me.can_cast().unwrap_or(false) {
        for slot in me.abilities() {
            if slot.is_ready().unwrap_or(false) && slot.is_shield() {
                let _ = cast_self_ability(me, slot);
                break;
            }
        }
    }
    move_towards_closest_with_blocked(me, my_pos, goals, &blocked)
}

fn has_owned_altar_lock(me: &Champion) -> bool {
    me.consumables()
        .iter()
        .any(|c| c.kind() == ConsumableType::AltarLock && c.count().unwrap_or(0) > 0)
}

fn has_owned_teleport_crystal(me: &Champion) -> bool {
    me.consumables()
        .iter()
        .any(|c| c.kind() == ConsumableType::TpCrystal && c.count().unwrap_or(0) > 0)
}

fn try_place_altar_lock(me: &Champion, pos: Vec2, altar: Vec2, has_lock_flag: &mut bool) {
    if distance(pos, altar) > 2 {
        return;
    }
    if !*has_lock_flag && !has_owned_altar_lock(me) {
        return;
    }
    if !cambc::vision::has_altar_lock(altar).unwrap_or(true) {
        for item in me.consumables().iter() {
            if item.kind() == ConsumableType::AltarLock
                && item.count().unwrap_or(0) > 0
                && item.use_at(altar).unwrap_or(false)
            {
                *has_lock_flag = false;
                break;
            }
        }
    }
}

fn manage_nearby_altar_locks(
    me: &Champion,
    my_pos: Vec2,
    altars: &[Vec2],
    has_lock_flag: &mut bool,
    allow_attack: bool,
) {
    let atk_range = me.attack_range().unwrap_or(1);
    for &altar in altars {
        let d = distance(my_pos, altar);
        if d <= 2 {
            try_place_altar_lock(me, my_pos, altar, has_lock_flag);
        }
        if allow_attack && me.can_attack().unwrap_or(false) && d <= atk_range {
            if let Ok(lock) = cambc::vision::altar_lock_at(altar) {
                if lock.is_enemy() && me.attack(&lock).is_ok() {
                    if me.can_cast().unwrap_or(false) {
                        for slot in me.abilities() {
                            if slot.is_ready().unwrap_or(false) && slot.is_attack_reset() {
                                if cast_self_ability(me, slot) {
                                    let _ = me.attack(&lock);
                                    break;
                                }
                            }
                        }
                    }
                    try_place_altar_lock(me, my_pos, altar, has_lock_flag);
                }
            }
        }
    }
}

fn try_place_free_ward(me: &Champion, my_pos: Vec2, hit_unseen: bool) {
    if !has_fuel(MIN_FUEL_FOR_WARD) {
        return;
    }
    if me.ward_cooldown().unwrap_or(1) != 0 {
        return;
    }
    let Ok(bushes) = cambc::map::bushes() else { return };

    let mut bush_tiles = Vec::with_capacity(12);
    for dx in -2..=2 {
        for dy in -2..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let tile = Vec2::new(my_pos.x + dx, my_pos.y + dy);
            if in_bounds_fast(tile) && bushes.is_set(tile).unwrap_or(false) {
                bush_tiles.push(tile);
            }
        }
    }

    if bush_tiles.is_empty() && !hit_unseen {
        return;
    }

    if let Ok(existing) = me.ward_positions() {
        if existing.iter().any(|&w| distance(my_pos, w) <= 4) {
            return;
        }
    }

    let prev = PREV_POS.with(|p| p.get()).unwrap_or(my_pos);
    let move_dx = (my_pos.x - prev.x).signum();
    let move_dy = (my_pos.y - prev.y).signum();

    if !bush_tiles.is_empty() {
        let mut ranked: Vec<(i32, Vec2)> = bush_tiles
            .into_iter()
            .map(|tile| {
                let d = distance(my_pos, tile);
                let dir_align = (tile.x - my_pos.x) * move_dx + (tile.y - my_pos.y) * move_dy;
                (d * 12 + dir_align * 6, tile)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, target) in ranked.into_iter().take(4) {
            if me.place_ward(target).unwrap_or(false) {
                return;
            }
        }
    }

    if hit_unseen {
        let Ok(static_walkable) = ensure_static_walkable() else {
            return;
        };
        let mut fallback = Vec::with_capacity(8);
        for (dx, dy) in [(2, 0), (-2, 0), (0, 2), (0, -2), (2, 2), (-2, -2), (2, -2), (-2, 2)] {
            let tile = Vec2::new(my_pos.x + dx, my_pos.y + dy);
            if in_bounds_fast(tile) && static_walkable.is_set(tile).unwrap_or(false) {
                fallback.push(tile);
            }
        }
        for tile in oriented(fallback).into_iter().take(4) {
            if me.place_ward(tile).unwrap_or(false) {
                break;
            }
        }
    }
}

fn dash_slot_index(me: &Champion) -> Option<usize> {
    me.abilities().iter().position(|a| a.is_dash())
}

fn dash_ready(me: &Champion) -> Option<usize> {
    let idx = dash_slot_index(me)?;
    if me.abilities()[idx].is_ready().unwrap_or(false) {
        Some(idx)
    } else {
        None
    }
}

fn cast_dash_to_first_valid(me: &Champion, idx: usize, ranked: &[Vec2]) -> bool {
    for &tile in ranked.iter().take(6) {
        if me.abilities()[idx].cast_at(tile).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn dash_retreat(
    me: &Champion,
    origin: Vec2,
    goals: &Bitboard,
    blocked: &Bitboard,
    enemies: &[EnemyInfo],
) -> Result<bool> {
    if !has_fuel(MIN_FUEL_FOR_SEARCH) {
        return Ok(false);
    }
    let Some(idx) = dash_ready(me) else { return Ok(false) };
    let candidates = legal_dash_tiles(origin, blocked)?;
    if candidates.is_empty() {
        return Ok(false);
    }

    let bfs_obstacles = blocked.intersect(&goals.complement()?)?;
    BFS_FIELD.with(|f_cell| {
        if f_cell.borrow().is_none() {
            *f_cell.borrow_mut() = DistanceField::new().ok();
        }
    });
    BFS_VISITED.with(|v_cell| {
        if v_cell.borrow().is_none() {
            *v_cell.borrow_mut() = Bitboard::new().ok();
        }
    });
    BFS_DEST.with(|d_cell| {
        if d_cell.borrow().is_none() {
            *d_cell.borrow_mut() = Bitboard::new().ok();
        }
    });

    BFS_GOALS.with(|g| *g.borrow_mut() = None);
    let compute_ok = BFS_FIELD.with(|f_cell| {
        let mut f_borrow = f_cell.borrow_mut();
        let Some(ref mut field) = *f_borrow else {
            return Err(cambc::Error::MissingIdentity);
        };
        BFS_VISITED.with(|v_cell| {
            let mut v_borrow = v_cell.borrow_mut();
            let Some(ref mut visited) = *v_borrow else {
                return Err(cambc::Error::MissingIdentity);
            };
            BFS_DEST.with(|d_cell| {
                let mut d_borrow = d_cell.borrow_mut();
                let Some(ref mut destinations) = *d_borrow else {
                    return Err(cambc::Error::MissingIdentity);
                };
                destinations.clear();
                destinations.set(origin, true)?;
                field.compute_to(
                    &bfs_obstacles,
                    goals,
                    destinations,
                    visited,
                    UNREACHABLE_DIST,
                )
            })
        })
    });
    if compute_ok.is_err() {
        return Ok(false);
    }

    let here = BFS_FIELD.with(|f_cell| {
        f_cell
            .borrow()
            .as_ref()
            .and_then(|f| f.get(origin).ok())
            .unwrap_or(UNREACHABLE_DIST)
    });
    if here >= UNREACHABLE_DIST {
        return Ok(false);
    }
    let is_combat = is_combat_dash(me, &me.abilities()[idx]);
    let mut ranked: Vec<(u16, i32, Vec2)> = Vec::new();
    BFS_FIELD.with(|f_cell| {
        if let Some(ref field) = *f_cell.borrow() {
            for tile in candidates {
                let Ok(d) = field.get(tile) else { continue };
                if d >= UNREACHABLE_DIST || d.saturating_add(2) > here {
                    continue;
                }
                let nearest_enemy = enemies
                    .iter()
                    .map(|e| distance(tile, e.pos))
                    .min()
                    .unwrap_or(99);
                let stun_peel_bonus = if is_combat
                    && enemies
                    .iter()
                    .any(|e| distance(tile, e.pos) <= 1 && !e.stunned)
                {
                    2
                } else {
                    0
                };
                ranked.push((d, -(nearest_enemy + stun_peel_bonus), tile));
            }
        }
    });
    ranked.sort_by_key(|r| (r.0, r.1));
    let order: Vec<Vec2> = ranked.iter().map(|r| r.2).collect();

    if !cast_dash_to_first_valid(me, idx, &order) {
        return Ok(false);
    }

    if let Ok(new_pos) = me.position() {
        if !goals.is_set(new_pos).unwrap_or(false) && me.can_move().unwrap_or(false) {
            let mut best_step: Option<(u16, Vec2)> = None;
            BFS_FIELD.with(|f_cell| {
                if let Some(ref field) = *f_cell.borrow() {
                    let cur_d = field.get(new_pos).unwrap_or(UNREACHABLE_DIST);
                    for tile in legal_step_neighbors(new_pos, blocked) {
                        if let Ok(d) = field.get(tile) {
                            if d < cur_d && best_step.as_ref().map_or(true, |(bd, _)| d < *bd) {
                                best_step = Some((d, tile));
                            }
                        }
                    }
                }
            });
            if let Some((_, step_tile)) = best_step {
                let _ = me.move_absolute(step_tile);
            }
        }
    }
    Ok(true)
}

fn dash_engage(
    me: &Champion,
    my_pos: Vec2,
    enemies: &[EnemyInfo],
    is_varg: bool,
) -> Result<bool> {
    let atk_range = me.attack_range().unwrap_or(1);
    let Some(idx) = dash_ready(me) else { return Ok(false) };
    let is_combat = is_combat_dash(me, &me.abilities()[idx]);

    if is_combat {
        if try_cast_combat_dash_on_enemies(me, my_pos, enemies)? {
            if let Ok(pos_now) = me.position() {
                if !enemies.iter().any(|e| distance(pos_now, e.pos) <= 1)
                    && me.can_move().unwrap_or(false)
                {
                    if let Ok(mut ring) = tiles_around(enemies[0].pos, 1) {
                        let _ = ring.set(enemies[0].pos, false);
                        let _ = move_towards_closest(ring);
                    }
                }
                let final_pos = me.position().unwrap_or(pos_now);
                let _ = try_combat_actions(me, final_pos, enemies, is_varg);
            }
            return Ok(true);
        }

        if let Some(target) = enemies.iter().find(|e| (3..=4).contains(&e.dist)) {
            if me.can_move().unwrap_or(false) && !enemies.iter().any(|e| e.dist <= 2) {
                let step_goals = tiles_around(target.pos, 3)?;
                if move_towards_closest(step_goals)? {
                    let pos_after_walk = me.position().unwrap_or(my_pos);
                    let _ = try_cast_combat_dash_on_enemies(me, pos_after_walk, enemies);
                    let final_pos = me.position().unwrap_or(pos_after_walk);
                    let _ = try_combat_actions(me, final_pos, enemies, is_varg);
                    return Ok(true);
                }
            }
        }
    }

    if enemies.iter().any(|e| e.dist <= atk_range) {
        return Ok(false);
    }
    let max_engage_dist = if atk_range > 1 { 5 } else { 4 };
    let Some(target) = enemies
        .iter()
        .find(|e| e.dist > atk_range && e.dist <= max_engage_dist)
    else {
        return Ok(false);
    };

    let blocked = blocked_tiles(my_pos)?;
    let candidates = legal_dash_tiles(my_pos, &blocked)?;

    let mut ranked: Vec<(i32, Vec2)> = Vec::new();
    for tile in candidates {
        let d = distance(tile, target.pos);
        if d >= 1 && d <= atk_range + 1 && d < target.dist {
            ranked.push(((d - atk_range).abs(), tile));
        }
    }
    ranked.sort_by_key(|r| r.0);
    let order: Vec<Vec2> = ranked.iter().map(|r| r.1).collect();

    if !cast_dash_to_first_valid(me, idx, &order) {
        return Ok(false);
    }

    if let Ok(new_pos) = me.position() {
        if distance(new_pos, target.pos) > atk_range && me.can_move().unwrap_or(false) {
            if let Ok(mut ring) = tiles_around(target.pos, atk_range) {
                let _ = ring.set(target.pos, false);
                let _ = move_towards_closest(ring);
            }
        }
    }

    if let Ok(pos_now) = me.position() {
        let _ = try_combat_actions(me, pos_now, enemies, is_varg);
    }
    Ok(true)
}

const EXPLORE_REFRESH_ROUNDS: u32 = 150;

#[derive(Default)]
pub struct Explorer {
    seen: Option<Bitboard>,
    walkable: Option<Bitboard>,
    last_reset: u32,
}

impl Explorer {
    pub fn observe_with(&mut self, visible: &Bitboard) -> Result<()> {
        if let Some(seen) = self.seen.as_mut() {
            seen.union_assign(visible)?;
        } else {
            self.seen = Some(visible.clone());
        }
        Ok(())
    }

    fn ensure_walkable(&mut self, origin: Vec2) -> Result<()> {
        if self.walkable.is_some() {
            return Ok(());
        }
        let mut blocked = ensure_static_blocked()?;
        blocked.set(origin, false)?;

        let mut field = DistanceField::new()?;
        let mut visited = Bitboard::new()?;
        let roots = tiles_around(origin, 0)?;
        if field
            .compute(&blocked, &roots, &mut visited, UNREACHABLE_DIST)
            .is_ok()
        {
            self.walkable = Some(visited);
        } else {
            self.walkable = Some(ensure_static_walkable()?);
        }
        Ok(())
    }

    pub fn explore(&mut self, me: &Champion, origin: Vec2) -> Result<bool> {
        if !me.can_move()? || !has_fuel(32_000) {
            return Ok(false);
        }
        self.ensure_walkable(origin)?;
        if self.seen.is_none() {
            let visible = cambc::vision::visible_tiles()?;
            self.observe_with(&visible)?;
        }
        let round = cambc::round_number()? as u32;

        let walkable = self.walkable.as_ref().expect("set above");
        let mut goals =
            walkable.intersect(&self.seen.as_ref().expect("set above").complement()?)?;

        if goals.is_empty()? || round.saturating_sub(self.last_reset) >= EXPLORE_REFRESH_ROUNDS {
            self.seen = Some(cambc::vision::visible_tiles()?);
            self.last_reset = round;
            goals = walkable.intersect(&self.seen.as_ref().expect("just set").complement()?)?;
        }
        if goals.is_empty()? {
            return Ok(false);
        }
        move_towards_closest(goals)
    }
}

fn read_allied_camp_claims(my_slot: usize) -> [u8; SHARED_CAMP_SLOTS] {
    let mut claims = [0u8; SHARED_CAMP_SLOTS];
    let shm = cambc::shared_memory();
    let _ = shm.read(0, &mut claims);
    if my_slot < SHARED_CAMP_SLOTS {
        claims[my_slot] = 0;
    }
    claims
}

fn publish_camp_claim(my_slot: usize, group_idx: Option<usize>) {
    if my_slot >= SHARED_CAMP_SLOTS {
        return;
    }
    let val = group_idx
        .and_then(|idx| u8::try_from(idx.saturating_add(1)).ok())
        .unwrap_or(0);
    let _ = cambc::shared_memory().set(my_slot, val);
}

const GEM_WEIGHT: f32 = 8.5;
const GOLD_WEIGHT_START: f32 = 12.5;
const GOLD_WEIGHT_MIN: f32 = 2.2;
const GOLD_ROUND_HALF_LIFE: f32 = 240.0;
const GOLD_SHARD_HALF_LIFE: f32 = 28.0;
const POOR_GOLD_BOOST: f32 = 1.65;
const POOR_GOLD: u32 = 120;

const DIST_OFFSET: f32 = 3.0;
const SWITCH_RATIO: f32 = 1.5;

const SIDE_RADIUS: i32 = 10;
const MAX_DETOUR: i32 = 8;
const SIDE_TIMEOUT: u32 = 40;

const UNREADY_FACTOR: f32 = 0.35;
const ALLY_CLAIMED_FACTOR: f32 = 0.25;
const ELK_MIN_SHARDS: u32 = 0;
const GOLEM_MIN_SHARDS: u32 = 6;
const MONOLITH_MIN_SHARDS: u32 = 35;

const STALL_ROUNDS: u32 = 80;
const TARGET_TIMEOUT: u32 = 100;
const BLACKLIST_ROUNDS: u32 = 60;
const MAX_TARGET_FAILS: u32 = 5;

fn decayed_gold_weight(round: u32, shards: u32, poor: bool) -> f32 {
    let by_round = 0.5f32.powf(round as f32 / GOLD_ROUND_HALF_LIFE);
    let by_shards = 0.5f32.powf(shards as f32 / GOLD_SHARD_HALF_LIFE);
    let w = (GOLD_WEIGHT_START * by_round * by_shards).max(GOLD_WEIGHT_MIN);
    if poor { w * POOR_GOLD_BOOST } else { w }
}

struct GroupInfo {
    group: cambc::CampGroup,
    positions: Vec<Vec2>,
    bbox: (i32, i32, i32, i32),
    gems: i32,
    gold: i32,
    monolith: bool,
    wolf_count: usize,
    min_shards: u32,
    max_respawn: u32,
    total_hp: f32,
    total_strength: f32,
}

#[derive(Default)]
pub struct CampPlanner {
    groups: Vec<GroupInfo>,
    loaded: bool,
    respawning_since: Vec<Option<u32>>,
    skip_until: Vec<u32>,
    state_round: u32,
    state_cache: Vec<Option<cambc::CampGroupState>>,
    monolith_group_tiles: Option<Bitboard>,
    golem_group_tiles: Option<Bitboard>,
    target: Option<(usize, u32)>,
    side: Option<(usize, u32)>,
    target_fails: u32,
    row: Vec<usize>,
    dps: f32,
    hp: f32,
    last_gems: u32,
    last_gain_round: u32,
    gem_init: bool,
    near_origin: Option<Vec2>,
    near: Vec<i32>,
    tile_group: Vec<u16>,
    pub altar_positions: Vec<Vec2>,
    pub ally_spawn_positions: Vec<Vec2>,
    gem_clusters: u32,
    heavy_camps: u32,
    claim: f32,
}

impl CampPlanner {
    fn load(&mut self) -> Result<()> {
        if self.loaded {
            return Ok(());
        }
        self.altar_positions = cambc::map::altars()
            .and_then(|a| a.positions())
            .unwrap_or_default();
        self.ally_spawn_positions = cambc::map::spawn_region(Team::Ally)
            .and_then(|s| s.positions())
            .unwrap_or_default();

        self.claim = ALLY_CLAIMED_FACTOR;
        let mut mono_mask = Bitboard::new().ok();
        let mut golem_mask = Bitboard::new().ok();

        for group in cambc::map::camp_groups()? {
            let mut positions: Vec<Vec2> = Vec::with_capacity(group.camps().len());
            let mut monolith = false;
            let mut golem = false;
            let mut elk = false;
            let mut wolf_count = 0usize;
            let mut total_hp = 0.0f32;
            let mut total_strength = 0.0f32;

            for c in group.iter() {
                let sp = c.spawn_position();
                positions.push(sp);
                let kind = c.kind();
                match kind {
                    CampType::Monolith => monolith = true,
                    CampType::Golem => golem = true,
                    CampType::Elk => elk = true,
                    CampType::Wolf => wolf_count += 1,
                    _ => {}
                }
                match kind {
                    CampType::GemCluster => self.gem_clusters += 1,
                    CampType::Elk => self.heavy_camps += 1,
                    CampType::Golem => self.heavy_camps += 1,
                    _ => {}
                }
                total_hp += camp_design_health(kind);
                total_strength += camp_design_strength(kind);
            }
            if positions.is_empty() {
                if let Ok(tiles) = group.spawn_tiles() {
                    positions = tiles.positions().unwrap_or_default();
                }
            }
            if monolith {
                if let Some(ref mut m) = mono_mask {
                    for &p in &positions {
                        let _ = m.set(p, true);
                    }
                }
            } else if golem {
                if let Some(ref mut m) = golem_mask {
                    for &p in &positions {
                        let _ = m.set(p, true);
                    }
                }
            }
            let min_shards = if monolith {
                MONOLITH_MIN_SHARDS
            } else if golem {
                GOLEM_MIN_SHARDS
            } else if elk {
                ELK_MIN_SHARDS
            } else {
                0
            };
            let bbox = positions.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, p| {
                (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y))
            });
            self.groups.push(GroupInfo {
                bbox,
                gems: group.gem_value() as i32,
                gold: group.gold_value() as i32,
                monolith,
                wolf_count,
                min_shards,
                max_respawn: group.max_respawn_timer().max(0) as u32,
                total_hp,
                total_strength,
                positions,
                group,
            });
        }

        self.monolith_group_tiles = mono_mask;
        self.golem_group_tiles = golem_mask;

        let n = self.groups.len();
        let (w, h) = map_dims();
        self.tile_group = vec![0; (w * h).max(0) as usize];
        for (i, g) in self.groups.iter().enumerate() {
            for p in &g.positions {
                if in_bounds_fast(*p) {
                    self.tile_group[(p.y * w + p.x) as usize] = i as u16 + 1;
                }
            }
        }
        self.respawning_since = vec![None; n];
        self.skip_until = vec![0; n];
        self.state_cache = vec![None; n];

        let three_wolf_groups = self
            .groups
            .iter()
            .filter(|g| g.wolf_count == 3 && !g.monolith)
            .count();
        if w == 50
            && h == 50
            && three_wolf_groups == 6
            && self.groups.iter().any(|g| g.monolith)
        {
            let mut wolves: Vec<(i32, usize)> = self
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.wolf_count == 3 && !g.monolith)
                .map(|(i, g)| {
                    let d = g
                        .positions
                        .iter()
                        .take(4)
                        .flat_map(|p| {
                            self.ally_spawn_positions
                                .iter()
                                .take(6)
                                .map(move |s| distance(*p, *s))
                        })
                        .min()
                        .unwrap_or(i32::MAX);
                    (d, i)
                })
                .collect();
            wolves.sort();
            self.row = wolves.into_iter().take(3).map(|(_, i)| i).collect();
        }
        self.loaded = true;
        Ok(())
    }

    #[inline]
    fn group_dist(&self, i: usize, j: usize) -> i32 {
        let ai = self.groups[i]
            .positions
            .first()
            .copied()
            .unwrap_or(Vec2::new(0, 0));
        let bj = self.groups[j]
            .positions
            .first()
            .copied()
            .unwrap_or(Vec2::new(0, 0));
        distance(ai, bj)
    }

    fn group_tiles_bitboard(&self, idx: usize) -> Result<Bitboard> {
        let mut b = Bitboard::new()?;
        for &p in &self.groups[idx].positions {
            let _ = b.set(p, true);
        }
        Ok(b)
    }

    pub fn note_gems(&mut self, round: u32, gems: u32) {
        if !self.gem_init {
            self.gem_init = true;
            self.last_gems = gems;
            self.last_gain_round = round;
            return;
        }
        if gems > self.last_gems {
            self.last_gain_round = round;
        }
        self.last_gems = gems;
    }

    pub fn reset_clock(&mut self, round: u32) {
        self.gem_init = true;
        self.last_gems = 0;
        self.last_gain_round = round;
        self.target = None;
        self.side = None;
    }

    pub fn starving(&self, round: u32) -> bool {
        round.saturating_sub(self.last_gain_round) >= STALL_ROUNDS
    }

    fn group_state_cached(
        &mut self,
        i: usize,
        round: u32,
        visible: &Bitboard,
        camp_tiles: &Bitboard,
    ) -> Option<cambc::CampGroupState> {
        if round <= 1 {
            return Some(cambc::CampGroupState::Alive);
        }
        if self.state_round != round {
            self.state_cache.fill(None);
            self.state_round = round;
        }
        if let Some(st) = self.state_cache[i] {
            return Some(st);
        }

        let mut any_visible = false;
        for &p in &self.groups[i].positions {
            if camp_tiles.is_set(p).unwrap_or(false) {
                self.respawning_since[i] = None;
                let st = cambc::CampGroupState::Alive;
                self.state_cache[i] = Some(st);
                return Some(st);
            }
            if !any_visible && visible.is_set(p).unwrap_or(false) {
                any_visible = true;
            }
        }
        if !any_visible {
            return None;
        }

        if self.is_respawning(i, round) {
            let st = cambc::CampGroupState::Respawning(1);
            self.state_cache[i] = Some(st);
            return Some(st);
        }

        let st = self.groups[i].group.state().ok()?;
        match st {
            cambc::CampGroupState::Respawning(rem) => {
                let rem_u32 = rem.max(1) as u32;
                let elapsed = self.groups[i].max_respawn.saturating_sub(rem_u32);
                self.respawning_since[i] = Some(round.saturating_sub(elapsed));
            }
            cambc::CampGroupState::RewardAvailable | cambc::CampGroupState::Alive => {
                self.respawning_since[i] = None;
            }
        }
        self.state_cache[i] = Some(st);
        Some(st)
    }

    fn observe(
        &mut self,
        round: u32,
        origin: Vec2,
        visible: &Bitboard,
        camp_tiles: &Bitboard,
        allies: &[Champion],
    ) -> Result<()> {
        if round <= 1 {
            return Ok(());
        }
        self.aim(origin);
        let allied_positions: Vec<Vec2> = allies
            .iter()
            .filter_map(|a| {
                let p = a.position().ok()?;
                (p != origin && a.health().unwrap_or(0.0) > 0.0).then_some(p)
            })
            .collect();

        for i in 0..self.groups.len() {
            if self.near[i] > 8 {
                continue;
            }
            if let Some(cambc::CampGroupState::RewardAvailable) =
                self.group_state_cached(i, round, visible, camp_tiles)
            {
                if allied_positions
                    .iter()
                    .any(|ap| self.contains(i, *ap))
                {
                    let respawn_pad = self.groups[i].max_respawn.min(55).max(20);
                    let skip_turns = if PATCH_ADAPT_CAPTURE_STATE {
                        self.groups[i]
                            .group
                            .capture_state()
                            .ok()
                            .and_then(|cs| {
                                (cs.state == CampGroupCaptureType::AllyCapturing && cs.timer > 0)
                                    .then_some((cs.timer as u32).saturating_add(respawn_pad))
                            })
                            .unwrap_or(respawn_pad)
                    } else {
                        respawn_pad
                    };
                    self.skip_until[i] = self.skip_until[i].max(round + skip_turns);
                }
            }
        }
        Ok(())
    }

    fn is_respawning(&self, i: usize, round: u32) -> bool {
        match self.respawning_since[i] {
            Some(s) => round.saturating_sub(s) < self.groups[i].max_respawn,
            None => false,
        }
    }

    fn aim(&mut self, origin: Vec2) {
        if self.near_origin != Some(origin) {
            self.near = (0..self.groups.len()).map(|i| self.measure(origin, i)).collect();
            self.near_origin = Some(origin);
        }
    }

    fn measure(&self, origin: Vec2, i: usize) -> i32 {
        let (x0, y0, x1, y1) = self.groups[i].bbox;
        if x0 > x1 {
            return 99;
        }
        let dx = (x0 - origin.x).max(origin.x - x1).max(0);
        let dy = (y0 - origin.y).max(origin.y - y1).max(0);
        dx.max(dy)
    }

    fn group_at(&self, p: Vec2) -> Option<usize> {
        if !in_bounds_fast(p) {
            return None;
        }
        let w = map_dims().0;
        match self.tile_group.get((p.y * w + p.x) as usize) {
            Some(&g) if g > 0 => Some(g as usize - 1),
            _ => None,
        }
    }

    fn contains(&self, i: usize, p: Vec2) -> bool {
        self.group_at(p) == Some(i)
    }

    fn dist_to(&self, origin: Vec2, i: usize) -> i32 {
        if self.near_origin == Some(origin) {
            self.near[i]
        } else {
            self.measure(origin, i)
        }
    }

    fn unavailable(&self, i: usize, round: u32) -> bool {
        self.skip_until[i] > round || self.is_respawning(i, round)
    }

    fn is_claimed_by_ally(i: usize, ally_claims: &[u8; SHARED_CAMP_SLOTS]) -> bool {
        let Ok(code) = u8::try_from(i.saturating_add(1)) else {
            return false;
        };
        ally_claims.iter().any(|&c| c == code)
    }

    fn score_of(
        &self,
        i: usize,
        origin: Vec2,
        shards: u32,
        gold_weight: f32,
        ally_claims: &[u8; SHARED_CAMP_SLOTS],
    ) -> f32 {
        let g = &self.groups[i];
        let mut value = g.gems as f32 * GEM_WEIGHT + g.gold as f32 * gold_weight;

        if shards < g.min_shards {
            value *= UNREADY_FACTOR;
        }
        let solo = g.total_hp / self.dps.max(1.0) < 0.85 * self.hp / g.total_strength.max(1.0);
        let pack_capable =
            g.total_hp / (self.dps * 5.0).max(1.0) < 1.2 * (self.hp * 4.0) / g.total_strength.max(1.0);
        let multi_or_tough = g.total_hp >= 2400.0 || g.total_strength >= 40.0;

        if Self::is_claimed_by_ally(i, ally_claims) {
            value *= if crate::profile::has(crate::profile::PACK) {
                3.0
            } else if !solo {
                2.8
            } else if multi_or_tough && self.dist_to(origin, i) <= 10 {
                1.65
            } else {
                self.claim
            };
        } else if !solo {
            value *= if pack_capable { 0.85 } else { 0.18 };
        }
        let kill_turns = 1.0 * g.total_hp / self.dps.max(1.0);
        let d = self.dist_to(origin, i) as f32 + kill_turns + 5.0;
        let x = d + DIST_OFFSET;
        value / (x * x.sqrt())
    }

    fn best_scored(
        &self,
        origin: Vec2,
        round: u32,
        shards: u32,
        gold_weight: f32,
        allow_gold_only: bool,
        ally_claims: &[u8; SHARED_CAMP_SLOTS],
    ) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        for (i, g) in self.groups.iter().enumerate() {
            if self.unavailable(i, round) {
                continue;
            }
            if g.monolith && shards < g.min_shards {
                continue;
            }
            if !allow_gold_only && g.gems == 0 {
                continue;
            }
            if let Some((b, _)) = best {
                let top = (g.gems as f32 * GEM_WEIGHT + g.gold as f32 * gold_weight) * 3.0;
                let x = self.dist_to(origin, i) as f32 + 5.0 + DIST_OFFSET;
                if top * top <= b * b * x * x * x {
                    continue;
                }
            }
            let s = self.score_of(i, origin, shards, gold_weight, ally_claims);
            if best.map_or(true, |(b, _)| s > b) {
                best = Some((s, i));
            }
        }
        best
    }

    fn pick_main(
        &mut self,
        origin: Vec2,
        round: u32,
        shards: u32,
        gold: u32,
        ally_claims: &[u8; SHARED_CAMP_SLOTS],
    ) -> Option<usize> {
        let poor = gold < POOR_GOLD;
        let gw = decayed_gold_weight(round, shards, poor);
        let starving = self.starving(round);
        let allow_gold_only = !starving || poor;

        if round < 120 {
            let mut best: Option<(f32, usize)> = None;
            for (i, g) in self.groups.iter().enumerate() {
                if self.unavailable(i, round) || g.monolith || Self::is_claimed_by_ally(i, ally_claims) {
                    continue;
                }
                let kill = g.total_hp / self.dps.max(1.0);
                if g.total_strength * kill > 0.6 * self.hp {
                    continue;
                }
                let value = g.gems as f32 * GEM_WEIGHT + g.gold as f32 * gw;
                let rate = value / (self.dist_to(origin, i) as f32 + kill + 5.0);
                if best.map_or(true, |(b, _)| rate > b) {
                    best = Some((rate, i));
                }
            }
            if let Some((_, i)) = best {
                self.target = Some((i, round));
                return Some(i);
            }
        }

        if let Some((i, since)) = self.target {
            let timed_out = round.saturating_sub(since) > TARGET_TIMEOUT;
            if timed_out {
                self.skip_until[i] = round + BLACKLIST_ROUNDS;
            }
            let (monolith, min_shards, gems) = {
                let g = &self.groups[i];
                (g.monolith, g.min_shards, g.gems)
            };
            let bad = timed_out
                || self.unavailable(i, round)
                || (monolith && shards < min_shards)
                || (!allow_gold_only && gems == 0);
            if !bad && round % 4 != 0 {
                return Some(i);
            }
            if !bad {
                if let Some((s, j)) =
                    self.best_scored(origin, round, shards, gw, allow_gold_only, ally_claims)
                {
                    if j != i
                        && s > self.score_of(i, origin, shards, gw, ally_claims) * SWITCH_RATIO
                    {
                        self.target = Some((j, round));
                        self.target_fails = 0;
                        return Some(j);
                    }
                }
                return Some(i);
            }
            self.target = None;
            self.target_fails = 0;
        }

        let choice = self
            .best_scored(origin, round, shards, gw, allow_gold_only, ally_claims)
            .or_else(|| self.best_scored(origin, round, shards, gw, true, ally_claims))
            .map(|(_, i)| i);
        if let Some(i) = choice {
            self.target = Some((i, round));
            self.target_fails = 0;
        }
        choice
    }

    fn find_side(
        &self,
        origin: Vec2,
        round: u32,
        shards: u32,
        main: usize,
        ally_claims: &[u8; SHARED_CAMP_SLOTS],
    ) -> Option<usize> {
        let dm = self.dist_to(origin, main);
        let mut best: Option<(i32, i32, usize)> = None;
        for (i, g) in self.groups.iter().enumerate() {
            if i == main || self.unavailable(i, round) || Self::is_claimed_by_ally(i, ally_claims) {
                continue;
            }
            if shards < g.min_shards || g.gems == 0 {
                continue;
            }
            let d = self.dist_to(origin, i);
            if d > SIDE_RADIUS {
                continue;
            }
            let detour = d + self.group_dist(i, main) - dm;

            if detour > MAX_DETOUR {
                continue;
            }
            let better = match best {
                None => true,
                Some((bd, bdist, _)) => detour < bd || (detour == bd && d < bdist),
            };
            if better {
                best = Some((detour, d, i));
            }
        }
        best.map(|(_, _, i)| i)
    }

    fn row_cleared(&self, round: u32) -> bool {
        !self.row.is_empty() && self.row.iter().all(|&i| self.unavailable(i, round))
    }

    fn pick(
        &mut self,
        origin: Vec2,
        round: u32,
        shards: u32,
        gold: u32,
        ally_claims: &[u8; SHARED_CAMP_SLOTS],
    ) -> Option<usize> {
        if !self.row.is_empty() {
            let next = self
                .row
                .iter()
                .copied()
                .filter(|&i| !self.unavailable(i, round))
                .min_by_key(|&i| self.dist_to(origin, i));
            if let Some(i) = next {
                if self.target.map(|t| t.0) != Some(i) {
                    self.target = Some((i, round));
                }
                self.side = None;
                return Some(i);
            }

            if shards >= MONOLITH_MIN_SHARDS {
                if let Some((i, _)) = self
                    .groups
                    .iter()
                    .enumerate()
                    .find(|&(i, g)| g.monolith && !self.unavailable(i, round))
                {
                    self.target = Some((i, round));
                    self.side = None;
                    return Some(i);
                }
            }
            if let Some(soonest) = self
                .row
                .iter()
                .copied()
                .min_by_key(|&i| self.respawning_since[i].unwrap_or(0))
            {
                self.target = Some((soonest, round));
                self.side = None;
                return Some(soonest);
            }
        }

        let main = self.pick_main(origin, round, shards, gold, ally_claims)?;

        if let Some((s, since)) = self.side {
            let timed_out = round.saturating_sub(since) > SIDE_TIMEOUT;
            if timed_out {
                self.skip_until[s] = round + BLACKLIST_ROUNDS;
            }
            let bad = s == main
                || timed_out
                || self.unavailable(s, round)
                || shards < self.groups[s].min_shards;
            if !bad {
                return Some(s);
            }
            self.side = None;
            self.target_fails = 0;
        }

        if round % 2 != 0 {
            return Some(main);
        }
        if let Some(s) = self.find_side(origin, round, shards, main, ally_claims) {
            self.side = Some((s, round));
            self.target_fails = 0;
            return Some(s);
        }
        Some(main)
    }

    fn note_move(&mut self, idx: usize, moved: bool, round: u32) {
        if moved {
            self.target_fails = 0;
            return;
        }
        self.target_fails += 1;
        if self.target_fails >= MAX_TARGET_FAILS {
            self.skip_until[idx] = round + BLACKLIST_ROUNDS;
            if self.target.map(|t| t.0) == Some(idx) {
                self.target = None;
            }
            if self.side.map(|s| s.0) == Some(idx) {
                self.side = None;
            }
            self.target_fails = 0;
        }
    }
}

fn reward_waiting(
    planner: &mut CampPlanner,
    my_pos: Vec2,
    round: u32,
    is_vesper: bool,
    visible: &Bitboard,
    camp_tiles: &Bitboard,
    allies: &[Champion],
) -> Result<Option<(usize, Bitboard)>> {
    if round <= 1 {
        return Ok(None);
    }
    let mut other_allies: Vec<(Vec2, bool)> = Vec::new();
    for ally in allies {
        let Ok(p) = ally.position() else { continue };
        if p == my_pos || ally.health().unwrap_or(0.0) <= 0.0 {
            continue;
        }
        let ally_is_vesper = matches!(ally.champion_name(), Ok(ChampionType::VesperVale));
        other_allies.push((p, ally_is_vesper));
    }

    let mut best: Option<(i32, usize)> = None;
    for i in 0..planner.groups.len() {
        let d = planner.dist_to(my_pos, i);
        if d > 5 {
            continue;
        }
        if !matches!(
            planner.group_state_cached(i, round, visible, camp_tiles),
            Some(cambc::CampGroupState::RewardAvailable)
        ) {
            continue;
        }

        if PATCH_ADAPT_CAPTURE_STATE && d > 1 {
            if let Ok(cs) = planner.groups[i].group.capture_state() {
                if cs.state == CampGroupCaptureType::EnemyCapturing && cs.timer < d {
                    continue;
                }
            }
        }

        if !is_vesper
            && other_allies
            .iter()
            .any(|&(p, _)| planner.contains(i, p))
        {
            continue;
        }

        let on_tile_now = planner.contains(i, my_pos);
        if !is_vesper
            && !on_tile_now
            && other_allies.iter().any(|&(p, ally_vesper)| {
            let ad = planner.dist_to(p, i);
            ad < d || (ad == d && ally_vesper)
        })
        {
            continue;
        }

        let eff_d = if PATCH_ADAPT_CAPTURE_STATE && on_tile_now {
            -10
        } else {
            d
        };
        if best.as_ref().map_or(true, |(bd, _)| eff_d < *bd) {
            best = Some((eff_d, i));
        }
    }
    if let Some((_, i)) = best {
        Ok(Some((i, planner.group_tiles_bitboard(i)?)))
    } else {
        Ok(None)
    }
}

fn farm_camps(
    me: &Champion,
    planner: &mut CampPlanner,
    shards: u32,
    completed_passives: u16,
    my_slot: usize,
    visible: &Bitboard,
    camp_tiles: &Bitboard,
    allies: &[Champion],
    near_camps: &[NearCamp],
) -> Result<bool> {
    planner.load()?;

    let is_heart = is_heart_of_ruin(me);
    let mut effective_shards = shards;
    if (completed_passives & PASSIVE_MONSTER) != 0 {
        effective_shards = effective_shards.saturating_add(5);
    }
    if (completed_passives & PASSIVE_JUNGLE) != 0 {
        effective_shards = effective_shards.saturating_add(10);
    }
    if is_heart {
        effective_shards = effective_shards.saturating_add(3);
    }

    let here = me.position()?;
    let ally_near = allies
        .iter()
        .any(|a| a.position().is_ok_and(|p| p != here && distance(p, here) <= 5));
    let safe_camps: Vec<&NearCamp> = near_camps
        .iter()
        .filter(|c| {
            if !ally_near && !c.awake && planner.dps > 0.0 {
                if let Some(gi) = planner.group_at(c.pos) {
                    let g = &planner.groups[gi];
                    if g.total_strength > 0.0
                        && g.total_hp / planner.dps >= 0.85 * planner.hp / g.total_strength
                    {
                        return false;
                    }
                }
            }
            if effective_shards < MONOLITH_MIN_SHARDS {
                if planner
                    .monolith_group_tiles
                    .as_ref()
                    .and_then(|m| m.is_set(c.pos).ok())
                    .unwrap_or(false)
                {
                    return false;
                }
            }
            if effective_shards < GOLEM_MIN_SHARDS {
                if planner
                    .golem_group_tiles
                    .as_ref()
                    .and_then(|m| m.is_set(c.pos).ok())
                    .unwrap_or(false)
                {
                    return false;
                }
            }
            true
        })
        .collect();

    if try_attack_camps_in_range(me, &safe_camps) {
        return Ok(true);
    }
    if !me.can_move()? && dash_ready(me).is_none() {
        return Ok(false);
    }

    let round = cambc::round_number()? as u32;
    let origin = me.position()?;
    let gold = me.gold_count().unwrap_or(0);
    planner.observe(round, origin, visible, camp_tiles, allies)?;

    let camp_dmg_mult = if (completed_passives & PASSIVE_MONSTER) != 0 {
        if PATCH_ADAPT_MONSTER_115 { 1.15 } else { 1.10 }
    } else {
        1.0
    };
    let mut ehp_mult = 1.0f32;
    if (completed_passives & PASSIVE_JUNGLE) != 0 {
        ehp_mult /= 0.70;
    }
    if (completed_passives & PASSIVE_DURABILITY) != 0 {
        ehp_mult /= 0.90;
    }
    planner.dps = rough_dps(me) * camp_dmg_mult;
    let bonus_shield_hp = if is_heart {
        1.5 * me.focus().unwrap_or(100.0)
    } else if matches!(me.champion_name(), Ok(ChampionType::Varg)) {
        2.5 * me.focus().unwrap_or(100.0)
    } else {
        0.0
    };
    planner.hp = (me.max_health()? + bonus_shield_hp) * ehp_mult;

    let mut ally_claims = read_allied_camp_claims(my_slot);
    for ally in allies {
        let Ok(apos) = ally.position() else { continue };
        if apos == origin {
            continue;
        }
        if let Some(gi) = planner.group_at(apos) {
            {
                if let Ok(code) = u8::try_from(gi.saturating_add(1)) {
                    if ally_claims[SHARED_CAMP_SLOTS - 1] == 0 {
                        ally_claims[SHARED_CAMP_SLOTS - 1] = code;
                    }
                }
            }
        }
    }

    let Some(idx) = planner.pick(origin, round, effective_shards, gold, &ally_claims) else {
        publish_camp_claim(my_slot, None);
        return Ok(false);
    };
    publish_camp_claim(my_slot, Some(idx));

    let mut living_group_monsters: Vec<Vec2> = Vec::new();
    for camp in &safe_camps {
        if planner.contains(idx, camp.pos)
            || planner.dist_to(camp.pos, idx) <= 2
        {
            living_group_monsters.push(camp.pos);
        }
    }

    if !living_group_monsters.is_empty() {
        let atk_range = me.attack_range().unwrap_or(1).max(1);
        let mut attack_goals = Bitboard::new()?;
        if PATCH_ADAPT_FAST_ATTACK_GOALS {
            let r_f32 = atk_range as f32;
            for &mpos in &living_group_monsters {
                attack_goals.draw_chebyshev(mpos, r_f32)?;
            }
        } else {
            for &mpos in &living_group_monsters {
                attack_goals.union_assign(&tiles_around(mpos, atk_range)?)?;
            }
        }
        let moved = move_towards_closest(attack_goals)?;
        planner.note_move(idx, moved, round);
        if moved {
            try_attack_camps_in_range(me, &safe_camps);
        }
        return Ok(moved);
    }

    if planner.contains(idx, origin) {
        return Ok(true);
    }

    let group_tiles = planner.group_tiles_bitboard(idx)?;
    let moved = move_towards_closest(group_tiles)?;
    planner.note_move(idx, moved, round);
    if moved {
        try_attack_camps_in_range(me, &safe_camps);
    }
    Ok(moved)
}

fn build_plan_for_champion(
    champ: Option<ChampionType>,
    intel: &DraftIntel,
) -> (Vec<(EmblemFamily, StatKind)>, [(EmblemFamily, StatKind); 3]) {
    let mut plan: Vec<(EmblemFamily, StatKind)> = Vec::with_capacity(6);
    let pve_monster_map = intel.is_pve_monster_build_map();
    let heavy_jungle =
        intel.is_heavy_jungle() || intel.is_rows_map() || intel.monolith_count >= 1;
    let large_or_distant =
        intel.is_large_map() || intel.is_distant_altars() || intel.is_chokepoint();

    match champ {
        Some(ChampionType::VesperVale) => {
            plan.push((EmblemFamily::Treasure, StatKind::Strength));
            if pve_monster_map {
                plan.push((EmblemFamily::Monster, StatKind::Strength));
                plan.push((EmblemFamily::Jungle, StatKind::Health));
            } else {
                plan.push((EmblemFamily::Bruiser, StatKind::Strength));
                plan.push((EmblemFamily::Durability, StatKind::Health));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                ],
            )
        }

        Some(ChampionType::Vendetta) => {
            if pve_monster_map {
                plan.push((EmblemFamily::Monster, StatKind::Strength));
                plan.push((EmblemFamily::Jungle, StatKind::Health));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Strength),
                        (EmblemFamily::Bruiser, StatKind::Strength),
                        (EmblemFamily::Bruiser, StatKind::Strength),
                    ],
                )
            } else {
                plan.push((EmblemFamily::Hunter, StatKind::Health));
                plan.push((EmblemFamily::Jungle, StatKind::Strength));
                plan.push((EmblemFamily::Monster, StatKind::Strength));
                plan.push((EmblemFamily::Durability, StatKind::Health));
                plan.push((EmblemFamily::Capture, StatKind::Health));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Strength),
                        (EmblemFamily::Bruiser, StatKind::Strength),
                        (EmblemFamily::Bruiser, StatKind::Strength),
                    ],
                )
            }
        }

        Some(ChampionType::HeartOfRuin) => {
            if pve_monster_map {
                plan.push((EmblemFamily::Monster, StatKind::Focus));
                plan.push((EmblemFamily::Jungle, StatKind::Health));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Focus),
                        (EmblemFamily::Bruiser, StatKind::Focus),
                        (EmblemFamily::Bruiser, StatKind::Focus),
                    ],
                )
            } else {
                plan.push((EmblemFamily::Hunter, StatKind::Health));
                plan.push((EmblemFamily::Jungle, StatKind::Focus));
                plan.push((EmblemFamily::Durability, StatKind::Health));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Health),
                        (EmblemFamily::Bruiser, StatKind::Health),
                        (EmblemFamily::Bruiser, StatKind::Focus),
                    ],
                )
            }
        }

        Some(ChampionType::Cassi) => {
            if pve_monster_map {
                plan.push((EmblemFamily::Monster, StatKind::Focus));
                plan.push((EmblemFamily::Jungle, StatKind::Health));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Focus),
                        (EmblemFamily::Bruiser, StatKind::Focus),
                        (EmblemFamily::Bruiser, StatKind::Focus),
                    ],
                )
            } else {
                plan.push((EmblemFamily::Bruiser, StatKind::Focus));
                plan.push((EmblemFamily::Durability, StatKind::Focus));
                (
                    plan,
                    [
                        (EmblemFamily::Bruiser, StatKind::Health),
                        (EmblemFamily::Bruiser, StatKind::Health),
                        (EmblemFamily::Bruiser, StatKind::Health),
                    ],
                )
            }
        }

        Some(ChampionType::Issac) => {
            if pve_monster_map {
                plan.push((EmblemFamily::Monster, StatKind::Strength));
                plan.push((EmblemFamily::Jungle, StatKind::Health));
            } else {
                plan.push((EmblemFamily::Treasure, StatKind::Strength));
                plan.push((EmblemFamily::Bruiser, StatKind::Strength));
                plan.push((EmblemFamily::Home, StatKind::Health));
                plan.push((EmblemFamily::Durability, StatKind::Health));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                ],
            )
        }

        Some(ChampionType::Ryth) => {
            plan.push((EmblemFamily::Bruiser, StatKind::Health));
            plan.push((EmblemFamily::Durability, StatKind::Health));
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                ],
            )
        }

        Some(ChampionType::Varg) => {
            plan.push((EmblemFamily::Monster, StatKind::Strength));
            plan.push((EmblemFamily::Jungle, StatKind::Health));
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                ],
            )
        }

        Some(ChampionType::Marcella) => {
            if crate::profile::has(crate::profile::PRISM_CARRY) {
                plan.push((EmblemFamily::Treasure, StatKind::Health));
                plan.push((EmblemFamily::Jungle, StatKind::Focus));
                plan.push((EmblemFamily::Monster, StatKind::Focus));
                plan.push((EmblemFamily::Durability, StatKind::Health));
            } else {
                plan.push((EmblemFamily::Jungle, StatKind::Strength));
                plan.push((EmblemFamily::Durability, StatKind::Health));
                plan.push((EmblemFamily::Bruiser, StatKind::Strength));
                plan.push((EmblemFamily::Bruiser, StatKind::Health));
                plan.push((EmblemFamily::Bruiser, StatKind::Focus));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Health),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                ],
            )
        }

        Some(ChampionType::Prism) => {
            plan.push((EmblemFamily::Monster, StatKind::Focus));
            plan.push((EmblemFamily::Durability, StatKind::Health));
            plan.push((EmblemFamily::Jungle, StatKind::Health));
            plan.push((EmblemFamily::Home, StatKind::Health));
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Health),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                ],
            )
        }

        Some(ChampionType::Willow) => {
            plan.push((EmblemFamily::Jungle, StatKind::Strength));
            plan.push((EmblemFamily::Durability, StatKind::Health));
            plan.push((EmblemFamily::Monster, StatKind::Strength));
            if large_or_distant {
                plan.push((EmblemFamily::Home, StatKind::Focus));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                ],
            )
        }

        Some(ChampionType::Ren) => {
            plan.push((EmblemFamily::Durability, StatKind::Health));
            plan.push((EmblemFamily::Jungle, StatKind::Focus));
            plan.push((EmblemFamily::Monster, StatKind::Focus));
            if large_or_distant {
                plan.push((EmblemFamily::Home, StatKind::Focus));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Health),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                ],
            )
        }

        Some(ChampionType::Tavi) => {
            plan.push((EmblemFamily::Treasure, StatKind::Focus));
            plan.push((EmblemFamily::Monster, StatKind::Focus));
            plan.push((EmblemFamily::Durability, StatKind::Health));
            if heavy_jungle {
                plan.push((EmblemFamily::Jungle, StatKind::Focus));
            }
            plan.push((EmblemFamily::Bruiser, StatKind::Focus));
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Strength),
                ],
            )
        }

        Some(ChampionType::Stellar) => {
            plan.push((EmblemFamily::Monster, StatKind::Focus));
            plan.push((EmblemFamily::Durability, StatKind::Health));
            plan.push((EmblemFamily::Bruiser, StatKind::Focus));
            if large_or_distant {
                plan.push((EmblemFamily::Home, StatKind::Focus));
            }
            (
                plan,
                [
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Focus),
                    (EmblemFamily::Bruiser, StatKind::Health),
                ],
            )
        }

        None => (
            vec![
                (EmblemFamily::Monster, StatKind::Strength),
                (EmblemFamily::Durability, StatKind::Health),
            ],
            [
                (EmblemFamily::Bruiser, StatKind::Strength),
                (EmblemFamily::Bruiser, StatKind::Health),
                (EmblemFamily::Bruiser, StatKind::Strength),
            ],
        ),
    }
}

#[derive(Clone, Copy)]
struct Style {
    first: Option<EmblemFamily>,
    claim: f32,
    engage: i32,
}

fn style_for(intel: &DraftIntel, planner: &CampPlanner) -> Style {
    let area = u32::from(intel.map_width) * u32::from(intel.map_height);
    let elkrich = planner.heavy_camps >= 12 && area > 1300;
    if intel.enemy_mask & (BIT_CASSI | BIT_ISSAC) != 0 {
        crate::profile::enable(crate::profile::TWINS);
    }
    Style {
        first: (planner.gem_clusters >= 10).then_some(EmblemFamily::Prospector),
        claim: 0.8,
        engage: if elkrich { 6 } else { 4 },
    }
}

#[derive(Default)]
pub struct Bot {
    style: Option<Style>,
    shards: u32,
    completed_emblems: usize,
    completed_passives: u16,
    current_emblem_shard: Option<Item>,
    altar_lock: bool,
    team_slot: Option<usize>,
    recall_after_flee: bool,
    row_trip: u8,
    intel: DraftIntel,
    explorer: Explorer,
    planner: CampPlanner,
    last_hp: f32,
    ambush_until: u32,
    capture_started_round: Option<u32>,
}

impl Bot {
    fn ensure_team_slot(&mut self, me: &Champion) -> usize {
        if let Some(slot) = self.team_slot {
            return slot;
        }
        let my_name = me.champion_name().ok();
        let my_pos = me.position().ok();
        let mut idx = 0usize;
        if let Ok(allies) = cambc::champions(Some(Team::Ally)) {
            for (i, ally) in allies.iter().enumerate() {
                if ally.champion_name().ok() == my_name && my_name.is_some() {
                    if ally.position().ok() == my_pos || idx == 0 {
                        idx = i;
                        break;
                    }
                }
            }
        }
        let slot = idx % SHARED_CAMP_SLOTS;
        self.team_slot = Some(slot);
        slot
    }

    fn choose_next_emblem_item(
        &self,
        me: &Champion,
        champ_type: Option<ChampionType>,
        is_tavi: bool,
        at_spawn_or_dead: bool,
    ) -> Item {
        let (plan, repeat) = build_plan_for_champion(champ_type, &self.intel);
        let mut plan = plan;
        if let (Some(&(_, stat)), Some(first)) = (plan.first(), self.style.and_then(|s| s.first)) {
            plan.insert(0, (first, stat));
        }
        let avoid_field_health = if is_tavi && !at_spawn_or_dead {
            let hp = me.health().unwrap_or(0.0);
            let max_hp = me.max_health().unwrap_or(1.0);
            hp / (max_hp + 75.0) <= PILGRIM_THRESHOLD + 0.06
        } else {
            false
        };

        let mut non_passive_seen = 0usize;
        let non_passive_completed = self
            .completed_emblems
            .saturating_sub(self.completed_passives.count_ones() as usize);

        for (family, mut stat) in plan {
            let pbit = family.passive_bit();
            if pbit != 0 {
                if (self.completed_passives & pbit) != 0 {
                    continue;
                }
            } else {
                if non_passive_seen < non_passive_completed {
                    non_passive_seen += 1;
                    continue;
                }
            }
            if avoid_field_health && stat == StatKind::Health {
                stat = StatKind::Focus;
            }
            return shard_item(family, stat);
        }

        let idx = self.completed_emblems % repeat.len();
        let (family, mut stat) = repeat[idx];
        if avoid_field_health && stat == StatKind::Health {
            stat = StatKind::Focus;
        }
        shard_item(family, stat)
    }

    fn buy_items(
        &mut self,
        me: &Champion,
        champ_type: Option<ChampionType>,
        is_tavi: bool,
        at_spawn_or_dead: bool,
    ) -> Result<()> {
        let cur_gold = me.gold_count().unwrap_or(0);
        if cur_gold < 10 || (cur_gold < 30 && (!at_spawn_or_dead || self.shards < 5)) {
            return Ok(());
        }

        let _ = self.planner.load();
        self.intel.ensure_loaded(&self.planner);

        while me.gold_count().unwrap_or(0) >= 30 {
            if self.shards % 5 == 0 || self.current_emblem_shard.is_none() {
                let next_item =
                    self.choose_next_emblem_item(me, champ_type, is_tavi, at_spawn_or_dead);
                self.current_emblem_shard = Some(next_item);
            }

            let item = self.current_emblem_shard.expect("set above");
            let cost = 30u32;
            let gold = me.gold_count()?;
            if gold < cost {
                break;
            }

            if is_tavi && !at_spawn_or_dead && item.health_points().unwrap_or(0) > 0 {
                let hp = me.health().unwrap_or(0.0);
                let max_hp = me.max_health().unwrap_or(1.0);
                if hp / (max_hp + 75.0) <= PILGRIM_THRESHOLD + 0.04 {
                    if self.shards % 5 == 0 {
                        let (family, _) = item_family_and_stat(item);
                        let alt = shard_item(family, StatKind::Focus);
                        self.current_emblem_shard = Some(alt);
                        continue;
                    }
                    break;
                }
            }

            let remaining = 5 - (self.shards % 5);
            let affordable = gold / cost;
            let count = std::cmp::min(affordable, remaining);
            if count == 0 || !me.can_buy(item, count).unwrap_or(false) {
                break;
            }

            if me.buy_count(item, count).is_ok() {
                self.shards += count;
                if self.shards % 5 == 0 {
                    let (family, _) = item_family_and_stat(item);
                    self.completed_passives |= family.passive_bit();
                    self.completed_emblems += 1;
                    self.current_emblem_shard = None;
                }
            } else {
                break;
            }
        }

        if at_spawn_or_dead && self.shards >= 5 {
            if me.gold_count().unwrap_or(0) >= 10
                && !has_owned_altar_lock(me)
                && me
                .can_buy_consumable(ConsumableType::AltarLock)
                .unwrap_or(false)
                && me.buy_consumable(ConsumableType::AltarLock).is_ok()
            {
                self.altar_lock = true;
            }

            let wants_crystal = self.intel.is_chokepoint()
                || self.intel.is_hole_heavy()
                || self.intel.wall_pct >= 12;
            if wants_crystal
                && me.gold_count().unwrap_or(0) >= 10
                && !has_owned_teleport_crystal(me)
                && me
                .can_buy_consumable(ConsumableType::TpCrystal)
                .unwrap_or(false)
            {
                let _ = me.buy_consumable(ConsumableType::TpCrystal);
            }
        }

        Ok(())
    }

    fn is_safe_to_recall_against(
        &self,
        my_pos: Vec2,
        enemies: &[EnemyInfo],
        near_camps: &[NearCamp],
    ) -> bool {
        let in_danger = DANGER.with(|d| {
            d.borrow()
                .as_ref()
                .is_some_and(|z| z.is_set(my_pos).unwrap_or(false))
        });
        if in_danger {
            return false;
        }

        let in_bush = cambc::map::bushes()
            .and_then(|b| b.is_set(my_pos))
            .unwrap_or(false);
        let has_home = (self.completed_passives & PASSIVE_HOME) != 0;

        for e in enemies {
            let d = distance(my_pos, e.pos);
            if d <= e.interrupt_radius {
                return false;
            }
            if !e.submitting && !e.recalling && !e.stunned {
                let extra_margin = if in_bush || has_home { 0 } else { 1 };
                if d <= e.threat_radius + extra_margin {
                    return false;
                }
            }
        }

        for camp in near_camps {
            if camp.hp > 0.0 && camp.strength > 0.0 {
                let camp_r = camp_attack_range(camp.unit.kind());
                let safe_dist = if camp.awake { camp_r } else { 0 };
                if distance(my_pos, camp.pos) <= safe_dist {
                    return false;
                }
            }
        }
        true
    }

    fn retreat_or_recall(
        &self,
        me: &Champion,
        origin: Vec2,
        has_nearby_enemy: bool,
        enemies: &[EnemyInfo],
        near_camps: &[NearCamp],
    ) -> Result<bool> {
        let spawn = cambc::map::spawn_region(Team::Ally)?;
        if spawn.is_set(origin)? {
            return Ok(false);
        }

        let recall_min_dist = if (self.completed_passives & PASSIVE_HOME) != 0 {
            3
        } else {
            5
        };
        let min_spawn_dist = spawn_distance(origin, &self.planner.ally_spawn_positions);

        let enemy_pressing = has_nearby_enemy
            || enemies
                .iter()
                .any(|e| distance(origin, e.pos) <= e.interrupt_radius + 1);
        let awake_camp_pressing = near_camps.iter().any(|c| {
            c.awake
                && c.hp > 0.0
                && c.strength > 0.0
                && distance(origin, c.pos) <= camp_attack_range(c.unit.kind())
        });

        if min_spawn_dist > recall_min_dist
            && !enemy_pressing
            && !awake_camp_pressing
            && self.is_safe_to_recall_against(origin, enemies, near_camps)
        {
            if me.recall().is_ok() {
                return Ok(true);
            }
        }

        if (enemy_pressing || awake_camp_pressing) && me.can_cast().unwrap_or(false) {
            let in_immediate_threat = enemies
                .iter()
                .any(|e| distance(origin, e.pos) <= e.interrupt_radius)
                || awake_camp_pressing;
            if in_immediate_threat {
                for slot in me.abilities() {
                    if slot.is_ready().unwrap_or(false) && slot.is_shield() {
                        let _ = cast_self_ability(me, slot);
                        break;
                    }
                }
            }
        }

        let blocked = blocked_tiles(origin)?;
        let raw_blk = raw_blocked_tiles(origin)?;
        let in_danger = DANGER.with(|d| {
            d.borrow()
                .as_ref()
                .is_some_and(|z| z.is_set(origin).unwrap_or(false))
        });

        let mut goals = spawn.clone();
        if in_danger && has_fuel(MIN_FUEL_FOR_SEARCH) {
            if let Some(escape_goals) = DANGER.with(|d| -> Result<Option<Bitboard>> {
                if let Some(ref zone) = *d.borrow() {
                    build_escape_goals_out_of_range(me, origin, spawn, &raw_blk, zone)
                } else {
                    Ok(None)
                }
            })? {
                goals = escape_goals;
            }
        } else {
            goals.intersect_assign(&blocked.complement()?)?;
            if goals.is_empty()? {
                goals = spawn.clone();
            }
        }

        let step_blocked = if in_danger { &raw_blk } else { &blocked };
        let mut moved = false;
        if enemy_pressing && dash_retreat(me, origin, &goals, step_blocked, enemies)? {
            moved = true;
        } else if move_towards_closest_with_blocked(me, origin, goals, &blocked)? {
            moved = true;
        }

        let final_pos = me.position().unwrap_or(origin);
        let final_spawn_dist = spawn_distance(final_pos, &self.planner.ally_spawn_positions);
        if final_spawn_dist > recall_min_dist
            && !me.is_recalling().unwrap_or(true)
            && self.is_safe_to_recall_against(final_pos, enemies, near_camps)
        {
            if me.recall().is_ok() {
                return Ok(true);
            }
        }

        Ok(moved)
    }

    fn clear_or_submit_at_altar(&mut self, me: &Champion, pos: Vec2, altar: Vec2) -> bool {
        try_place_altar_lock(me, pos, altar, &mut self.altar_lock);

        let enemy_locked = cambc::vision::altar_lock_at(altar)
            .map(|l| l.is_enemy())
            .unwrap_or(false);

        if enemy_locked {
            let mut hit_lock = false;
            if me.submit_gems(altar).is_ok() {
                hit_lock = true;
            } else if let Ok(lock) = cambc::vision::altar_lock_at(altar) {
                if lock.is_enemy() && me.attack(&lock).is_ok() {
                    hit_lock = true;
                }
            }

            if me.can_cast().unwrap_or(false) {
                let still_locked = cambc::vision::altar_lock_at(altar)
                    .map(|l| l.is_enemy())
                    .unwrap_or(false);
                if still_locked {
                    for slot in me.abilities() {
                        if slot.is_ready().unwrap_or(false) && slot.is_attack_reset() {
                            if cast_self_ability(me, slot) {
                                if let Ok(lock) = cambc::vision::altar_lock_at(altar) {
                                    if lock.is_enemy() && me.attack(&lock).is_ok() {
                                        hit_lock = true;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }

            let now_unlocked = !cambc::vision::altar_lock_at(altar)
                .map(|l| l.is_enemy())
                .unwrap_or(false);
            if now_unlocked {
                try_place_altar_lock(me, pos, altar, &mut self.altar_lock);
                let _ = me.submit_gems(altar);
            }
            return hit_lock || now_unlocked;
        }

        me.submit_gems(altar).is_ok()
    }

    fn try_bank_gems(
        &mut self,
        me: &Champion,
        my_pos: Vec2,
        near_camps: &[NearCamp],
    ) -> Result<bool> {
        let range = cambc::constants().submit_gems_range;
        if self.planner.altar_positions.is_empty() {
            return Ok(false);
        }

        let awake_camps: Vec<Vec2> = near_camps
            .iter()
            .filter(|c| c.awake && c.strength >= 60.0)
            .map(|c| c.pos)
            .collect();

        let mut safe_altars: Vec<Vec2> = self
            .planner
            .altar_positions
            .iter()
            .copied()
            .filter(|&al| !awake_camps.iter().any(|&cp| distance(al, cp) <= 4))
            .collect();
        if safe_altars.is_empty() {
            safe_altars = self.planner.altar_positions.clone();
        }

        let mut goals = Bitboard::new()?;

        for &altar in &safe_altars {
            if distance(my_pos, altar) <= range {
                if self.clear_or_submit_at_altar(me, my_pos, altar) {
                    return Ok(true);
                }
            }
            goals.union_assign(&tiles_around(altar, range)?)?;
        }

        if move_towards_closest(goals)? {
            let new_pos = me.position()?;
            for &altar in &safe_altars {
                if distance(new_pos, altar) <= range {
                    let _ = self.clear_or_submit_at_altar(me, new_pos, altar);
                    break;
                }
            }
            if me.can_attack().unwrap_or(false) && !me.is_submitting_gems().unwrap_or(false) {
                let blocking_camps: Vec<&NearCamp> = near_camps
                    .iter()
                    .filter(|c| {
                        c.hp > 0.0
                            && safe_altars.iter().any(|&al| distance(c.pos, al) <= 4)
                    })
                    .collect();
                let _ = try_attack_camps_in_range(me, &blocking_camps);
            }
            return Ok(true);
        }
        if !near_camps.is_empty() {
            let blocking_camps: Vec<&NearCamp> = near_camps
                .iter()
                .filter(|c| c.hp > 0.0)
                .collect();
            if try_attack_camps_in_range(me, &blocking_camps) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn play_turn(&mut self) -> Result<()> {
        let _ = self.play_turn_inner();
        Ok(())
    }

    fn play_turn_inner(&mut self) -> Result<()> {
        let me = me()?;
        let champ_type = me.champion_name().ok();
        let is_varg = matches!(champ_type, Some(ChampionType::Varg));
        let is_heart = is_heart_of_ruin(me);
        let is_tavi = matches!(champ_type, Some(ChampionType::Tavi));
        let is_willow = matches!(champ_type, Some(ChampionType::Willow));
        let is_vesper = matches!(champ_type, Some(ChampionType::VesperVale));
        let my_slot = self.ensure_team_slot(me);

        let _ = self.planner.load();
        self.intel.ensure_loaded(&self.planner);
        if self.style.is_none() && self.planner.loaded {
            self.style = Some(style_for(&self.intel, &self.planner));
        }
        let style = self.style.unwrap_or(Style {
            first: Some(EmblemFamily::Prospector),
            claim: ALLY_CLAIMED_FACTOR,
            engage: 4,
        });
        self.planner.claim = style.claim;

        let mut at_spawn_or_dead = !me.is_alive()?;

        if !me.is_alive()? {
            publish_camp_claim(my_slot, None);
            self.capture_started_round = None;
            self.planner.reset_clock(cambc::round_number()? as u32);
            self.buy_items(me, champ_type, is_tavi, true)?;
            sync_priority(me, champ_type, &[], false);
            return Ok(());
        }

        let my_pos = me.position()?;
        let round = cambc::round_number()? as u32;
        let my_gems = me.gem_count()?;
        self.planner.note_gems(round, my_gems);

        let visible_tiles = cambc::vision::visible_tiles()?;
        let camp_tiles = cambc::vision::camp_tiles()?;
        if has_fuel(35_000) {
            let _ = self.explorer.observe_with(&visible_tiles);
        }
        let visible_allies = cambc::vision::champions(Some(Team::Ally)).unwrap_or_default();

        let mut near_camps = Vec::with_capacity(8);
        if let Ok(raw_camps) = cambc::vision::camps() {
            for c in raw_camps {
                let cpos = c.spawn_position();
                let dist = distance(my_pos, cpos);
                if dist > 6 {
                    continue;
                }
                let hp = c.health().unwrap_or(0.0);
                if hp <= 0.0 {
                    continue;
                }
                let kind = c.kind();
                let strength = camp_design_strength(kind);
                let awake = if strength > 0.0 {
                    c.is_awake().unwrap_or(false)
                } else {
                    false
                };
                near_camps.push(NearCamp {
                    unit: c,
                    pos: cpos,
                    hp,
                    strength,
                    dist,
                    awake,
                });
            }
        }

        let at_spawn = cambc::map::spawn_region(Team::Ally)?.is_set(my_pos)?;
        at_spawn_or_dead = at_spawn_or_dead || at_spawn;
        if self.planner.row.is_empty() || my_gems == 0 {
            self.row_trip = 0;
        } else if self.row_trip == 0 && self.planner.row_cleared(round) {
            self.row_trip = 1;
        }
        if self.row_trip == 1 && at_spawn {
            self.row_trip = 2;
        }

        let enemy_spawn = cambc::map::spawn_region(Team::Enemy)?;
        let scanned_enemies: Vec<EnemyInfo> = scan_enemies(my_pos, 10)?
            .into_iter()
            .filter(|e| !enemy_spawn.is_set(e.pos).unwrap_or(false))
            .collect();
        self.intel
            .observe_live_rosters(&scanned_enemies, &visible_allies);

        if at_spawn {
            self.buy_items(me, champ_type, is_tavi, true)?;
        }

        let hp = me.health()?;
        let max_hp = me.max_health()?.max(1.0);
        let hp_ratio = hp / max_hp;
        let has_enemy_within_5 = scanned_enemies.iter().any(|e| e.dist <= 5);
        let has_enemy_within_3 = scanned_enemies.iter().any(|e| e.dist <= 3);

        let mut danger: Option<Bitboard> = None;
        let mut core_danger: Option<Bitboard> = None;
        for c in &near_camps {
            if !c.awake || c.strength <= 0.0 {
                continue;
            }
            let on = |m: &Option<Bitboard>| m.as_ref().is_some_and(|b| b.is_set(c.pos).unwrap_or(false));
            let guarded = (self.shards < MONOLITH_MIN_SHARDS && on(&self.planner.monolith_group_tiles))
                || (self.shards < GOLEM_MIN_SHARDS && on(&self.planner.golem_group_tiles))
                || (hp_ratio <= PILGRIM_THRESHOLD + 0.05 && c.strength >= 60.0);
            if guarded {
                let r = camp_attack_range(c.unit.kind());
                if r > 0 {
                    let zone = tiles_around(c.pos, r)?;
                    match danger.as_mut() {
                        Some(d) => d.union_assign(&zone)?,
                        None => danger = Some(zone.clone()),
                    }
                    match core_danger.as_mut() {
                        Some(d) => d.union_assign(&zone)?,
                        None => core_danger = Some(zone),
                    }
                }
            }
        }
        for e in &scanned_enemies {
            if e.dist <= 8 {
                let tzone = tiles_around(e.pos, e.threat_radius)?;
                match danger.as_mut() {
                    Some(d) => d.union_assign(&tzone)?,
                    None => danger = Some(tzone),
                }
                let czone = tiles_around(e.pos, e.atk_range)?;
                match core_danger.as_mut() {
                    Some(d) => d.union_assign(&czone)?,
                    None => core_danger = Some(czone),
                }
            }
        }
        if danger.is_some() {
            BFS_GOALS.with(|g| *g.borrow_mut() = None);
        }
        DANGER.with(|d| *d.borrow_mut() = danger);
        CORE_DANGER.with(|cd| *cd.borrow_mut() = core_danger);
        COMBAT_ENGAGE_MODE.with(|m| m.set(false));

        let mut on_reward_tile = false;
        if round > 1 {
            if let Some(i) = self.planner.group_at(my_pos) {
                if matches!(
                        self.planner
                            .group_state_cached(i, round, &visible_tiles, &camp_tiles),
                        Some(cambc::CampGroupState::RewardAvailable)
                    )
                {
                    on_reward_tile = true;
                }
            }
        }
        if on_reward_tile {
            if self.capture_started_round.is_none() {
                self.capture_started_round = Some(round);
            }
        } else {
            self.capture_started_round = None;
        }

        if me.is_recalling()? {
            let rem_time = me.remaining_channel_time().unwrap_or(5);
            let enemy_can_interrupt = scanned_enemies.iter().any(|e| {
                let reach = if rem_time <= 1 {
                    if e.stunned {
                        0
                    } else {
                        e.interrupt_radius
                    }
                } else {
                    e.interrupt_radius + 1
                };
                e.dist <= reach
            });
            let camp_can_interrupt = near_camps.iter().any(|c| {
                c.awake
                    && c.hp > 0.0
                    && c.strength > 0.0
                    && c.dist <= camp_attack_range(c.unit.kind())
            });
            let immediate_danger = enemy_can_interrupt || camp_can_interrupt;
            if !at_spawn && !immediate_danger {
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
            let _ = me.cancel_recall();
            if !at_spawn {
                let _ = self.retreat_or_recall(me, my_pos, true, &scanned_enemies, &near_camps);
                let cur = me.position().unwrap_or(my_pos);
                let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        if me.is_submitting_gems()? {
            let lost = self.last_hp - hp;
            self.last_hp = hp;
            let left = me.remaining_channel_time().unwrap_or(0).max(0) as f32;
            if lost > 0.0 && lost * left >= hp && me.cancel_submit_gems().is_ok() {
                self.ambush_until = round + 3;
            }
        }
        if me.is_submitting_gems()?
            || me.is_teleporting()?
            || (!me.can_move()? && !me.can_attack()? && !me.can_cast()?)
        {
            sync_priority(me, champ_type, &scanned_enemies, true);
            return Ok(());
        }

        let hit_unseen = hp + 1.0 < self.last_hp
            && scanned_enemies.iter().all(|e| e.dist > 4)
            && !near_camps
            .iter()
            .any(|c| c.hp > 0.0 && c.strength > 0.0 && c.dist <= 4);
        self.last_hp = hp;
        if hit_unseen && crate::profile::has(crate::profile::AMBUSH) {
            self.ambush_until = round + 3;
        }
        if !at_spawn && round < self.ambush_until {
            if self.retreat_or_recall(me, my_pos, true, &scanned_enemies, &near_camps)? {
                let cur = me.position().unwrap_or(my_pos);
                let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
            let _ = try_combat_actions(me, my_pos, &scanned_enemies, is_varg);
        }

        let enemy_in_sight = !scanned_enemies.is_empty();

        manage_nearby_altar_locks(
            me,
            my_pos,
            &self.planner.altar_positions,
            &mut self.altar_lock,
            false,
        );

        if at_spawn && hp_ratio < 0.98 && self.row_trip != 2 {
            if is_willow {
                let _ = heal_lowest_ally(me, my_pos, &visible_allies);
            }
            let in_range_enemies: Vec<EnemyInfo> = scanned_enemies
                .iter()
                .filter(|e| e.dist <= 4)
                .cloned()
                .collect();
            if !in_range_enemies.is_empty() {
                let _ = try_combat_actions(me, my_pos, &in_range_enemies, is_varg);
            }
            try_place_free_ward(me, me.position().unwrap_or(my_pos), hit_unseen);
            sync_priority(me, champ_type, &scanned_enemies, false);
            return Ok(());
        }

        let effective_pilgrim_threshold = if is_heart
            && my_gems <= 25
            && (active_shield_value(me) > 0.0
            || me
            .abilities()
            .iter()
            .any(|s| s.is_shield() && s.is_ready().unwrap_or(false)))
        {
            0.28
        } else {
            PILGRIM_THRESHOLD
        };

        if !at_spawn && hp_ratio <= effective_pilgrim_threshold {
            publish_camp_claim(my_slot, None);
            if hp_ratio <= RECALL_THRESHOLD
                && !has_enemy_within_5
                && self.is_safe_to_recall_against(my_pos, &scanned_enemies, &near_camps)
            {
                if me.recall().is_ok() {
                    sync_priority(me, champ_type, &scanned_enemies, true);
                    return Ok(());
                }
            }
            if self.retreat_or_recall(
                me,
                my_pos,
                has_enemy_within_5,
                &scanned_enemies,
                &near_camps,
            )? {
                let cur = me.position().unwrap_or(my_pos);
                let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                try_place_free_ward(me, cur, hit_unseen);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        let ganged = scanned_enemies.iter().filter(|e| e.dist <= 5).count() >= 2
            || (hp_ratio < 0.6 && scanned_enemies.iter().any(|e| e.dist <= 4));
        if !at_spawn
            && hp_ratio < 0.85
            && ganged
            && spawn_distance(my_pos, &self.planner.ally_spawn_positions) <= 6
        {
            if self.retreat_or_recall(me, my_pos, true, &scanned_enemies, &near_camps)? {
                let cur = me.position().unwrap_or(my_pos);
                let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
            let _ = try_combat_actions(me, my_pos, &scanned_enemies, is_varg);
        }

        let holding_urgent_capture = PATCH_ADAPT_CAPTURE_STATE
            && on_reward_tile
            && hp_ratio > RECALL_THRESHOLD
            && self
            .planner
            .group_at(my_pos)
            .and_then(|gi| self.planner.groups[gi].group.capture_state().ok())
            .is_some_and(|cs| {
                cs.state == CampGroupCaptureType::AllyCapturing && (1..=2).contains(&cs.timer)
            });

        let should_retreat_from_enemies = !at_spawn
            && !holding_urgent_capture
            && ((scanned_enemies.iter().any(|e| e.dist <= 4)
            && losing_matchup(me, my_pos, &scanned_enemies, &visible_allies, 4))
            || (scanned_enemies.iter().any(|e| e.dist <= 5)
            && hp_ratio < 0.72
            && losing_matchup(me, my_pos, &scanned_enemies, &visible_allies, 5)));

        if should_retreat_from_enemies {
            if self.retreat_or_recall(me, my_pos, true, &scanned_enemies, &near_camps)? {
                let cur = me.position().unwrap_or(my_pos);
                let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
            let _ = try_combat_actions(me, my_pos, &scanned_enemies, is_varg);
        }

        let team_gems = cambc::global_submitted_gems(Team::Ally).unwrap_or(0);
        let visible_ally_gems: u32 = visible_allies
            .iter()
            .filter(|a| a.position().ok() != Some(my_pos))
            .map(|a| a.gem_count().unwrap_or(0))
            .sum();
        let can_win_now =
            my_gems > 0 && team_gems.saturating_add(my_gems).saturating_add(visible_ally_gems) >= 1000;
        let endgame_bank = my_gems > 0 && round >= ENDGAME_BANK_ROUND;
        if (can_win_now || endgame_bank) && self.try_bank_gems(me, my_pos, &near_camps)? {
            let cur = me.position().unwrap_or(my_pos);
            try_place_free_ward(me, cur, hit_unseen);
            sync_priority(me, champ_type, &scanned_enemies, true);
            return Ok(());
        }

        if round > 1 && (!has_enemy_within_3 || holding_urgent_capture) {
            self.planner.load()?;
            self.planner.aim(my_pos);

            let cur_pos = me.position().unwrap_or(my_pos);
            self.planner.aim(cur_pos);

            if let Some((reward_idx, tiles)) = reward_waiting(
                &mut self.planner,
                cur_pos,
                round,
                is_vesper,
                &visible_tiles,
                &camp_tiles,
                &visible_allies,
            )? {
                publish_camp_claim(my_slot, Some(reward_idx));
                let cur_in_danger = DANGER.with(|d| {
                    d.borrow()
                        .as_ref()
                        .is_some_and(|z| z.is_set(cur_pos).unwrap_or(false))
                });
                let mut capture_done = false;
                if tiles.is_set(cur_pos)? {
                    if cur_in_danger && me.can_move().unwrap_or(false) {
                        if let Some(safer_group_tiles) = DANGER.with(|d| -> Option<Bitboard> {
                            let z = d.borrow();
                            let zone = z.as_ref()?;
                            let z_comp = zone.complement().ok()?;
                            let safe_t = tiles.intersect(&z_comp).ok()?;
                            (!safe_t.is_empty().unwrap_or(true)).then_some(safe_t)
                        }) {
                            let _ = move_towards_closest(safer_group_tiles);
                        }
                    }
                    capture_done = true;
                } else if move_towards_closest(tiles)? {
                    capture_done = true;
                }
                if capture_done {
                    let cur = me.position().unwrap_or(cur_pos);
                    if !scanned_enemies.is_empty() {
                        let _ = try_combat_actions(me, cur, &scanned_enemies, is_varg);
                    }
                    try_place_free_ward(me, cur, hit_unseen);
                    sync_priority(me, champ_type, &scanned_enemies, false);
                    return Ok(());
                }
            }
        }

        let is_prism = matches!(champ_type, Some(ChampionType::Prism))
            || (me.attack_range().unwrap_or(1) > 1
            && me.abilities()[0].is_cc()
            && me.abilities()[1].is_aoe_damage());
        let is_marcella = matches!(champ_type, Some(ChampionType::Marcella));
        let big = crate::profile::has(crate::profile::BIG_BAGS);
        let bank_threshold = if is_prism && hp_ratio > 0.50 {
            95
        } else if big && hp_ratio >= 0.50 {
            50
        } else if is_marcella && hp_ratio >= 0.55 {
            20
        } else if hp_ratio < 0.50 {
            10
        } else {
            20
        };
        let near_altar_with_gems = (!is_prism || my_gems >= 70 || hp_ratio < 0.48)
            && my_gems >= 6
            && self
            .planner
            .altar_positions
            .iter()
            .any(|&al| distance(my_pos, al) <= 6);

        let wants_bank = if self.planner.row.is_empty() {
            my_gems >= bank_threshold || near_altar_with_gems
        } else {
            my_gems > 0 && (self.row_trip > 0 || my_gems > 150)
        };

        let right_at_altar = self
            .planner
            .altar_positions
            .iter()
            .any(|&al| distance(my_pos, al) <= 2);
        if wants_bank
            && right_at_altar
            && !scanned_enemies.iter().any(|e| e.dist <= 2 || e.submitting)
        {
            if self.try_bank_gems(me, my_pos, &near_camps)? {
                let cur = me.position().unwrap_or(my_pos);
                try_place_free_ward(me, cur, hit_unseen);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        if is_willow {
            let _ = heal_lowest_ally(me, my_pos, &visible_allies)?;
        }

        let engage_range = if is_varg || is_heart {
            style.engage + 1
        } else {
            style.engage.max(5)
        };
        let combat_targets: Vec<EnemyInfo> = scanned_enemies
            .iter()
            .filter(|e| e.dist <= engage_range || e.submitting || (e.recalling && e.dist <= 5))
            .cloned()
            .collect();

        if engage_enemies(
            me,
            my_pos,
            hp_ratio,
            &combat_targets,
            &visible_allies,
            is_varg,
            &self.planner.altar_positions,
            &near_camps,
        )? {
            let pos_now = me.position().unwrap_or(my_pos);
            manage_nearby_altar_locks(
                me,
                pos_now,
                &self.planner.altar_positions,
                &mut self.altar_lock,
                true,
            );
            try_place_free_ward(me, pos_now, hit_unseen);
            sync_priority(me, champ_type, &scanned_enemies, false);
            return Ok(());
        }

        if PATCH_ADAPT_FUEL_GUARD && !has_fuel(MIN_FUEL_FOR_TURN_TAIL) {
            sync_priority(me, champ_type, &scanned_enemies, false);
            return Ok(());
        }

        let nearby_living_monster = near_camps.iter().any(|c| c.hp > 0.0 && c.dist <= 4);

        if at_spawn || hp_ratio >= 0.95 {
            self.recall_after_flee = false;
        }
        if let Some(threats) = losing_camp_fight(
            me,
            my_pos,
            self.completed_passives,
            &near_camps,
            &visible_allies,
        )? {
            self.recall_after_flee = true;
            publish_camp_claim(my_slot, None);
            if flee_from(me, my_pos, &threats)? {
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }
        if self.recall_after_flee && !at_spawn {
            if my_gems > 0
                && self
                    .planner
                    .altar_positions
                    .iter()
                    .any(|&al| distance(my_pos, al) <= 4)
                && self.try_bank_gems(me, my_pos, &near_camps)?
            {
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
            if self.retreat_or_recall(
                me,
                my_pos,
                has_enemy_within_5,
                &scanned_enemies,
                &near_camps,
            )? {
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        if self.row_trip == 1
            && !at_spawn
            && self.is_safe_to_recall_against(my_pos, &scanned_enemies, &near_camps)
            && me.recall().is_ok()
        {
            sync_priority(me, champ_type, &scanned_enemies, true);
            return Ok(());
        }

        if wants_bank {
            if self.try_bank_gems(me, my_pos, &near_camps)? {
                let cur = me.position().unwrap_or(my_pos);
                try_place_free_ward(me, cur, hit_unseen);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        let shop_recall_gold = if self.completed_emblems == 0 {
            if self.shards == 0 {
                90
            } else {
                (5 - (self.shards % 5)) * 30
            }
        } else if (self.completed_passives & PASSIVE_HOME) != 0 || is_vesper {
            150
        } else {
            180
        };
        let low_hp_between_camps = !at_spawn
            && !on_reward_tile
            && !nearby_living_monster
            && !wants_bank
            && (hp_ratio <= 0.44 || (hp_ratio <= 0.54 && me.gold_count().unwrap_or(0) >= 90));

        if is_tavi && me.gold_count()? >= 30 {
            self.buy_items(me, champ_type, is_tavi, at_spawn_or_dead)?;
        }
        if !at_spawn
            && !on_reward_tile
            && !wants_bank
            && !nearby_living_monster
            && (low_hp_between_camps
                || (!is_tavi && !has_enemy_within_5 && me.gold_count()? >= shop_recall_gold))
        {
            if self.retreat_or_recall(
                me,
                my_pos,
                has_enemy_within_5,
                &scanned_enemies,
                &near_camps,
            )? {
                let cur = me.position().unwrap_or(my_pos);
                try_place_free_ward(me, cur, hit_unseen);
                sync_priority(me, champ_type, &scanned_enemies, true);
                return Ok(());
            }
        }

        if PATCH_ADAPT_FUEL_GUARD && !has_fuel(MIN_FUEL_FOR_TURN_TAIL) {
            sync_priority(me, champ_type, &scanned_enemies, false);
            return Ok(());
        }

        let farmed = farm_camps(
            me,
            &mut self.planner,
            self.shards,
            self.completed_passives,
            my_slot,
            &visible_tiles,
            &camp_tiles,
            &visible_allies,
            &near_camps,
        )?;
        let any_living_monster = near_camps.iter().any(|c| c.hp > 0.0);
        if !farmed && !enemy_in_sight && !any_living_monster {
            let _ = self.explorer.explore(me, me.position()?);
        }

        let final_pos = me.position().unwrap_or(my_pos);
        manage_nearby_altar_locks(
            me,
            final_pos,
            &self.planner.altar_positions,
            &mut self.altar_lock,
            true,
        );
        try_place_free_ward(me, final_pos, hit_unseen);
        sync_priority(me, champ_type, &scanned_enemies, false);
        Ok(())
    }
}