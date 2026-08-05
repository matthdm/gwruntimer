use std::time::Duration;
use crate::map_names;
use crate::run_log::RouteRecord;

pub struct RunCategory {
    pub name: &'static str,
    pub zones: &'static [&'static str],
    pub exclude_explorables: &'static [&'static str],
}

impl RunCategory {
    pub fn matches(&self, from_map_id: u32, run_map_id: u32, to_map_id: u32) -> bool {
        let from_name = map_names::map_name_by_id(from_map_id);
        let run_name = map_names::map_name_by_id(run_map_id);
        let to_name = map_names::map_name_by_id(to_map_id);

        let norm_run = normalize(run_name);
        if self.exclude_explorables.iter().any(|&z| normalize(z) == norm_run) {
            return false;
        }

        self.contains_zone(from_name) && self.contains_zone(run_name) && self.contains_zone(to_name)
    }

    fn contains_zone(&self, name: &str) -> bool {
        let norm_target = normalize(name);
        self.zones.iter().any(|&z| normalize(z) == norm_target)
    }

    /// Calculate summary statistics for all matching route records in this category.
    pub fn summarize(&self, records: &[RouteRecord]) -> CategorySummary {
        let mut matching = Vec::new();
        let mut total_pb = Duration::ZERO;
        let mut total_avg = Duration::ZERO;

        for rec in records {
            if self.matches(rec.from_map_id, rec.run_map_id, rec.to_map_id) {
                total_pb += rec.pb;
                total_avg += rec.avg;
                matching.push(*rec);
            }
        }

        let zone_count = matching.len();
        CategorySummary {
            records: matching,
            total_pb,
            total_avg,
            zone_count,
        }
    }
}

pub struct CategorySummary {
    pub records: Vec<RouteRecord>,
    pub total_pb: Duration,
    pub total_avg: Duration,
    pub zone_count: usize,
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub static ALL_CATEGORIES: &[RunCategory] = &[
    RunCategory {
        name: "Ascalon <-> Lion's Arch",
        zones: &[
            "Ascalon City",
            "Old Ascalon",
            "Sardelac Sanitarium",
            "The Breach",
            "Piken Square",
            "Diessa Lowlands",
            "Grendich Courthouse",
            "Nolani Academy",
            "Ascalon Foothills",
            "Traveler's Vale",
            "Yak's Bend",
            "Borlis Pass",
            "Iron Horse Mine",
            "Anvil Rock",
            "Ice Tooth Cave",
            "The Frost Gate",
            "Deldrimor Bowl",
            "Beacon's Perch",
            "Griffon's Mouth",
            "Scoundrel's Rise",
            "Gates of Kryta",
            "North Kryta Province",
            "Lion's Arch",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Lion's Arch -> Temple of the Ages",
        zones: &[
            "Lion's Arch",
            "North Kryta Province",
            "Nebo Terrace",
            "Bergen Hot Springs",
            "Beetletun",
            "Cursed Lands",
            "The Black Curtain",
            "Temple of the Ages",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Temple of the Ages -> Sanctum Cay",
        zones: &[
            "Temple of the Ages",
            "The Black Curtain",
            "Talmark Wilderness",
            "Tears of the Fallen",
            "Fisherman's Haven",
            "Fishermen's Haven",
            "Stingray Strand",
            "Sanctum Cay",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Maguuma Tour",
        zones: &[
            "Talmark Wilderness",
            "Majesty's Rest",
            "Sage Lands",
            "Druid's Overlook",
            "The Wilderness",
            "The Wilds",
            "Mamnoon Lagoon",
            "Bloodstone Fen",
            "Silverwood",
            "Quarrel Falls",
            "Ettin's Back",
            "Ventari's Refuge",
            "Aurora Glade",
            "Dry Top",
            "Henge of Denravi",
            "Tangle Root",
            "Maguuma Stade",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Droks Run",
        zones: &[
            "Lion's Arch",
            "North Kryta Province",
            "Scoundrel's Rise",
            "Griffon's Mouth",
            "Deldrimor Bowl",
            "Beacon's Perch",
            "Lornar's Pass",
            "Dreadnought's Drift",
            "Snake Dance",
            "Camp Rankor",
            "Talus Chute",
            "Droknar's Forge",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Shiverpeaks Tour",
        zones: &[
            "Droknar's Forge",
            "Talus Chute",
            "Icedome",
            "Frozen Forest",
            "Iron Mines of Moladune",
            "Thunderhead Keep",
            "Ice Floe",
            "Marhan's Grotto",
            "Witman's Folly",
            "Port Sledge",
            "Camp Rankor",
            "Snake Dance",
            "Snake's Dance",
            "Grenth's Footprint",
            "Deldrimor War Camp",
            "Spearhead Peak",
            "The Granite Citadel",
            "Copperhammer Mines",
        ],
        exclude_explorables: &[
            "Iron Mines of Moladune",
        ],
    },
    RunCategory {
        name: "Desert Tour",
        zones: &[
            "The Amnoon Oasis",
            "Amnoon Oasis",
            "Prophets Path",
            "Prophet's Path",
            "Heroes' Audience",
            "Augury Rock",
            "Vulture Drifts",
            "Dunes of Despair",
            "Skyward Reach",
            "Destiny's Gorge",
            "Diviner's Ascent",
            "Elona Reach",
            "Salt Flats",
            "Seeker's Passage",
            "The Scar",
            "Thirsty River",
            "The Arid Sea",
        ],
        exclude_explorables: &[
            "Elona Reach",
        ],
    },
    RunCategory {
        name: "Factions Rush",
        zones: &[
            "Kaineng Center",
            "Bukdek Byway",
            "The Marketplace",
            "Vizunah Square (Foreign Quarter)",
            "Vizinuh Square (Foreign Quarter)",
            "Vizunah Square (Local Quarter)",
            "Vizinuh Square (Local Quarter)",
            "Xaquang Skyway",
            "Senji's Corner",
            "Shenzun Tunnels",
            "Maatu Keep",
            "Pongmei Valley",
            "Zin Ku Corridor",
            "Sunjiang District",
            "Tahnnakai Temple",
            "Boreas Seabed (Mission)",
            "Zos Shivros Channel",
            "Boreas Seabed (Explorable)",
            "Boreas Seabed",
            "Mount Qinkai",
            "Breaker Hollow",
            "Aspenwood Gate (Luxon)",
            "Fort Aspenwood (Luxon)",
            "Fort Aspendwood (Luxon)",
            "Archipelagos",
            "Jade Flats (Luxon)",
            "The Jade Quarry (Luxon)",
            "Cavalon",
            "Maishang Hills",
            "Bai Paasu Reach",
            "Eradon Terrace",
            "Eredon Terrace",
            "Gyala Hatchery",
            "Gyala Haterchy",
            "Leviathan Pits",
            "Rhea's Crater",
            "The Aurios Mines",
            "Silent Surf",
            "Seafarer's Rest",
            "Unwaking Waters",
            "Harvest Temple",
            "Morostav Trail",
            "Durheim Archives",
            "Amatz Basin",
            "Mourning Veil Falls",
            "Vasburg Armory",
            "The Eternal Grove (Explorable)",
            "The Eternal Grove",
            "Lutgardis Conservatory",
            "Drazach Thicket",
            "Brauer Academy",
            "Melandru's Hope",
            "Jade Flats (Kurzick)",
            "Saint Anjeka's Shrine",
            "Ferndale",
            "Aspenwood Gate (Kurzick)",
            "House zu Heltzer",
            "Arborstone",
            "Altrumm Ruins",
            "Tanglewood Copse",
        ],
        exclude_explorables: &[
            "Sunjiang District",
        ],
    },
    RunCategory {
        name: "Bone Palace Run",
        zones: &[
            "Yohlon Haven",
            "Arkjok Ward",
            "Sunspear Sanctuary",
            "Turai's Procession",
            "Gate of Desolation",
            "The Sulfurous Wastes",
            "Sulfurous Wastes",
            "Remains of Sahlahja",
            "Joko's Domain",
            "Basalt Grotto",
            "The Shattered Ravines",
            "The Alkali Pan",
            "Bone Palace",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Kodash Run",
        zones: &[
            "Yohlon Haven",
            "Arkjok Ward",
            "Sunspear Sanctuary",
            "Turai's Procession",
            "Gate of Desolation",
            "The Sulfurous Wastes",
            "Sulfurous Wastes",
            "Remains of Sahlahja",
            "Joko's Domain",
            "Basalt Grotto",
            "Vehjin Mines",
            "Jennur's Horde",
            "Forum Highlands",
            "Tihark Orchard",
            "The Kodash Bazaar",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Vabbi Tour",
        zones: &[
            "The Kodash Bazaar",
            "Forum Highlands",
            "The Mirror of Lyss",
            "Mihanu Township",
            "Grand Court of Sebelkeh",
            "Dasha Vestibule",
            "Dzagonur Bastion",
            "Honur Hill",
            "Resplendent Makuun",
            "Yahnur Market",
            "Vehtendi Valley",
            "Yatendi Canyons",
            "Chantry of Secrets",
            "Wehhan Terraces",
            "Moddok Crevice",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Kourna Tour",
        zones: &[
            "Yohlon Haven",
            "Arkjok Ward",
            "Marga Coast",
            "Nundu Bay",
            "Dajkah Inlet",
            "Sunward Marches",
            "Venta Cemetery",
            "Venta Cemetary",
            "Jahai Bluffs",
            "Kodonur Crossroads",
            "Dejarin Estate",
            "Pogahn Passage",
            "Camp Hojanu",
            "The Floodplain of Mahnkelon",
            "Rilohn Refuge",
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Eye of the North Tour",
        zones: &[
            "Boreal Station",
            "Ice Cliff Chasms",
            "Eye of the North",
            "Norrhart Domains",
            "Gunnar's Hold",
            "Bjora Marches",
            "Longeye's Ledge",
            "Grothmar Wardowns",
            "Dalada Uplands",
            "Doomlore Shrine",
            "Drakkar Lake",
            "Sifhalla",
            "Varajar Fells",
            "Olafstead",
            "Verdant Cascades",
            "Umbral Grotto",
            "Vloxen Excavations",
            "Vloxen Excavations (Level 1)",
            "Vloxen Excavations: Level 1",
            "Vloxen Excavations: Level 2",
            "Vloxen Excavations: Level 3",
            "Vlox's Falls",
            "Arbor Bay",
            "Shards of Orr",
            "Shards of Orr: Level 1",
            "Shards of Orr: Level 2",
            "Shards of Orr: Level 3",
            "Gadd's Encampment",
            "Alcazia Tangle",
            "Rata Sum",
            "Riven Earth",
            "Tarnished Haven",
            "Magus Stones",
        ],
        exclude_explorables: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_13_categories_present() {
        assert_eq!(ALL_CATEGORIES.len(), 13);
    }

    #[test]
    fn test_droks_run_matching_and_summarize() {
        let droks_cat = ALL_CATEGORIES.iter().find(|c| c.name == "Droks Run").unwrap();

        // Map IDs: Beacon's Perch (133), Lornar's Pass (90), Dreadnought's Drift (97)
        let rec1 = RouteRecord {
            from_map_id: 133,
            run_map_id: 90,
            to_map_id: 97,
            run_count: 5,
            pb: Duration::from_secs(120),
            avg: Duration::from_secs(140),
        };
        // Unrelated map route
        let rec_unrelated = RouteRecord {
            from_map_id: 9999,
            run_map_id: 9998,
            to_map_id: 9997,
            run_count: 2,
            pb: Duration::from_secs(50),
            avg: Duration::from_secs(60),
        };

        let summary = droks_cat.summarize(&[rec1, rec_unrelated]);
        assert_eq!(summary.records.len(), 1);
        assert_eq!(summary.zone_count, 1);
        assert_eq!(summary.total_pb, Duration::from_secs(120));
        assert_eq!(summary.total_avg, Duration::from_secs(140));
    }

    #[test]
    fn test_exclusions() {
        let shiverpeaks = ALL_CATEGORIES.iter().find(|c| c.name == "Shiverpeaks Tour").unwrap();
        // Icedome (87) -> Frozen Forest (98) -> Iron Mines (24): SHOULD MATCH
        assert!(shiverpeaks.matches(87, 98, 24));
        // Iron Mines (24) -> Iron Mines (24) -> Droknar's Forge (20): EXCLUDED MISSION
        assert!(!shiverpeaks.matches(24, 24, 20));

        let desert = ALL_CATEGORIES.iter().find(|c| c.name == "Desert Tour").unwrap();
        // Destiny's Gorge (154) -> Skyward Reach (115) -> Elona Reach (118): SHOULD MATCH
        assert!(desert.matches(154, 115, 118));
        // Elona Reach mission (Through 118): EXCLUDED
        assert!(!desert.matches(118, 118, 154));

        let factions = ALL_CATEGORIES.iter().find(|c| c.name == "Factions Rush").unwrap();
        // Sunjiang District (220) as explorable run_map: EXCLUDED
        assert!(!factions.matches(218, 220, 222));

        let eotn = ALL_CATEGORIES.iter().find(|c| c.name == "Eye of the North Tour").unwrap();
        // Umbral Grotto (639) -> Vloxen Excavations (604) -> Vlox's Falls (624): SHOULD MATCH
        assert!(eotn.matches(639, 604, 624));
    }
}
