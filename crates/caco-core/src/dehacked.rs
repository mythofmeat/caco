//! What a DEHACKED patch says about which things are monsters, for
//! [`crate::wad_analysis`], which tells a map to play from a credits screen by
//! whether it has any.
//!
//! A patch can make a monster of a thing number the original game never used.
//! Cybernights' last map is ten Cyberdemons that its patch gives the numbers
//! 9000 to 9004. Read without the patch the map looked empty, was taken for
//! the credits, and the WAD would have counted as finished a map early. What
//! makes a thing a monster here is what makes the game count it in the kills
//! total: `COUNTKILL`.
//!
//! Read as nyan-doom reads a patch (`prboom2/src/deh/thing.c`). `Thing N` is
//! the game's Nth kind of thing, counting from 1. Its `ID #` is the number a map
//! places it by, its `Bits` are its flags as a number or as names, and a blank
//! line ends the block.

use std::collections::{HashMap, HashSet};

/// The original game's monsters: each one's `Thing` in a patch, and the number
/// a map places it by. All but the Lost Soul and the Icon of Sin count kills,
/// and those two are here because a map with nothing else on it is still one to
/// play.
const VANILLA_MONSTERS: &[(u32, u16)] = &[
    (2, 3004),  // Former Human
    (3, 9),     // Former Sergeant
    (4, 64),    // Arch-vile
    (6, 66),    // Revenant
    (9, 67),    // Mancubus
    (11, 65),   // Heavy Weapon Dude
    (12, 3001), // Imp
    (13, 3002), // Demon
    (14, 58),   // Spectre
    (15, 3005), // Cacodemon
    (16, 3003), // Baron of Hell
    (18, 69),   // Hell Knight
    (19, 3006), // Lost Soul
    (20, 7),    // Spider Mastermind
    (21, 68),   // Arachnotron
    (22, 16),   // Cyberdemon
    (23, 71),   // Pain Elemental
    (24, 84),   // Wolfenstein SS
    (25, 72),   // Commander Keen
    (26, 88),   // Boss Brain
];

/// The `Bits` flag that counts a thing in the kills total, bit 22 of the number.
const COUNTKILL: i64 = 0x40_0000;

/// The numbers a map places a monster by, once `patches` have been applied in
/// the order given.
///
/// These are always a superset of the original game's monsters: a vanilla
/// monster's number counts whatever a patch does to it. Patches that turn a
/// monster into scenery are rarer than patches that add one, and keeping the
/// vanilla numbers means this can only add monsters a patch defines.
pub fn monster_numbers(patches: &[String]) -> HashSet<u16> {
    // Each thing's number and whether it counts kills, for the vanilla
    // monsters and for every thing a patch changes.
    let mut things: HashMap<u32, (Option<u16>, bool)> = VANILLA_MONSTERS
        .iter()
        .map(|&(thing, number)| (thing, (Some(number), true)))
        .collect();
    for patch in patches {
        for (thing, field) in thing_fields(patch) {
            let (number, counts_kills) = things.entry(thing).or_insert((None, false));
            match field {
                Field::Number(n) => *number = n,
                Field::CountsKills(counts) => *counts_kills = counts,
            }
        }
    }

    let mut numbers: HashSet<u16> = VANILLA_MONSTERS.iter().map(|&(_, n)| n).collect();
    numbers.extend(
        things
            .into_values()
            .filter_map(|(number, counts_kills)| number.filter(|_| counts_kills)),
    );
    numbers
}

/// A field of a `Thing` block that decides whether it is a monster.
#[derive(Debug, PartialEq)]
enum Field {
    /// `ID #`: the number a map places it by, or none (`-1`).
    Number(Option<u16>),
    /// `Bits`: whether its flags include `COUNTKILL`.
    CountsKills(bool),
}

/// Each `ID #` and `Bits` that `patch` sets, with the `Thing` it is set on.
///
/// A block is a line naming it and its `key = value` lines, up to a blank line.
/// A line without `=` inside a block is skipped, as nyan-doom skips it, rather
/// than taken for the start of another block.
fn thing_fields(patch: &str) -> Vec<(u32, Field)> {
    let mut fields = Vec::new();
    // Inside a block, and the thing it is about if it is about one.
    let mut block: Option<Option<u32>> = None;
    for line in patch.lines() {
        let line = line.trim();
        if line.is_empty() {
            block = None;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            if block.is_none() {
                block = Some(thing_header(line));
            }
            continue;
        };
        let Some(Some(thing)) = block else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.eq_ignore_ascii_case("ID #") {
            let n = number(value).and_then(|n| u16::try_from(n).ok());
            fields.push((thing, Field::Number(n.filter(|&n| n != 0))));
        } else if key.eq_ignore_ascii_case("Bits") {
            let counts = match number(value) {
                Some(bits) => bits & COUNTKILL != 0,
                None => value
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .any(|name| name.eq_ignore_ascii_case("COUNTKILL")),
            };
            fields.push((thing, Field::CountsKills(counts)));
        }
    }
    fields
}

/// The thing a block's first line names, if it is a `Thing` block:
/// `Thing 151 (Cyberdemon (Deh_Actor_150))`.
fn thing_header(line: &str) -> Option<u32> {
    let mut words = line.split_whitespace();
    let kind = words.next()?;
    if !kind.eq_ignore_ascii_case("Thing") {
        return None;
    }
    words.next()?.parse().ok()
}

/// A value written as a number, in decimal or in hex after `0x`.
fn number(value: &str) -> Option<i64> {
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(patch: &str) -> HashSet<u16> {
        monster_numbers(&[patch.to_string()])
    }

    fn vanilla() -> HashSet<u16> {
        monster_numbers(&[])
    }

    #[test]
    fn test_every_vanilla_monster_is_one_with_no_patch() {
        let expected: HashSet<u16> = VANILLA_MONSTERS.iter().map(|&(_, n)| n).collect();
        assert_eq!(vanilla(), expected);
        assert_eq!(vanilla().len(), 20);
    }

    #[test]
    fn test_a_new_thing_that_counts_kills_is_a_monster() {
        // Cybernights' MAP29, as its patch has it: a DEHEXTRA thing given a number and the
        // Cyberdemon's bits, written as a number.
        let patch = "Patch File for DeHackEd v3.0\n\n\
            Thing 151 (Cyberdemon (Deh_Actor_150))\n\
            ID # = 9000\n\
            Hit points = 4000\n\
            Bits = 4194310\n\
            MBF21 Bits = 303208\n\n\
            Thing 156 (Cyberdemon (No Kill Count))\n\
            ID # = 9005\n\
            Bits = 6\n";

        let found = numbers(patch);

        assert!(found.contains(&9000));
        assert!(!found.contains(&9005), "it does not count kills");
        assert!(found.is_superset(&vanilla()));
    }

    #[test]
    fn test_bits_can_be_written_as_names() {
        let patch = "Thing 152\nID # = 9100\nBits = SOLID+SHOOTABLE+COUNTKILL\n\n\
            Thing 153\nID # = 9101\nBits = SOLID | shootable | countkill\n\n\
            Thing 154\nID # = 9102\nBits = SOLID+SHOOTABLE\n\n\
            Thing 155\nID # = 9103\nBits = 0x400006\n";

        let found = numbers(patch);

        assert!(found.contains(&9100));
        assert!(found.contains(&9101));
        assert!(!found.contains(&9102));
        assert!(found.contains(&9103));
    }

    #[test]
    fn test_a_vanilla_monster_given_a_new_number_is_found_by_it() {
        // The Imp's bits are its own, so it counts kills with no `Bits` line.
        let found = numbers("Thing 12 (Imp)\nID # = 9200\n");

        assert!(found.contains(&9200));
        assert!(found.contains(&3001), "the vanilla numbers stay");
    }

    #[test]
    fn test_a_thing_with_a_number_and_no_kills_is_not_a_monster() {
        // A DEHEXTRA thing has no flags until a patch gives it some.
        assert_eq!(numbers("Thing 160\nID # = 9300\n"), vanilla());
    }

    #[test]
    fn test_kills_and_a_number_can_come_from_different_patches() {
        let patches = [
            "Thing 170\nBits = COUNTKILL\n".to_string(),
            "Thing 170\nID # = 9400\n".to_string(),
        ];

        assert!(monster_numbers(&patches).contains(&9400));
    }

    #[test]
    fn test_a_later_patch_has_the_last_word() {
        let patches = [
            "Thing 171\nID # = 9500\nBits = COUNTKILL\n".to_string(),
            "Thing 171\nBits = SOLID\n".to_string(),
        ];

        assert!(!monster_numbers(&patches).contains(&9500));
    }

    #[test]
    fn test_only_a_thing_block_sets_a_thing() {
        // A Frame's fields, a top-level line, and a key after a block's blank line are none
        // of a thing's business.
        let patch = "Doom version = 21\nPatch format = 6\n\n\
            Frame 151\nBits = COUNTKILL\nID # = 9600\n\n\
            Thing 172\nID # = 9601\n\nBits = COUNTKILL\n";

        assert_eq!(numbers(patch), vanilla());
    }

    #[test]
    fn test_a_line_without_a_value_does_not_end_a_block() {
        let patch = "Thing 173 (Ghost)\n# a comment\nnot a field\nID # = 9700\nBits = COUNTKILL\n";

        assert!(numbers(patch).contains(&9700));
    }

    #[test]
    fn test_no_number_is_none() {
        let patch =
            "Thing 174\nID # = -1\nBits = COUNTKILL\n\nThing 175\nID # = 0\nBits = COUNTKILL\n";

        assert_eq!(numbers(patch), vanilla());
    }

    #[test]
    fn test_windows_line_endings() {
        let patch = "Thing 176\r\nID # = 9800\r\nBits = COUNTKILL\r\n\r\n";

        assert!(numbers(patch).contains(&9800));
    }
}
