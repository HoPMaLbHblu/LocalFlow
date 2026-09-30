//! Curated hero traits for heuristics. OWNER: research agent.
//!
//! This table is game knowledge written by hand, NOT measured data. Everything derived from it
//! is labelled `Evidence::Heuristic`. Values are deliberately conservative; heroes missing from
//! the table (new heroes, or ones we were unsure about) fall back to the source's `roles` list.
//!
//! Each row: hero id, short name, position fit for positions 1-5 as digits 0-9 (9 = typical,
//! 0 = almost never), and trait letters:
//!
//! | letter | trait |
//! |---|---|
//! | M | deals mostly magic damage |
//! | P | deals mostly physical (right-click) damage |
//! | S | reliable stun / hard disable |
//! | H | healing, lifesteal or strong regeneration |
//! | I | illusions |
//! | U | summons / controllable units |
//! | V | invisibility |
//! | E | evasion or blind (attacks miss) |
//! | N | initiation |
//! | A | can save allies |
//! | Z | area damage |

use super::{Hero, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trait {
    MagicDamage,
    PhysicalDamage,
    Disable,
    Heal,
    Illusions,
    Summons,
    Invisibility,
    Evasion,
    Initiation,
    Save,
    AreaDamage,
}

impl Trait {
    pub const ALL: [Trait; 11] = [
        Trait::MagicDamage,
        Trait::PhysicalDamage,
        Trait::Disable,
        Trait::Heal,
        Trait::Illusions,
        Trait::Summons,
        Trait::Invisibility,
        Trait::Evasion,
        Trait::Initiation,
        Trait::Save,
        Trait::AreaDamage,
    ];

    fn letter(self) -> char {
        match self {
            Trait::MagicDamage => 'M',
            Trait::PhysicalDamage => 'P',
            Trait::Disable => 'S',
            Trait::Heal => 'H',
            Trait::Illusions => 'I',
            Trait::Summons => 'U',
            Trait::Invisibility => 'V',
            Trait::Evasion => 'E',
            Trait::Initiation => 'N',
            Trait::Save => 'A',
            Trait::AreaDamage => 'Z',
        }
    }

    /// "magic damage", for sentences.
    pub fn describe(self) -> &'static str {
        match self {
            Trait::MagicDamage => "magic damage",
            Trait::PhysicalDamage => "physical damage",
            Trait::Disable => "a reliable stun or disable",
            Trait::Heal => "healing or strong regeneration",
            Trait::Illusions => "illusions",
            Trait::Summons => "summoned units",
            Trait::Invisibility => "invisibility",
            Trait::Evasion => "evasion or blind",
            Trait::Initiation => "initiation",
            Trait::Save => "ways to save allies",
            Trait::AreaDamage => "area damage",
        }
    }

    /// "Magic damage", a short capitalised label for lists (hero lookup).
    pub fn label(self) -> &'static str {
        match self {
            Trait::MagicDamage => "Magic damage",
            Trait::PhysicalDamage => "Physical damage",
            Trait::Disable => "Stun or disable",
            Trait::Heal => "Healing or regeneration",
            Trait::Illusions => "Illusions",
            Trait::Summons => "Summons",
            Trait::Invisibility => "Invisibility",
            Trait::Evasion => "Evasion or blind",
            Trait::Initiation => "Initiation",
            Trait::Save => "Saves allies",
            Trait::AreaDamage => "Area damage",
        }
    }
}

/// (id, short name, position fit 1-5, traits)
const TABLE: &[(u32, &str, &str, &str)] = &[
    (1, "antimage", "90000", "P"),
    (2, "axe", "02930", "PSNZ"),
    (3, "bane", "00069", "MSA"),
    (4, "bloodseeker", "74300", "PM"),
    (5, "crystal_maiden", "00039", "MSZ"),
    (6, "drow_ranger", "93000", "P"),
    (7, "earthshaker", "00386", "MSNZ"),
    (8, "juggernaut", "92000", "PH"),
    (9, "mirana", "01065", "MSV"),
    (10, "morphling", "95000", "PM"),
    (11, "nevermore", "58000", "PMZ"),
    (12, "phantom_lancer", "92000", "PI"),
    (13, "puck", "08030", "MSNZ"),
    (14, "pudge", "02565", "MSN"),
    (15, "razor", "56600", "P"),
    (16, "sand_king", "00685", "MSNZ"),
    (17, "storm_spirit", "08000", "MS"),
    (18, "sven", "90300", "PSZ"),
    (19, "tiny", "36470", "PMSN"),
    (20, "vengefulspirit", "00068", "MSA"),
    (21, "windrunner", "36350", "PMSE"),
    (22, "zuus", "08050", "MZ"),
    (23, "kunkka", "37500", "PSNZ"),
    (25, "lina", "58050", "MSZ"),
    (26, "lion", "00058", "MS"),
    (27, "shadow_shaman", "00058", "MSU"),
    (28, "slardar", "20900", "PSN"),
    (29, "tidehunter", "00900", "PSNZ"),
    (30, "witch_doctor", "00059", "MSHZ"),
    (31, "lich", "00049", "MSA"),
    (32, "riki", "40150", "PVE"),
    (33, "enigma", "00660", "MSNUZ"),
    (34, "tinker", "07030", "MZ"),
    (35, "sniper", "56040", "P"),
    (36, "necrolyte", "05500", "MHZ"),
    (37, "warlock", "00069", "MSHUZ"),
    (38, "beastmaster", "00860", "PSU"),
    (39, "queenofpain", "09040", "MZ"),
    (40, "venomancer", "00565", "MUZ"),
    (41, "faceless_void", "91300", "PSNE"),
    (42, "skeleton_king", "90400", "PSH"),
    (43, "death_prophet", "06600", "MUZ"),
    (44, "phantom_assassin", "93000", "PE"),
    (45, "pugna", "05056", "MZ"),
    (46, "templar_assassin", "59000", "PV"),
    (47, "viper", "36600", "PM"),
    (48, "luna", "92000", "PMZ"),
    (49, "dragon_knight", "54500", "PSZ"),
    (50, "dazzle", "00059", "HA"),
    (51, "rattletrap", "00570", "MSN"),
    (52, "leshrac", "07430", "MSZ"),
    (53, "furion", "44470", "PU"),
    (54, "life_stealer", "92000", "PH"),
    (55, "dark_seer", "00900", "MNZ"),
    (56, "clinkz", "74040", "PV"),
    (57, "omniknight", "00567", "HA"),
    (58, "enchantress", "30048", "PHU"),
    (59, "huskar", "57500", "PMH"),
    (60, "night_stalker", "03800", "PMSN"),
    (61, "broodmother", "36600", "PU"),
    (62, "bounty_hunter", "00385", "PV"),
    (63, "weaver", "70070", "PV"),
    (64, "jakiro", "00059", "MSZ"),
    (65, "batrider", "05570", "MSN"),
    (66, "chen", "00069", "HU"),
    (67, "spectre", "90000", "P"),
    (68, "ancient_apparition", "00049", "MZ"),
    (69, "doom_bringer", "00860", "MS"),
    (70, "ursa", "91000", "P"),
    (71, "spirit_breaker", "00580", "PSN"),
    (72, "gyrocopter", "80056", "PMZ"),
    (73, "alchemist", "74400", "PSH"),
    (74, "invoker", "08040", "MSUZ"),
    (75, "silencer", "00259", "MZ"),
    (76, "obsidian_destroyer", "58000", "MA"),
    (77, "lycan", "60600", "PU"),
    (78, "brewmaster", "00840", "PMSNU"),
    (79, "shadow_demon", "00069", "MSA"),
    (80, "lone_druid", "76400", "PU"),
    (81, "chaos_knight", "73600", "PSI"),
    (82, "meepo", "58000", "PSU"),
    (83, "treant", "00059", "SHN"),
    (84, "ogre_magi", "00469", "MS"),
    (85, "undying", "00069", "MHU"),
    (86, "rubick", "00095", "MS"),
    (87, "disruptor", "00059", "MSZ"),
    (88, "nyx_assassin", "00095", "MSV"),
    (89, "naga_siren", "90055", "PISA"),
    (90, "keeper_of_the_light", "04059", "MZ"),
    (91, "wisp", "00069", "HA"),
    (92, "visage", "07350", "MU"),
    (93, "slark", "92000", "PHV"),
    (94, "medusa", "95000", "PZ"),
    (95, "troll_warlord", "92000", "P"),
    (96, "centaur", "00900", "PMSN"),
    (97, "magnataur", "04850", "PSNZ"),
    (98, "shredder", "06800", "MZ"),
    (99, "bristleback", "30900", "PZ"),
    (100, "tusk", "00096", "PSN"),
    (101, "skywrath_mage", "00088", "M"),
    (102, "abaddon", "30668", "MHA"),
    (103, "elder_titan", "00067", "MSN"),
    (104, "legion_commander", "00900", "PSN"),
    (105, "techies", "00077", "MSZ"),
    (106, "ember_spirit", "39000", "PMZ"),
    (107, "earth_spirit", "00095", "MSN"),
    (108, "abyssal_underlord", "00950", "MSZ"),
    (109, "terrorblade", "90000", "PI"),
    (110, "phoenix", "00590", "MHNZ"),
    (111, "oracle", "00059", "MHA"),
    (112, "winter_wyvern", "00059", "MSA"),
    (113, "arc_warden", "88000", "PM"),
    (114, "monkey_king", "65070", "PSN"),
    (119, "dark_willow", "00097", "MS"),
    (120, "pangolier", "05670", "PMSN"),
    (121, "grimstroke", "00069", "MS"),
    (123, "hoodwink", "00088", "PMS"),
    (126, "void_spirit", "07040", "MSN"),
    (128, "snapfire", "00089", "MSZ"),
    (129, "mars", "03900", "PSNZ"),
    (131, "ringmaster", "00069", "MS"),
    (135, "dawnbreaker", "00860", "PHSN"),
    (136, "marci", "30380", "PS"),
    (137, "primal_beast", "03900", "PMSN"),
    (138, "muerta", "86000", "PM"),
    (145, "kez", "65000", "P"),
    // Not listed (unsure): largo. Falls back to the source's roles.
];

/// A hero's curated traits.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroTraits {
    pub id: u32,
    pub short_name: &'static str,
    /// Fit for positions 1-5, 0.0 - 1.0.
    pub positions: [f32; 5],
    pub letters: &'static str,
}

impl HeroTraits {
    pub fn has(&self, t: Trait) -> bool {
        self.letters.contains(t.letter())
    }
}

fn row(r: &(u32, &'static str, &'static str, &'static str)) -> HeroTraits {
    let mut positions = [0.0; 5];
    for (i, c) in r.2.chars().take(5).enumerate() {
        positions[i] = c.to_digit(10).unwrap_or(0) as f32 / 9.0;
    }
    HeroTraits { id: r.0, short_name: r.1, positions, letters: r.3 }
}

/// The curated row for a hero (by short name, then id), if the table has one.
pub fn curated(hero: &Hero) -> Option<HeroTraits> {
    let short = hero.short_name();
    TABLE.iter().find(|r| r.1 == short).or_else(|| TABLE.iter().find(|r| r.0 == hero.id)).map(row)
}

/// Every curated row (used when the statistics source has no hero list at all).
pub fn all_curated() -> Vec<HeroTraits> {
    TABLE.iter().map(row).collect()
}

fn role_index(role: Role) -> usize {
    match role {
        Role::Carry => 0,
        Role::Mid => 1,
        Role::Offlane => 2,
        Role::SoftSupport => 3,
        Role::HardSupport => 4,
    }
}

/// Position fit guessed from the source's role labels (for heroes not in the table).
fn fit_from_roles(hero: &Hero) -> [f32; 5] {
    let has = |r: &str| hero.roles.iter().any(|x| x.eq_ignore_ascii_case(r));
    let mut fit: [f32; 5] = [0.2; 5];
    let mut raise = |i: usize, v: f32| fit[i] = fit[i].max(v);
    if has("Carry") {
        raise(0, 0.7);
        raise(1, 0.5);
    }
    if has("Support") {
        raise(3, 0.7);
        raise(4, 0.7);
    }
    if has("Initiator") || has("Durable") {
        raise(2, 0.6);
        raise(3, 0.4);
    }
    if has("Nuker") {
        raise(1, 0.5);
        raise(3, 0.4);
    }
    if has("Disabler") {
        raise(3, 0.5);
        raise(4, 0.5);
    }
    if has("Escape") {
        raise(1, 0.4);
    }
    fit
}

/// How well a hero usually fits a position, 0.0 - 1.0.
pub fn role_fit(hero: &Hero, role: Role) -> f32 {
    let fit = curated(hero).map(|t| t.positions).unwrap_or_else(|| fit_from_roles(hero));
    fit[role_index(role)]
}

/// The position the hero is most often played in.
pub fn main_role(hero: &Hero) -> Role {
    let roles = [Role::Carry, Role::Mid, Role::Offlane, Role::SoftSupport, Role::HardSupport];
    let mut best = Role::Carry;
    let mut best_fit = -1.0;
    for r in roles {
        let f = role_fit(hero, r);
        if f > best_fit {
            best = r;
            best_fit = f;
        }
    }
    best
}

/// Whether the hero has a trait: from the table, or guessed from the source's roles.
pub fn has_trait(hero: &Hero, t: Trait) -> bool {
    if let Some(c) = curated(hero) {
        return c.has(t);
    }
    let has = |r: &str| hero.roles.iter().any(|x| x.eq_ignore_ascii_case(r));
    match t {
        Trait::MagicDamage => has("Nuker"),
        Trait::PhysicalDamage => has("Carry"),
        Trait::Disable => has("Disabler"),
        Trait::Initiation => has("Initiator"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero(id: u32, short: &str, roles: &[&str]) -> Hero {
        Hero {
            id,
            name: format!("npc_dota_hero_{short}"),
            localized_name: short.into(),
            primary_attr: "agi".into(),
            attack_type: "Melee".into(),
            roles: roles.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn table_is_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for r in TABLE {
            assert_eq!(r.2.len(), 5, "{}", r.1);
            assert!(r.2.chars().all(|c| c.is_ascii_digit()), "{}", r.1);
            assert!(r.3.chars().all(|c| "MPSHIUVENAZ".contains(c)), "{}", r.1);
            assert!(ids.insert(r.0), "duplicate id {}", r.0);
        }
    }

    #[test]
    fn curated_and_fallback_fits() {
        let am = hero(1, "antimage", &[]);
        assert!(role_fit(&am, Role::Carry) > 0.9);
        assert!(role_fit(&am, Role::HardSupport) < 0.1);
        assert!(has_trait(&am, Trait::PhysicalDamage));
        let unknown = hero(999, "brand_new", &["Support", "Disabler"]);
        assert!(role_fit(&unknown, Role::HardSupport) >= 0.7);
        assert!(role_fit(&unknown, Role::Carry) < 0.3);
        assert!(has_trait(&unknown, Trait::Disable));
        assert_eq!(main_role(&am), Role::Carry);
    }
}
