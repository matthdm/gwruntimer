use std::time::Duration;
use crate::run_log::RouteRecord;

pub struct RunCategory {
    pub name: &'static str,
    pub zones: &'static [u32],
    pub exclude_explorables: &'static [u32],
}

impl RunCategory {
    #[inline]
    pub fn matches(&self, from_map_id: u32, run_map_id: u32, to_map_id: u32) -> bool {
        if self.exclude_explorables.contains(&run_map_id) {
            return false;
        }
        self.zones.contains(&from_map_id)
            && self.zones.contains(&run_map_id)
            && self.zones.contains(&to_map_id)
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

#[derive(Clone, Debug, Default)]
pub struct CategorySummary {
    pub records: Vec<RouteRecord>,
    pub total_pb: Duration,
    pub total_avg: Duration,
    pub zone_count: usize,
}

pub static ALL_CATEGORIES: &[RunCategory] = &[
    RunCategory {
        name: "Ascalon",
        zones: &[
            81, 148, 811, // Ascalon City
            33,           // Old Ascalon
            131,          // Serenity Temple
            107,          // Eastern Frontier
            104,          // Pockmark Flats
            29, 166,      // Fort Ranik
            101, 162,     // Regent Valley
            30,           // Ruins of Surmia
            135,          // Frontier Gate
            28,           // The Great Northern Wall
            39,           // Sardelac Sanitarium
            102,          // The Breach
            40, 779,      // Piken Square
            13,           // Diessa Lowlands
            36, 729,      // Grendich Courthouse
            32,           // Nolani Academy
            103,          // Ascalon Foothills
            54,           // Scoundrel's Rise
            14,           // Gates of Kryta
            58,           // North Kryta Province
            55, 385, 808, 809, 810, 844, // Lion's Arch
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Northern Shiverpeaks",
        zones: &[
            99,           // Traveler's Vale
            134,          // Yak's Bend
            25,           // Borlis Pass
            88,           // Iron Horse Mine
            89,           // Anvil Rock
            132,          // Ice Tooth Cave
            21,           // The Frost Gate
            100,          // Deldrimor Bowl
            133,          // Beacon's Perch
            27,           // Griffon's Mouth
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Kryta",
        zones: &[
            55, 385, 808, 809, 810, 844, // Lion's Arch
            15, 841,      // D'Alessio Seaboard
            58,           // North Kryta Province
            59,           // Nebo Terrace
            57,           // Bergen Hot Springs
            136, 846, 850,// Beetletun
            56,           // Cursed Lands
            18,           // The Black Curtain
            138, 788,     // Temple of the Ages
            62, 849, 853, // Watchtower Coast
            16, 839,      // Divinity Coast
            17, 837,      // Talmark Wilderness
            53,           // Tears of the Fallen
            63,           // Stingray Strand
            137,          // Fishermen's Haven
            19,           // Sanctum Cay
            61,           // Twin Serpent Lakes
            73, 843,      // Riverside Province
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Maguuma",
        zones: &[
            60, 848, 852, // Majesty's Rest
            41,           // Sage Lands
            140,          // Druid's Overlook
            11,           // The Wilds / The Wilderness
            42,           // Mamnoon Lagoon
            10, 471,      // Bloodstone Fen
            43,           // Silverwood
            142,          // Quarrel Falls
            44,           // Ettin's Back
            139,          // Ventari's Refuge
            12, 847, 851, // Aurora Glade
            47,           // Dry Top
            49,           // Henge of Denravi
            48,           // Tangle Root
            141,          // Maguuma Stade
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Southern Shiverpeaks",
        zones: &[
            133,          // Beacon's Perch
            90,           // Lornar's Pass
            97,           // Dreadnought's Drift
            91,           // Snake Dance
            155,          // Camp Rankor
            26,           // Talus Chute
            20, 783, 812, 813, // Droknar's Forge
            87,           // Icedome
            98,           // Frozen Forest
            24,           // Iron Mines of Moladune
            23,           // Thunderhead Keep
            94,           // Ice Floe
            157,          // Marhan's Grotto
            95,           // Witman's Folly
            158,          // Port Sledge
            155,          // Camp Rankor
            91,           // Snake Dance
            191,          // Grenth's Footprint
            206,          // Deldrimor War Camp
            93,           // Spearhead Peak
            156,          // The Granite Citadel
            159,          // Copperhammer Mines
        ],
        exclude_explorables: &[
            24,           // Iron Mines of Moladune
        ],
    },
    RunCategory {
        name: "Crystal Desert",
        zones: &[
            109,          // The Amnoon Oasis
            113,          // Prophet's Path
            152,          // Heroes' Audience
            38, 119,      // Augury Rock
            111,          // Vulture Drifts
            116,          // Dunes of Despair
            115,          // Skyward Reach
            154,          // Destiny's Gorge
            110,          // Diviner's Ascent
            118,          // Elona Reach
            114,          // Salt Flats
            153,          // Seeker's Passage
            108,          // The Scar
            117,          // Thirsty River
            112,          // The Arid Sea
        ],
        exclude_explorables: &[
            118,          // Elona Reach
        ],
    },
    RunCategory {
        name: "Shing Jea",
        zones: &[
            242, 505, 815, 816, 871, // Shing Jea Monastery
            238,                     // Sunqua Vale
            249, 509, 511, 864,      // Tsumei Village
            235,                     // Panjiang Peninsula
            236,                     // Kinya Province
            251,                     // Ran Musu Gardens
            214, 245, 257, 512,      // Minister Cho's Estate
            252,                     // Linnok Courtyard
            196, 507,                // Jaya Bluffs
            313,                     // Saoshang Trail
            250, 508, 510, 865,      // Seitung Harbor
            213, 246, 258,           // Zen Daijun
            237, 506,                // Haiju Lagoon
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Cantha",
        zones: &[
            194, 400, 817, 861, 872, // Kaineng Center
            225, 233, 270,           // Raisu Palace
            240, 863,                // Bukdek Byway
            303, 774, 862,           // The Marketplace
            292, 305,                // Vizunah Square (Foreign Quarter)
            291, 304,                // Vizunah Square (Local Quarter)
            215,                     // Vizunah Square
            31,                      // Xaquang Skyway
            51,                      // Senji's Corner
            197,                     // Shenzun Tunnels
            283,                     // Maatu Keep
            211,                     // Pongmei Valley
            284,                     // Zin Ku Corridor
            220, 256, 264,           // Sunjiang District
            217, 261, 269, 866,      // Tahnnakai Temple
        ],   
        exclude_explorables: &[            
            220, 256, 264,           // Sunjiang District
        ],
    },
    RunCategory {
        name: "The Jade Sea",
        zones: &[
            219, 247, 263,// Boreas Seabed
            273,          // Zos Shivros Channel
            200,          // Mount Qinkai
            278,          // Breaker Hollow
            389,          // Aspenwood Gate (Luxon)
            221, 293, 324,// Fort Aspenwood (Luxon)
            198,          // Archipelagos
            391,          // Jade Flats (Luxon)
            223, 295, 306,// The Jade Quarry (Luxon)
            193,          // Cavalon
            199,          // Maishang Hills
            288,          // Bai Paasu Reach
            350,          // Eredon Terrace
            144, 224, 253, 268, // Gyala Hatchery
            279,          // Leviathan Pits
            202,          // Rhea's Crater
            234,          // The Aurios Mines
            203,          // Silent Surf
            289,          // Seafarer's Rest
            204, 227, 297,// Unwaking Waters (Luxon)
            277,          // Harvest Temple
        ],   
        exclude_explorables: &[],
    },
    RunCategory {
        name: "The Echovald Forest",
        zones: &[
            277,          // Harvest Temple
            204, 227, 298,// Unwaking Waters (Kurzick)
            205,          // Morostav Trail
            287, 291,     // Durheim Archives
            230,          // Amatz Basin
            209,          // Mourning Veil Falls
            130,          // Vasburg Armory
            128, 222, 267,// The Eternal Grove
            129,          // Lutgardis Conservatory
            195,          // Drazach Thicket
            286,          // Brauer Academy
            201,          // Melandru's Hope
            390,          // Jade Flats (Kurzick)
            349,          // Saint Anjeka's Shrine
            210,          // Ferndale
            388,          // Aspenwood Gate (Kurzick)
            77,           // House zu Heltzer
            218, 244, 262,// Arborstone
            272, 870,     // Altrumm Ruins
            348,          // Tanglewood Copse
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Istan",
        zones: &[
            370, 422, 449, 818, 819, 820, // Kamadan, Jewel of Istan
            456,          // Churrhir Fields
            544, 556,     // Chahbek Village
            430,          // Plains of Jarin
            431,          // Sunspear Great Hall
            502,          // The Astralarium
            483,          // Zehlon Reach
            479,          // Champion's Dawn
            432, 447,     // Cliffs of Dohjok
            491, 515,     // Jokanur Diggings
            492, 516,     // Blacktide Den
            457, 487,     // Beknur Harbor
            486,          // Issnur Isles
            489,          // Kodlonu Hamlet
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Kourna",
        zones: &[
            381,          // Yohlon Haven
            380,          // Arkjok Ward
            387,          // Sunspear Sanctuary
            371,          // Marga Coast
            477, 523,     // Nundu Bay
            554,          // Dajkah Inlet
            373,          // Sunward Marches
            421,          // Venta Cemetery
            369,          // Jahai Bluffs
            424, 458,     // Kodonur Crossroads
            379,          // Dejarin Estate
            426, 460,     // Pogahn Passage
            376,          // Camp Hojanu
            384,          // The Floodplain of Mahnkelon
            425, 459,     // Rilohn Refuge
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Desolation",
        zones: &[
            478,          // Gate of Desolation
            444,          // The Sulfurous Wastes
            545,          // Remains of Sahlahja
            437,          // Joko's Domain
            398,          // Basalt Grotto
            446,          // The Alkali Pan
            441,          // The Shattered Ravines
            442,          // Lair of the Forgotten
            443,          // Poisoned Outcrops
            439,          // The Ruptured Heart
            480, 525,     // Ruins of Morah
            438,          // Bone Palace
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Vabbi",
        zones: &[
            398,          // Basalt Grotto
            397,          // Vehjin Mines
            476, 522,     // Jennur's Horde
            399,          // Forum Highlands
            428, 518,     // Tihark Orchard
            414,          // The Kodash Bazaar
            419,          // The Mirror of Lyss
            396,          // Mihanu Township
            435, 521,     // Grand Court of Sebelkeh
            434,          // Dasha Vestibule
            433, 519,     // Dzagonur Bastion
            403,          // Honur Hill
            402,          // Resplendent Makuun
            407,          // Yahnur Market
            406,          // Vehtendi Valley
            392,          // Yatendi Canyons
            393,          // Chantry of Secrets
            378,          // Wehhan Terraces
            427,          // Moddok Crevice
        ],
        exclude_explorables: &[],
    },
    RunCategory {
        name: "Eye of the North",
        zones: &[
            675,          // Boreal Station
            499, 793,     // Ice Cliff Chasms
            642, 821,     // Eye of the North
            548,          // Norrhart Domains
            644,          // Gunnar's Hold
            482,          // Bjora Marches
            650,          // Longeye's Ledge
            649,          // Grothmar Wardowns
            647,          // Dalada Uplands
            648,          // Doomlore Shrine
            513,          // Drakkar Lake
            643,          // Sifhalla
            553, 708,     // Varajar Fells
            645, 854,     // Olafstead
            566,          // Verdant Cascades
            639,          // Umbral Grotto
            604, 605, 606,// Vloxen Excavations
            624,          // Vlox's Falls
            485,          // Arbor Bay
            581, 582, 583,// Shards of Orr
            638,          // Gadd's Encampment
            572,          // Alcazia Tangle
            640,          // Rata Sum
            501,          // Riven Earth
            641,          // Tarnished Haven
            569,          // Magus Stones
        ],
        exclude_explorables: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_15_categories_present() {
        assert_eq!(ALL_CATEGORIES.len(), 15);
    }

    #[test]
    fn test_southern_shiverpeaks_matching_and_summarize() {
        let shiverpeaks = ALL_CATEGORIES.iter().find(|c| c.name == "Southern Shiverpeaks").unwrap();

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

        let summary = shiverpeaks.summarize(&[rec1, rec_unrelated]);
        assert_eq!(summary.records.len(), 1);
        assert_eq!(summary.zone_count, 1);
        assert_eq!(summary.total_pb, Duration::from_secs(120));
        assert_eq!(summary.total_avg, Duration::from_secs(140));
    }

    #[test]
    fn test_exclusions() {
        let shiverpeaks = ALL_CATEGORIES.iter().find(|c| c.name == "Southern Shiverpeaks").unwrap();
        // Icedome (87) -> Frozen Forest (98) -> Droknar's Forge (20): SHOULD MATCH
        assert!(shiverpeaks.matches(87, 98, 20));
        // Iron Mines (24) as run_map_id: EXCLUDED
        assert!(!shiverpeaks.matches(87, 24, 20));

        let desert = ALL_CATEGORIES.iter().find(|c| c.name == "Crystal Desert").unwrap();
        // Destiny's Gorge (154) -> Skyward Reach (115) -> Augury Rock (119): SHOULD MATCH
        assert!(desert.matches(154, 115, 119));
        // Elona Reach (118) as run_map_id: EXCLUDED
        assert!(!desert.matches(154, 118, 119));

        let cantha = ALL_CATEGORIES.iter().find(|c| c.name == "Cantha").unwrap();
        // Sunjiang District (220) as run_map_id: EXCLUDED
        assert!(!cantha.matches(194, 220, 217));

        let eotn = ALL_CATEGORIES.iter().find(|c| c.name == "Eye of the North").unwrap();
        // Umbral Grotto (639) -> Vloxen Excavations (604) -> Vlox's Falls (624): SHOULD MATCH
        assert!(eotn.matches(639, 604, 624));

        let deso = ALL_CATEGORIES.iter().find(|c| c.name == "Desolation").unwrap();
        // Basalt Grotto (398) -> The Alkali Pan (446) -> Lair of the Forgotten (442): SHOULD MATCH
        assert!(deso.matches(398, 446, 442));
    }
}
