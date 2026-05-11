use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::models::ForumThread;

// =============================================================================
// Complevel Detection
// =============================================================================
//
// Multiple patterns can match a post. We score each hit by how specific the
// match is — a literal `complevel 21` beats a loose mention of "Vanilla+" —
// and return the strongest signal. First-match-wins (the previous behavior)
// regularly flipped MBF21 wads to cl2 because the word "vanilla" shows up in
// mod names, flavour text, and user chat.

/// Numeric `complevel N` / `cl-N` / `-complevel N` forms. Captures N.
static CL_NUMERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:-?complevel|cl)\s*[-:]?\s*(\d{1,2})\b").unwrap());
/// `mbf21`, `mbf-21`, `mbf 21`. Also covers the `[MBF21]` title-tag form.
static CL_MBF21_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bmbf[-\s]?21\b").unwrap());
/// `DSDHacked` / `DSDHACKED` — implies MBF21 feature set.
static CL_DSDHACKED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bdsdhacked\b").unwrap());
/// `boom compatible`, `boom-compatible`, `boom format`, `boom compat`.
static CL_BOOM_COMPAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bboom[- ](?:compat(?:ible|ibility)?|format)\b").unwrap());
/// `mbf compat(ible)` — the `[- ]` separator prevents this from matching
/// `mbf21` (no whitespace/hyphen between `mbf` and `21`).
static CL_MBF_COMPAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bmbf[- ](?:compat(?:ible|ibility)?|format)\b").unwrap());
/// `vanilla compat(ible)`, `vanilla doom`, `vanilla-compat`. Intentionally
/// tight — bare "vanilla" matches too many mod names ("Vanilla Essence",
/// "Vanilla+ goodness").
static CL_VANILLA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bvanilla[- ](?:compat(?:ible|ibility)?|doom|format)\b").unwrap()
});
/// `limit-removing`, `limit removing`. Conservative: requires the full phrase.
static CL_LIMIT_REMOVING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\blimit[- ]removing\b").unwrap());
/// Chocolate Doom is a specific port → cl2.
static CL_CHOCOLATE_DOOM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bchocolate[- ]?doom\b").unwrap());

/// Confidence-scored complevel detection. Returns the highest-confidence
/// complevel found in `text`, or `None` if no signal is present.
fn extract_complevel(text: &str) -> Option<i32> {
    let mut best: Option<(u8, i32)> = None;
    let mut push = |confidence: u8, cl: i32| {
        if best.map(|(c, _)| confidence > c).unwrap_or(true) {
            best = Some((confidence, cl));
        }
    };

    // 10 — explicit numeric form. Highest trust.
    for caps in CL_NUMERIC_RE.captures_iter(text) {
        if let Some(n) = caps.get(1).and_then(|m| m.as_str().parse::<i32>().ok())
            && (0..=21).contains(&n)
        {
            push(10, n);
        }
    }
    // 9 — explicit `MBF21` / `DSDHacked` literal.
    if CL_MBF21_RE.is_match(text) || CL_DSDHACKED_RE.is_match(text) {
        push(9, 21);
    }
    // 8 — `boom compatible` / `mbf compatible` with explicit qualifier.
    if CL_BOOM_COMPAT_RE.is_match(text) {
        push(8, 9);
    }
    if CL_MBF_COMPAT_RE.is_match(text) {
        push(8, 11);
    }
    // 6 — vanilla / limit-removing with qualifier, or Chocolate Doom.
    if CL_VANILLA_RE.is_match(text)
        || CL_LIMIT_REMOVING_RE.is_match(text)
        || CL_CHOCOLATE_DOOM_RE.is_match(text)
    {
        push(6, 2);
    }

    best.map(|(_, cl)| cl)
}

// =============================================================================
// IWAD Detection
// =============================================================================
//
// Previously a bare `\bheretic\b` was enough to match — that misfired on
// "H - Heretic/Hexen/Hexen 2" appearing in a texture-source legend. The
// non-Doom IWADs now require explicit context ("for heretic", "heretic.wad")
// and a `IWAD: X` label beats any loose match, which also lets us correctly
// handle negated labels like `IWAD: Plutonia, not Doom 2` (the label stops at
// the comma, so Plutonia wins over the Doom 2 mentioned after it).

/// Label form: `IWAD: X`, `IWAD required: X`, `IWAD needed: X`.
/// The capture excludes commas/parens/semicolons so "IWAD: Plutonia, not
/// Doom 2" captures just "Plutonia".
static EXPLICIT_IWAD_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\biwad\s*(?:required|needed)?\s*[:=]\s*([^\n.,;()]{2,40})").unwrap()
});

/// Context-required patterns — run after label detection fails.
static IWAD_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // Doom II — common enough to allow loose mentions.
        (Regex::new(r"(?i)\bdoom\s*(?:ii|2)\b").unwrap(), "doom2"),
        (Regex::new(r"(?i)\bdoom2\.wad\b").unwrap(), "doom2"),
        // Final Doom (TNT)
        (Regex::new(r"(?i)\btnt\.wad\b").unwrap(), "tnt"),
        (Regex::new(r"(?i)\btnt[:\s]+evilution\b").unwrap(), "tnt"),
        (Regex::new(r"(?i)\bevilution\b").unwrap(), "tnt"),
        // Plutonia
        (
            Regex::new(r"(?i)\bpluton(?:ia)?\.wad\b").unwrap(),
            "plutonia",
        ),
        (
            Regex::new(r"(?i)\bpluton(?:ia)?\s*(?:experiment)?\b").unwrap(),
            "plutonia",
        ),
        (Regex::new(r"(?i)\bfinal\s*doom\b").unwrap(), "finaldoom"),
        // Ultimate Doom / Doom 1
        (Regex::new(r"(?i)\bultimate\s*doom\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bdoom\.wad\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bdoom\s*1\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bfor\s+doom\s*1?\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\brequires?\s+doom\b").unwrap(), "doom"),
        // Heretic / Hexen / Strife / Chex / FreeDoom — require explicit
        // context so "Heretic/Hexen" in a texture-credit line can't hijack
        // the detector.
        (Regex::new(r"(?i)\bheretic\.wad\b").unwrap(), "heretic"),
        (
            Regex::new(r"(?i)\b(?:for|requires?|needs?|using|on)\s+heretic\b").unwrap(),
            "heretic",
        ),
        (Regex::new(r"(?i)\bhexen\.wad\b").unwrap(), "hexen"),
        (
            Regex::new(r"(?i)\b(?:for|requires?|needs?|using|on)\s+hexen\b").unwrap(),
            "hexen",
        ),
        (Regex::new(r"(?i)\bstrife\.wad\b").unwrap(), "strife"),
        (
            Regex::new(r"(?i)\b(?:for|requires?|needs?|using|on)\s+strife\b").unwrap(),
            "strife",
        ),
        (
            Regex::new(r"(?i)\b(?:for|requires?|needs?|using)\s+chex\s*quest\b").unwrap(),
            "chex",
        ),
        (
            Regex::new(r"(?i)\bfreedoom[12]?\.wad\b").unwrap(),
            "freedoom",
        ),
        (
            Regex::new(r"(?i)\b(?:for|requires?|needs?|using)\s+freedoom\b").unwrap(),
            "freedoom",
        ),
    ]
});

/// Resolve a label-captured phrase (e.g. "Plutonia" or "Doom 2") to a
/// canonical IWAD short name. Label context is strong enough that bare
/// game-name matches are acceptable here.
fn iwad_from_label_snippet(snippet: &str) -> Option<&'static str> {
    let lower = snippet.to_lowercase();
    // Order matters — check more-specific matches first.
    if lower.contains("plutonia") {
        return Some("plutonia");
    }
    if lower.contains("tnt") || lower.contains("evilution") {
        return Some("tnt");
    }
    if lower.contains("ultimate doom") {
        return Some("doom");
    }
    if lower.contains("doom 2")
        || lower.contains("doom ii")
        || lower.contains("doom2")
        || lower.contains("doom2.wad")
    {
        return Some("doom2");
    }
    if lower.contains("doom 1") || lower.contains("doom1") {
        return Some("doom");
    }
    if lower.contains("heretic") {
        return Some("heretic");
    }
    if lower.contains("hexen") {
        return Some("hexen");
    }
    if lower.contains("strife") {
        return Some("strife");
    }
    if lower.contains("chex") {
        return Some("chex");
    }
    if lower.contains("freedoom") {
        return Some("freedoom");
    }
    if lower.contains("doom") {
        return Some("doom");
    }
    None
}

/// Extract the required IWAD. Explicit `IWAD: X` labels win; otherwise fall
/// back to context-gated patterns.
fn extract_iwad(text: &str) -> Option<&'static str> {
    if let Some(caps) = EXPLICIT_IWAD_LABEL_RE.captures(text)
        && let Some(snippet) = caps.get(1)
        && let Some(name) = iwad_from_label_snippet(snippet.as_str())
    {
        return Some(name);
    }
    for (re, iwad) in IWAD_PATTERNS.iter() {
        if re.is_match(text) {
            return Some(iwad);
        }
    }
    None
}

// =============================================================================
// Sourceport Detection
// =============================================================================

/// (compiled regex, normalized sourceport name)
static SOURCEPORT_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // GZDoom family
        (Regex::new(r"(?i)\bgzdoom\b").unwrap(), "gzdoom"),
        (Regex::new(r"(?i)\blzdoom\b").unwrap(), "lzdoom"),
        (Regex::new(r"(?i)\bvkdoom\b").unwrap(), "vkdoom"),
        (Regex::new(r"(?i)\bqzdoom\b").unwrap(), "qzdoom"),
        (Regex::new(r"(?i)\bzdoom\b").unwrap(), "zdoom"),
        // DSDA-Doom / PrBoom family. Bare `DSDA` (without `-Doom` suffix)
        // shows up in "Tested with: DSDA, Cherry, Woof" style lists.
        (
            Regex::new(r"(?i)\bdsda(?:[- ]?doom)?\b").unwrap(),
            "dsda-doom",
        ),
        (Regex::new(r"(?i)\bprboom\+?\b").unwrap(), "prboom+"),
        (Regex::new(r"(?i)\bglboom\+?\b").unwrap(), "glboom+"),
        (Regex::new(r"(?i)\bumapinfo\b").unwrap(), "dsda-doom"),
        // Cherry Doom (Woof/Nugget fork)
        (
            Regex::new(r"(?i)\bcherry[- ]?doom\b").unwrap(),
            "cherry-doom",
        ),
        // Eternity
        (
            Regex::new(r"(?i)\beternity\s*(?:engine)?\b").unwrap(),
            "eternity",
        ),
        // Crispy Doom
        (Regex::new(r"(?i)\bcrispy\s*doom\b").unwrap(), "crispy-doom"),
        // Chocolate Doom
        (
            Regex::new(r"(?i)\bchocolate\s*doom\b").unwrap(),
            "chocolate-doom",
        ),
        // Woof!
        (Regex::new(r"(?i)\bwoof!?\b").unwrap(), "woof"),
        // Nugget Doom
        (Regex::new(r"(?i)\bnugget\s*doom\b").unwrap(), "nugget-doom"),
        // EDGE
        (Regex::new(r"(?i)\bedge(?:-classic)?\b").unwrap(), "edge"),
        // Doomsday
        (Regex::new(r"(?i)\bdoomsday\b").unwrap(), "doomsday"),
        // Zandronum
        (Regex::new(r"(?i)\bzandronum\b").unwrap(), "zandronum"),
        // Odamex
        (Regex::new(r"(?i)\bodamex\b").unwrap(), "odamex"),
        // 3DGE
        (Regex::new(r"(?i)\b3dge\b").unwrap(), "3dge"),
        // Limit-removing (generic)
        (
            Regex::new(r"(?i)\blimit[- ]?removing\b").unwrap(),
            "limit-removing",
        ),
    ]
});

// Requirement phrases intentionally exclude "tested with" / "works in" style
// compatibility notes because Doomworld posts often mention the tester's port
// without making it mandatory.
const STRICT_PORT_PREFIXES: &[&str] = &[
    "requires",
    "require",
    "needs",
    "need",
    "must use",
    "must run",
    "only runs",
    "only run",
    "only compatible",
    "compatible only",
    "designed for",
    "built for",
    "made for",
    "specifically for",
    "intended port",
    "target port",
    "primary port",
    "source port:",
    "sourceport:",
    "port:",
];
const STRICT_PORT_SUFFIXES: &[&str] = &[
    "required",
    "is required",
    "needed",
    "is needed",
    "only",
    "compatible only",
];
const WEAK_COMPAT_INTENT: &[&str] = &[
    "tested with",
    "tested in",
    "tested on",
    "test with",
    "tested port",
    "tested source port",
    "works with",
    "works in",
    "plays in",
    "plays on",
    "compatible with",
    "runs on",
    "runs in",
    "run with",
];
const NON_REQUIREMENT_LABELS: &[&str] = &[
    "recommended port",
    "recommended source port",
    "preferred port",
    "preferred source port",
    "tested port",
    "tested source port",
    "port of choice",
];

/// Patterns that capture an explicit port declaration. The capture group is a
/// short phrase (up to the first comma/period/semicolon) that we then re-scan
/// with SOURCEPORT_PATTERNS to normalize to a canonical port name. Multiple
/// ports in a single capture (e.g. "GZDoom or VKDoom") all get boosted.
static EXPLICIT_PORT_LABEL_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(
            r"(?i)\b(?:intended|target|primary)\s+(?:source\s+)?port\s*[:=-]\s*([^\n.,;()]{2,60})",
        )
        .unwrap(),
        Regex::new(r"(?i)\b(?:source\s*)?port\s*[:=]\s*([A-Za-z][^\n.,;()]{1,60})").unwrap(),
        Regex::new(
            r"(?i)\b(?:advanced\s+)?engine\s+(?:needed|required)\s*[:=]\s*([^\n.,;()]{2,60})",
        )
        .unwrap(),
        Regex::new(
            r"(?i)\b(?:designed|built|made|specifically)\s+for\s+([A-Za-z][^\n.,;()]{2,40})",
        )
        .unwrap(),
    ]
});

/// Prefixes that signal the whole line is a negation — ports mentioned here
/// are being *excluded*, not recommended.
const NEGATION_PREFIXES: &[&str] = &[
    "may not",
    "does not",
    "do not",
    "doesn't",
    "don't",
    "won't",
    "will not",
    "cannot",
    "can't",
    "incompatible",
    "not compatible",
    "not supported",
];

/// Substrings anywhere in the line that signal a negation about a port.
/// These handle mid-sentence cases the prefix check misses, e.g.
/// "tested in dsda-doom // impossible on zdoom ports".
const NEGATION_PHRASES: &[&str] = &[
    "impossible on",
    "impossible in",
    "incompatible with",
    "doesn't work on",
    "doesn't work in",
    "does not work on",
    "does not work in",
    "won't run on",
    "won't run in",
    "will not run on",
    "will not run in",
    "crashes on",
    "crashes in",
    "not supported on",
    "not supported in",
    "may not run",
    "not tested on",
    "not tested in",
];

/// Extract the required sourceport by scoring every mention with requirement
/// signals in its surrounding line. Lines like
/// `FOR GZDOOM USERS: ...` get demoted as caveats, while
/// `TESTED WITH: GZDoom` is ignored because it does not state a strict
/// requirement.
fn extract_sourceport(text: &str) -> Option<&'static str> {
    // Preserve SOURCEPORT_PATTERNS order for deterministic tie-breaking.
    let mut scores: Vec<(&'static str, i32)> = Vec::new();

    // First pass: explicit requirement-style labels. These get the strongest
    // boost because the author is telling us directly. A single capture may
    // list multiple ports ("GZDoom or VKDoom") — boost every one.
    let explicit: std::collections::HashSet<&'static str> = EXPLICIT_PORT_LABEL_RE
        .iter()
        .flat_map(|re| re.captures_iter(text))
        .filter_map(|c| {
            let whole = c.get(0)?;
            let line = enclosing_line(text, whole.start(), whole.end()).to_lowercase();
            if is_non_requirement_sourceport_line(&line) {
                None
            } else {
                c.get(1).map(|m| m.as_str().to_string())
            }
        })
        .flat_map(|snippet| {
            SOURCEPORT_PATTERNS
                .iter()
                .filter_map(|(re, name)| re.is_match(&snippet).then_some(*name))
                .collect::<Vec<_>>()
        })
        .collect();

    for (re, name) in SOURCEPORT_PATTERNS.iter() {
        let mut best_for_port: Option<i32> = None;
        for m in re.find_iter(text) {
            let line = enclosing_line(text, m.start(), m.end());
            let line_lower = line.to_lowercase();
            // Line-prefix negation applies to every port on the line
            // ("May Not Run With: PRBoom+").
            let line_negated = NEGATION_PREFIXES
                .iter()
                .any(|p| line_lower.trim_start().starts_with(p));
            // Mention-proximity negation handles mid-line cases where only
            // THIS port is being excluded — e.g. "tested in dsda-doom //
            // impossible on zdoom ports": the phrase immediately precedes
            // the `zdoom` mention, so only zdoom gets demoted.
            let window_start = m.start().saturating_sub(50);
            let before_lower = text[window_start..m.start()].to_lowercase();
            let mention_negated = NEGATION_PHRASES.iter().any(|p| before_lower.contains(p));
            let negated = line_negated || mention_negated;
            let mut score: i32 = 0;

            if !negated && !is_non_requirement_sourceport_line(&line_lower) {
                if mention_has_strict_requirement_context(text, m.start(), m.end()) {
                    score += 12;
                }
            } else {
                // "May not run with X" / "impossible on X" — port is being
                // excluded, not recommended. Drop out of contention.
                score -= 6;
            }

            // Caveat demotion: "for X users:" / "if you use X" / "X users:".
            if is_caveat_line(&line_lower, name) {
                score -= 8;
            }

            best_for_port = Some(best_for_port.map_or(score, |b| b.max(score)));
        }
        if let Some(mut s) = best_for_port {
            if explicit.contains(name) {
                s += 20;
            }
            scores.push((name, s));
        }
    }

    // Highest score wins; ties broken by first appearance in SOURCEPORT_PATTERNS
    // (`max_by_key` returns the LAST maximum, so we fold manually with strict
    // `>` to keep the earlier entry).
    let mut best: Option<(&'static str, i32)> = None;
    for (name, s) in scores {
        if s <= 0 {
            continue;
        }
        if best.map(|(_, b)| s > b).unwrap_or(true) {
            best = Some((name, s));
        }
    }
    best.map(|(n, _)| n)
}

fn is_non_requirement_sourceport_line(line_lower: &str) -> bool {
    NON_REQUIREMENT_LABELS
        .iter()
        .any(|k| line_lower.contains(k))
}

fn mention_has_strict_requirement_context(text: &str, start: usize, end: usize) -> bool {
    let before_start = start.saturating_sub(50);
    let after_end = (end + 50).min(text.len());
    let before_lower = text[before_start..start].to_lowercase();
    let after_lower = text[end..after_end].to_lowercase();
    let after_trimmed = after_lower.trim_start();
    let strict_prefix = last_index_of_any(&before_lower, STRICT_PORT_PREFIXES);
    let weak_prefix = last_index_of_any(&before_lower, WEAK_COMPAT_INTENT);
    let has_strict_prefix = strict_prefix.is_some_and(|s| weak_prefix.map_or(true, |w| s > w));
    let suffix_negated = ["not required", "not needed", "not mandatory"]
        .iter()
        .any(|p| after_lower.contains(p));

    has_strict_prefix
        || (!suffix_negated
            && (STRICT_PORT_SUFFIXES
                .iter()
                .any(|p| after_trimmed.starts_with(p))
                || ["required", "needed"]
                    .iter()
                    .any(|p| after_lower.contains(p))))
}

fn last_index_of_any(haystack: &str, needles: &[&str]) -> Option<usize> {
    needles
        .iter()
        .filter_map(|needle| haystack.rfind(needle))
        .max()
}

fn enclosing_line(text: &str, start: usize, end: usize) -> &str {
    let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[end..]
        .find('\n')
        .map(|i| end + i)
        .unwrap_or(text.len());
    &text[line_start..line_end]
}

/// Clean a Doomworld thread subject so it reads like a WAD name rather than
/// a release announcement. Runs four passes:
///   1. Strip all `[...]` groups whose content is technical (MBF21, RC2, -cl21, v1.0, …).
///   2. Strip a trailing `(...)` group when it's just a version/hype parenthetical.
///   3. If a separator (` - `, ` | `, `: `, ` // `) splits the title, drop the
///      tail when it reads as metadata ("32 Maps", "for GZDoom",
///      "a vanilla episode", "Now on Idgames!").
///   4. Strip trailing hype words (RELEASED, VERSION, UPDATED, NOW, …).
fn strip_tech_brackets(title: &str) -> String {
    let mut t = title.trim().to_string();
    t = strip_all_tech_brackets(&t);
    t = strip_trailing_noise_parens(&t);
    t = strip_metadata_tail(&t);
    t = strip_trailing_hype_words(&t);
    // Collapse any double-spaces we may have left behind.
    static MULTI_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
    MULTI_WS.replace_all(t.trim(), " ").into_owned()
}

static BRACKET_GROUP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[\]]*\]").unwrap());

/// Regex for tech-tag content inside brackets that isn't covered by the
/// TITLE_TECH_TAGS word list (short tokens with digits that are unsafe to
/// substring-match against the whole title: `RC2`, `-cl21`, `v1.0`, …).
static BRACKET_TECH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b-?(?:rc|cl|complevel)\s*-?\s*\d|\bv\d+(?:\.\d+)+[a-z]?\b|\b(?:released|updated?|version)\b",
    )
    .unwrap()
});

fn strip_all_tech_brackets(title: &str) -> String {
    BRACKET_GROUP_RE
        .replace_all(title, |caps: &regex::Captures| {
            let group = &caps[0];
            let content = &group[1..group.len() - 1];
            if bracket_content_is_tech(content) {
                " ".to_string()
            } else {
                group.to_string()
            }
        })
        .into_owned()
}

fn bracket_content_is_tech(content: &str) -> bool {
    let upper = content.to_uppercase();
    if TITLE_TECH_TAGS.iter().any(|tag| upper.contains(tag)) {
        return true;
    }
    BRACKET_TECH_RE.is_match(content)
}

fn strip_trailing_noise_parens(title: &str) -> String {
    let trimmed = title.trim_end();
    if !trimmed.ends_with(')') {
        return title.to_string();
    }
    let Some(open) = trimmed.rfind('(') else {
        return title.to_string();
    };
    let content = &trimmed[open + 1..trimmed.len() - 1];
    if paren_content_is_noise(content) {
        trimmed[..open].trim_end().to_string()
    } else {
        title.to_string()
    }
}

static PAREN_NOISE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:beta|alpha|wip|final|rc\s*\d+|v\d+(?:\.\d+)+|minisode\s+\d|episode\s+\d|chapter\s+\d|updated?|released?|now\s+on|now\s+available|\d+\s*maps?)\b",
    )
    .unwrap()
});

fn paren_content_is_noise(content: &str) -> bool {
    if TITLE_TECH_TAGS
        .iter()
        .any(|tag| content.to_uppercase().contains(tag))
    {
        return true;
    }
    PAREN_NOISE_RE.is_match(content)
}

const TITLE_SEPARATORS: &[&str] = &[" - ", " — ", " | ", " // "];

fn strip_metadata_tail(title: &str) -> String {
    let mut best_cut: Option<usize> = None;
    for sep in TITLE_SEPARATORS {
        let mut search_from = 0;
        while let Some(rel) = title[search_from..].find(sep) {
            let cut = search_from + rel;
            let tail = &title[cut + sep.len()..];
            if tail_is_metadata(tail) {
                best_cut = Some(best_cut.map_or(cut, |b| b.min(cut)));
            }
            search_from = cut + sep.len();
        }
    }
    // ": " — only check the first occurrence, and only if there's letter
    // content before it (so we don't eat a leading "IWAD: Plutonia" label).
    if let Some(pos) = title.find(": ")
        && title[..pos].chars().any(|c| c.is_alphabetic())
    {
        let tail = &title[pos + 2..];
        if tail_is_metadata(tail) {
            best_cut = Some(best_cut.map_or(pos, |b| b.min(pos)));
        }
    }
    match best_cut {
        Some(cut) => title[..cut].trim_end().to_string(),
        None => title.to_string(),
    }
}

/// Substrings that, when present in a post-separator tail, mark the tail as
/// a descriptor rather than part of the WAD's canonical name. Padded with
/// spaces at match time so `map` needs word boundaries.
const TAIL_META_KEYWORDS: &[&str] = &[
    "megawad",
    "now on",
    "now available",
    "coming soon",
    "released",
    "my first",
    "my second",
    "my third",
    "single medium",
    "single small",
    "single large",
    "single map",
    "single level",
    "for doom",
    "for plutonia",
    "for tnt",
    "for heretic",
    "for hexen",
    "for strife",
    "for chex",
    "for gzdoom",
    "for zdoom",
    "for dsda",
    "psx doom",
    "psx-doom",
    "psx-inspired",
    "horror wad",
    "horror map",
    "horror mapset",
    "horror episode",
    "slaughter map",
    "slaughter wad",
    "modded action",
    "action-packed",
    "action packed",
    "a vanilla",
    "a boom",
    "a gzdoom",
    "a zdoom",
    "vanilla episode",
    "vanilla mapset",
    "vanilla compatible",
    "boom mapset",
    "boom compatible",
    "limit-removing",
    "limit removing",
    "by the waffle",
    "umapinfo",
    "dsdhacked",
    "community project",
    "challenging map",
    "challenging wad",
    "challenging maps",
];

static TAIL_META_REGEXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // "32 Maps", "12 cl2 maps", "10 map", "40 challenging maps"
        Regex::new(r"(?i)\b\d+\s*[\w-]*\s*(?:maps?|wads?|levels?)\b").unwrap(),
        // "*updated 10/21", "updated 2025-01"
        Regex::new(r"(?i)\*?\s*updated?\b").unwrap(),
        // complevel tech tag inline
        Regex::new(r"(?i)\bcl\d+\b").unwrap(),
        // "on idgames" / "on idgames!"
        Regex::new(r"(?i)\bon\s+idgames\b").unwrap(),
        // explicit version-released phrasing
        Regex::new(r"(?i)\bversion\s+released\b").unwrap(),
        // mbf21/mbf-21
        Regex::new(r"(?i)\bmbf[-\s]?21\b").unwrap(),
    ]
});

fn tail_is_metadata(tail: &str) -> bool {
    let padded = format!(" {} ", tail.to_lowercase());
    if TAIL_META_KEYWORDS.iter().any(|k| padded.contains(k)) {
        return true;
    }
    TAIL_META_REGEXES.iter().any(|re| re.is_match(tail))
}

/// Trailing words that are pure release-announcement hype — stripped one by
/// one from the end until we hit a "real" word. `NG+` / `RC3` / `v2.0` are
/// kept because they convey a version, not pure hype.
const TRAILING_HYPE_WORDS: &[&str] = &[
    "RELEASED",
    "RELEASE",
    "UPDATED",
    "UPDATE",
    "VERSION",
    "NOW",
    "OUT",
    "AVAILABLE",
    "HERE",
    "FINALLY",
];

fn strip_trailing_hype_words(title: &str) -> String {
    let mut words: Vec<&str> = title.split_whitespace().collect();
    while let Some(last) = words.last() {
        let cleaned: String = last
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_uppercase();
        if TRAILING_HYPE_WORDS.iter().any(|h| cleaned == *h) {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ")
}

/// Find a short version marker. Title is the strong-signal source (release
/// threads put the version in the subject), so we accept any pattern there.
/// In the body we require an explicit `Version:` / `Release:` / `Updated to`
/// label — otherwise unrelated `vX.Y.Z` mentions (GZDoom version, DSDA
/// version, Mediafire link fragments) would hijack the field.
fn extract_version(title: &str, body: &str) -> Option<String> {
    for re in VERSION_PATTERNS.iter() {
        if let Some(m) = re.find(title) {
            return Some(m.as_str().trim().to_string());
        }
    }
    if let Some(caps) = LABELED_VERSION_RE.captures(body)
        && let Some(m) = caps.get(1)
    {
        return Some(m.as_str().trim().to_string());
    }
    None
}

fn is_caveat_line(line_lower: &str, port_name: &str) -> bool {
    // e.g. "FOR GZDOOM USERS: ...", "for people using gzdoom, ...",
    // "if you're using gzdoom, ...", "nugget doom users:".
    line_lower.contains(&format!("for {} users", port_name))
        || line_lower.contains(&format!("{} users:", port_name))
        || (line_lower.contains("if you") && line_lower.contains(port_name))
        || (line_lower.starts_with("for ") && line_lower.contains("users"))
}

// =============================================================================
// Download Link Extraction
// =============================================================================

static DOWNLOAD_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Direct file downloads
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]+\.(?:zip|wad|pk3|pk7|7z|rar|tar\.gz)"#).unwrap(),
        // Dropbox
        Regex::new(r#"(?i)https?://(?:www\.)?dropbox\.com/[^\s<>"'\)\]]+"#).unwrap(),
        Regex::new(r#"(?i)https?://dl\.dropbox(?:usercontent)?\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Google Drive
        Regex::new(r#"(?i)https?://drive\.google\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Mediafire
        Regex::new(r#"(?i)https?://(?:www\.)?mediafire\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Mega
        Regex::new(r#"(?i)https?://mega\.(?:nz|co\.nz)/[^\s<>"'\)\]]+"#).unwrap(),
        // GitHub releases
        Regex::new(r#"(?i)https?://github\.com/[^\s<>"'\)\]]+/releases/[^\s<>"'\)\]]+"#).unwrap(),
        Regex::new(r#"(?i)https?://github\.com/[^\s<>"'\)\]]+\.(?:zip|wad|pk3|pk7)"#).unwrap(),
        // itch.io
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]+\.itch\.io/[^\s<>"'\)\]]+"#).unwrap(),
        // ModDB
        Regex::new(r#"(?i)https?://(?:www\.)?moddb\.com/[^\s<>"'\)\]]+/downloads/[^\s<>"'\)\]]+"#)
            .unwrap(),
        // Doomworld idgames
        Regex::new(r#"(?i)https?://(?:www\.)?doomworld\.com/idgames/[^\s<>"'\)\]]+"#).unwrap(),
        // idgames mirror
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]*idgames[^\s<>"'\)\]]*\.(?:zip|wad)"#).unwrap(),
        // GameBanana
        Regex::new(r#"(?i)https?://(?:www\.)?gamebanana\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // OneDrive
        Regex::new(r#"(?i)https?://(?:1drv\.ms|onedrive\.live\.com)/[^\s<>"'\)\]]+"#).unwrap(),
        // Catbox
        Regex::new(r#"(?i)https?://files\.catbox\.moe/[^\s<>"'\)\]]+"#).unwrap(),
        // Litterbox
        Regex::new(r#"(?i)https?://litter\.catbox\.moe/[^\s<>"'\)\]]+"#).unwrap(),
    ]
});

static HREF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)href=["']([^"']+)["']"#).unwrap());

/// Extract potential download URLs from post text.
fn extract_download_links(text: &str) -> Vec<String> {
    let mut all_urls: Vec<String> = Vec::new();

    // Check hrefs first (more reliable than text matching)
    for caps in HREF_PATTERN.captures_iter(text) {
        let href = &caps[1];
        for pattern in DOWNLOAD_PATTERNS.iter() {
            if pattern.is_match(href) {
                all_urls.push(href.to_string());
                break;
            }
        }
    }

    // Then check plain text
    for pattern in DOWNLOAD_PATTERNS.iter() {
        for m in pattern.find_iter(text) {
            all_urls.push(m.as_str().to_string());
        }
    }

    // Decode HTML entities (`&amp;` → `&`) and deduplicate while preserving
    // order. Without decoding, query-param separators leak through as
    // literal `&amp;` in the stored URL, which breaks browser handoff and
    // direct-download attempts.
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for url in all_urls {
        let decoded = html_escape::decode_html_entities(&url).to_string();
        let cleaned = decoded
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .to_string();
        let key = cleaned.to_lowercase();
        if seen.insert(key) {
            unique.push(cleaned);
        }
    }

    unique
}

// =============================================================================
// HTML Parsing Helpers
// =============================================================================

static JSON_LD_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]*type=["']application/ld\+json["'][^>]*>(.*?)</script>"#).unwrap()
});

static HTML_TITLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

static FIRST_POST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<div[^>]*data-role=["']commentContent["'][^>]*>(.*?)</div>\s*(?:<div[^>]*class=["'][^"']*ipsSigned|</article)"#,
    )
    .unwrap()
});

static FIRST_POST_FALLBACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<article[^>]*>.*?<div[^>]*class=["'][^"']*ipsType_richText[^"']*["'][^>]*>(.*?)</div>"#,
    )
    .unwrap()
});

static THREAD_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(?:forum/topic|vb/thread)/(\d+)").unwrap());

// HTML-to-text patterns
static BR_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?\s*>").unwrap());
static P_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</p>").unwrap());
static DIV_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</div>").unwrap());
static LI_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</li>").unwrap());
static ALL_TAGS_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static MULTI_NEWLINE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());
static MULTI_SPACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" +").unwrap());

// Forum-footer sentinels: anything from the first match downward is Invision chrome.
static FOOTER_CUT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\s*(?:share\s+this\s+post|link\s+to\s+post|edited\s+.+\s+by\s+\S)").unwrap()
});
// Lone numeric line = image attachment id left behind by tag stripping.
static LONE_DIGITS_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{1,6}$").unwrap());
// ASCII/Unicode decoration run (horizontal rule substitutes).
static DECORATION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\s_\-=/<>*|•·.#~]{5,}$").unwrap());
// Spoiler chrome left behind when collapsible sections are flattened.
const SPOILER_LABELS: &[&str] = &["Spoiler", "Quote", "Reveal hidden contents"];

// Version markers — checked against title first, then post body. First match
// wins within the set; numeric-version patterns come earlier because they're
// more specific.
static VERSION_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // v1.2, v2.0.1a, V3.4
        Regex::new(r"(?i)\bv\d+(?:\.\d+)+[a-z]?\b").unwrap(),
        // RC3, rc-3, rc 2
        Regex::new(r"(?i)\brc[- ]?\d+\b").unwrap(),
        // Beta, Beta 2, Alpha 1, Final
        Regex::new(r"(?i)\b(?:beta|alpha|final)\s*\d*\b").unwrap(),
        // Minisode 1, Episode 2, Chapter 3
        Regex::new(r"(?i)\b(?:minisode|episode|chapter)\s+\d+\b").unwrap(),
    ]
});

/// In the post body we require an explicit version *label* before accepting
/// a numeric/RC/beta token. `Version:` / `Release:` demand the colon (so
/// "release 3.2.7" referring to SLADE's release number doesn't match), while
/// `Updated [to] X` tolerates bare phrasing.
static LABELED_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)\b(?:(?:version|release)\s*[:=]\s*|updated?(?:\s+to)?\s+)(v?\d+(?:\.\d+)+[a-z]?|rc[- ]?\d+|beta\s*\d*|alpha\s*\d*)\b",
    )
    .unwrap()
});

/// Keywords that mark a bracketed title prefix/suffix as a *technical* tag
/// (IWAD/port/compat flavor) rather than part of the WAD's name. When found
/// inside `[...]` we strip the whole bracket group.
const TITLE_TECH_TAGS: &[&str] = &[
    "MBF21",
    "MBF-21",
    "MBF",
    "BOOM",
    "VANILLA",
    "LIMIT-REMOVING",
    "LIMIT REMOVING",
    "UMAPINFO",
    "DSDHACKED",
    "DSDHACK",
    "OGG",
    "NEW RELEASE",
    "RELEASE",
    "WIP",
    "BETA",
    "ALPHA",
    "FINAL",
    "COMMUNITY PROJECT",
    "GZDOOM",
    "ZDOOM",
    "VKDOOM",
    "LZDOOM",
    "QZDOOM",
    "DSDA-DOOM",
    "DSDA DOOM",
    "NUGGET",
    "WOOF",
    "CHOCOLATE",
    "ETERNITY",
    "CRISPY",
];

// =============================================================================
// Main Parser
// =============================================================================

/// Parser for extracting metadata from Doomworld forum thread pages.
///
/// Uses a multi-strategy approach:
/// 1. JSON-LD structured data (preferred, most reliable)
/// 2. HTML meta tags and content (fallback)
/// 3. Regex-based extraction for technical requirements
pub struct DoomworldParser;

impl DoomworldParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a Doomworld forum thread page.
    pub fn parse(&self, html_content: &str, url: &str) -> ForumThread {
        let thread_id = self.extract_thread_id(url);
        let mut title = String::new();
        let mut author = String::new();
        let mut posted_date = String::new();

        // Try JSON-LD first (most reliable)
        if let Some(json_ld) = self.extract_json_ld(html_content) {
            title = json_ld
                .get("headline")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let author_data = json_ld.get("author");
            if let Some(obj) = author_data.and_then(|a| a.as_object()) {
                author = obj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            } else if let Some(s) = author_data.and_then(|a| a.as_str()) {
                author = s.to_string();
            }

            posted_date = json_ld
                .get("dateCreated")
                .or_else(|| json_ld.get("datePublished"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        // Fallback to HTML title
        if title.is_empty() {
            title = self.extract_html_title(html_content);
        }
        // Keep the pre-strip title — `[MBF21]`, `-cl21`, `for Plutonia`, etc.
        // live in titles and the detectors need to see them.
        let raw_title = title.clone();
        title = strip_tech_brackets(&title);

        // Extract first post content
        let first_post_html = self.extract_first_post(html_content);
        let first_post_text = html_to_text(&first_post_html);

        // Extract technical metadata. Download links need the raw HTML so
        // we can scrape hrefs, but complevel / iwad / sourceport run on the
        // clean plain text so forum chrome (CSS classes, data attrs, @user
        // links) can't contaminate line-context analysis. The raw title is
        // prepended as its own line so tags in the subject line count.
        let combined_text = format!("{first_post_html} {first_post_text}");
        let metadata_text = format!("{raw_title}\n{first_post_text}");
        let download_links = extract_download_links(&combined_text);
        let complevel = extract_complevel(&metadata_text);
        let iwad = extract_iwad(&metadata_text).map(|s| s.to_string());
        let sourceport = extract_sourceport(&metadata_text).map(|s| s.to_string());
        let version = extract_version(&raw_title, &first_post_text);

        ForumThread {
            thread_id,
            title,
            author,
            posted_date,
            first_post_html,
            first_post_text,
            thread_url: url.to_string(),
            download_links,
            complevel,
            iwad,
            sourceport,
            version,
        }
    }

    /// Extract thread ID from Doomworld forum URL.
    fn extract_thread_id(&self, url: &str) -> i64 {
        THREAD_ID_PATTERN
            .captures(url)
            .and_then(|caps| caps.get(1)?.as_str().parse().ok())
            .unwrap_or(0)
    }

    /// Extract JSON-LD structured data from HTML.
    fn extract_json_ld(&self, html_content: &str) -> Option<serde_json::Value> {
        for caps in JSON_LD_PATTERN.captures_iter(html_content) {
            let json_str = caps.get(1)?.as_str().trim();
            let data: serde_json::Value = match serde_json::from_str(json_str) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Handle @graph format
            if let Some(graph) = data.get("@graph").and_then(|g| g.as_array()) {
                for item in graph {
                    if item.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting")
                    {
                        return Some(item.clone());
                    }
                }
            }
            // Direct DiscussionForumPosting
            if data.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting") {
                return Some(data);
            }
            // Array of items
            if let Some(arr) = data.as_array() {
                for item in arr {
                    if item.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting")
                    {
                        return Some(item.clone());
                    }
                }
            }
        }
        None
    }

    /// Extract title from HTML `<title>` tag, cleaning up suffix.
    fn extract_html_title(&self, html_content: &str) -> String {
        let caps = match HTML_TITLE_PATTERN.captures(html_content) {
            Some(c) => c,
            None => return String::new(),
        };
        let mut title = caps
            .get(1)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();

        // Remove common suffixes
        for suffix in [" - Doomworld", " - WADs & Mods", " - Everything Else"] {
            if let Some(stripped) = title.strip_suffix(suffix) {
                title = stripped.to_string();
            }
        }

        html_escape::decode_html_entities(&title).trim().to_string()
    }

    /// Extract HTML content of the first post.
    fn extract_first_post(&self, html_content: &str) -> String {
        // Try data-role attribute first (Invision Community 4.x)
        if let Some(caps) = FIRST_POST_PATTERN.captures(html_content) {
            return caps
                .get(1)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .to_string();
        }

        // Fallback: ipsType_richText inside first article
        if let Some(caps) = FIRST_POST_FALLBACK.captures(html_content) {
            return caps
                .get(1)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .to_string();
        }

        String::new()
    }
}

impl Default for DoomworldParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert HTML to plain text, preserving paragraph breaks, and strip the
/// forum chrome that survives tag removal (spoiler labels, share/edit footer,
/// stray attachment-id digits, ASCII decoration rules, leading indentation
/// from flattened `<blockquote>`/nested-`<div>` structure).
fn html_to_text(html_content: &str) -> String {
    if html_content.is_empty() {
        return String::new();
    }

    let mut text = html_content.to_string();

    // Replace block elements with newlines
    text = BR_PATTERN.replace_all(&text, "\n").to_string();
    text = P_CLOSE_PATTERN.replace_all(&text, "\n\n").to_string();
    text = DIV_CLOSE_PATTERN.replace_all(&text, "\n").to_string();
    text = LI_CLOSE_PATTERN.replace_all(&text, "\n").to_string();

    // Remove all other tags
    text = ALL_TAGS_PATTERN.replace_all(&text, "").to_string();

    // Decode HTML entities
    text = html_escape::decode_html_entities(&text).to_string();

    // Strip zero-width characters that break regex anchoring and show up as
    // garbage in descriptions; normalize NBSP to a regular space.
    text = text
        .replace(['\u{feff}', '\u{200b}', '\u{200c}', '\u{200d}'], "")
        .replace('\u{00a0}', " ");

    // Truncate at the first Invision footer marker ("Share this post",
    // "Link to post", "Edited <date> by <user>"); everything past that is
    // boilerplate chrome.
    if let Some(m) = FOOTER_CUT_PATTERN.find(&text) {
        text.truncate(m.start());
    }

    // Drop pure-noise lines and trim indentation that blockquotes leave behind.
    let mut cleaned = String::with_capacity(text.len());
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            cleaned.push('\n');
            continue;
        }
        if SPOILER_LABELS.iter().any(|l| line.eq_ignore_ascii_case(l)) {
            continue;
        }
        if LONE_DIGITS_PATTERN.is_match(line) {
            continue;
        }
        if DECORATION_PATTERN.is_match(line) {
            continue;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }

    // Normalize whitespace
    let mut out = MULTI_NEWLINE_PATTERN
        .replace_all(&cleaned, "\n\n")
        .to_string();
    out = MULTI_SPACE_PATTERN.replace_all(&out, " ").to_string();

    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Complevel detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_complevel_explicit() {
        assert_eq!(extract_complevel("complevel 9"), Some(9));
        assert_eq!(extract_complevel("cl21"), Some(21));
        assert_eq!(extract_complevel("-complevel 11"), Some(11));
        assert_eq!(extract_complevel("complevel: 2"), Some(2));
    }

    #[test]
    fn test_extract_complevel_named() {
        assert_eq!(extract_complevel("vanilla compatible"), Some(2));
        assert_eq!(extract_complevel("boom compatible"), Some(9));
        assert_eq!(extract_complevel("MBF21"), Some(21));
        assert_eq!(extract_complevel("limit-removing"), Some(2));
    }

    #[test]
    fn test_extract_complevel_none() {
        assert_eq!(extract_complevel("just a regular doom map"), None);
    }

    #[test]
    fn test_extract_complevel_ignores_loose_vanilla_mentions() {
        // These all used to trip the old first-match-wins logic into cl2.
        assert_eq!(extract_complevel("Vanilla+ goodness."), None);
        assert_eq!(extract_complevel("use the Vanilla Essence mod"), None);
        assert_eq!(extract_complevel("chocolate and strawberry"), None);
    }

    #[test]
    fn test_extract_complevel_mbf21_beats_coincidental_vanilla() {
        // When both a loose vanilla reference and an explicit MBF21 literal
        // appear, MBF21 wins.
        let text =
            "Tested with Vanilla Essence mod. MBF21 features required, compatibility mode MBF21.";
        assert_eq!(extract_complevel(text), Some(21));
    }

    #[test]
    fn test_extract_complevel_numeric_beats_named() {
        // Explicit `complevel 9` outranks any named-level keyword.
        assert_eq!(
            extract_complevel("chocolate doom is cool but this is complevel 9"),
            Some(9)
        );
    }

    #[test]
    fn test_extract_complevel_dsdhacked_implies_mbf21() {
        assert_eq!(extract_complevel("DSDHacked features used"), Some(21));
    }

    // -------------------------------------------------------------------------
    // IWAD detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_iwad_doom2() {
        assert_eq!(extract_iwad("requires Doom II"), Some("doom2"));
        assert_eq!(extract_iwad("doom2.wad"), Some("doom2"));
        assert_eq!(extract_iwad("for Doom 2"), Some("doom2"));
    }

    #[test]
    fn test_extract_iwad_tnt() {
        assert_eq!(extract_iwad("tnt.wad"), Some("tnt"));
        assert_eq!(extract_iwad("TNT: Evilution"), Some("tnt"));
    }

    #[test]
    fn test_extract_iwad_plutonia() {
        assert_eq!(extract_iwad("plutonia.wad"), Some("plutonia"));
    }

    #[test]
    fn test_extract_iwad_doom1() {
        assert_eq!(extract_iwad("Ultimate Doom"), Some("doom"));
        assert_eq!(extract_iwad("doom.wad"), Some("doom"));
    }

    #[test]
    fn test_extract_iwad_heretic() {
        assert_eq!(extract_iwad("for Heretic"), Some("heretic"));
        assert_eq!(extract_iwad("requires heretic"), Some("heretic"));
        assert_eq!(extract_iwad("heretic.wad"), Some("heretic"));
    }

    #[test]
    fn test_extract_iwad_bare_heretic_is_ignored() {
        // The old detector wrongly picked up "Heretic" from a texture credit
        // line like "H - Heretic/Hexen/Hexen 2".
        let text = "H - Heretic/Hexen/Hexen 2 resource credits here.";
        assert_eq!(extract_iwad(text), None);
    }

    #[test]
    fn test_extract_iwad_label_wins_over_later_game_mentions() {
        // Plutatoes: "IWAD: Plutonia, not Doom 2" → the label wins and the
        // negated `Doom 2` mention doesn't sneak in.
        let text = "IWAD: Plutonia, not Doom 2\nStill tested in Doom 2 launcher.";
        assert_eq!(extract_iwad(text), Some("plutonia"));
    }

    #[test]
    fn test_extract_iwad_label_required_form() {
        assert_eq!(extract_iwad("IWAD required: Heretic"), Some("heretic"));
        assert_eq!(extract_iwad("IWAD needed: Doom 2"), Some("doom2"));
    }

    // -------------------------------------------------------------------------
    // Title tech-tag stripping
    // -------------------------------------------------------------------------

    #[test]
    fn test_strip_tech_brackets_leading() {
        assert_eq!(strip_tech_brackets("[MBF21] Foo"), "Foo");
        assert_eq!(strip_tech_brackets("[NEW RELEASE] Lazer"), "Lazer");
        assert_eq!(strip_tech_brackets("[MBF21, UMapInfo, OGG] Falaz"), "Falaz");
    }

    #[test]
    fn test_strip_tech_brackets_trailing() {
        assert_eq!(
            strip_tech_brackets("In Bloom [MBF21 community project]"),
            "In Bloom"
        );
        // Both the trailing `[MBF21]` and the `24 maps` descriptor after the
        // `|` separator should come off — leaving just "Sewerlust RC3".
        assert_eq!(
            strip_tech_brackets("Sewerlust RC3 | 24 maps [MBF21]"),
            "Sewerlust RC3"
        );
    }

    #[test]
    fn test_strip_tech_brackets_repeats() {
        assert_eq!(strip_tech_brackets("[MBF21][WIP] Foo"), "Foo");
    }

    #[test]
    fn test_strip_tech_brackets_passthrough() {
        // Pure non-tech content round-trips unchanged.
        assert_eq!(strip_tech_brackets("Plain Title"), "Plain Title");
        assert_eq!(strip_tech_brackets("[Episode 1]"), "[Episode 1]");
        // Versioned brackets are treated as tech now.
        assert_eq!(strip_tech_brackets("Foo [v1.0 Bar]"), "Foo");
    }

    // -------------------------------------------------------------------------
    // Full thread-subject cleanup (strip brackets + tail + hype)
    // -------------------------------------------------------------------------

    #[test]
    fn test_clean_subject_inline_tech_bracket() {
        assert_eq!(
            strip_tech_brackets("[-cl21] Trixie Lulamoon Loves Slaughtermaps"),
            "Trixie Lulamoon Loves Slaughtermaps"
        );
        assert_eq!(
            strip_tech_brackets("FORLORN [RC2] | 12 cl2 maps *updated 10/21"),
            "FORLORN"
        );
    }

    #[test]
    fn test_clean_subject_trailing_paren_version() {
        assert_eq!(strip_tech_brackets("Baronden (beta)"), "Baronden");
        assert_eq!(
            strip_tech_brackets("Plutatoes (Minisode 1 RC Out Now!)"),
            "Plutatoes"
        );
    }

    #[test]
    fn test_clean_subject_metadata_tail() {
        assert_eq!(
            strip_tech_brackets("Destiny Doom - Megawad for Doom2 (32 Maps)"),
            "Destiny Doom"
        );
        assert_eq!(
            strip_tech_brackets("It's open mic night! - Single medium map for Plutonia"),
            "It's open mic night!"
        );
        assert_eq!(
            strip_tech_brackets("River Styx - My first map!"),
            "River Styx"
        );
        assert_eq!(
            strip_tech_brackets("Alter Locus: PSX Doom-Inspired 10 map WAD"),
            "Alter Locus"
        );
        assert_eq!(
            strip_tech_brackets("Butterknife - a vanilla episode"),
            "Butterknife"
        );
        assert_eq!(
            strip_tech_brackets("The Warp | Now on Idgames!"),
            "The Warp"
        );
    }

    #[test]
    fn test_clean_subject_keeps_artistic_subtitle() {
        // Artistic subtitle without metadata keywords stays put.
        assert_eq!(
            strip_tech_brackets("Pa'l Hipocampo - Journey into your dream islands"),
            "Pa'l Hipocampo - Journey into your dream islands"
        );
        assert_eq!(
            strip_tech_brackets("Mercuria - Katakomby"),
            "Mercuria - Katakomby"
        );
    }

    #[test]
    fn test_clean_subject_trailing_hype() {
        assert_eq!(
            strip_tech_brackets("The Pact NG+ VERSION RELEASED"),
            "The Pact NG+"
        );
    }

    // -------------------------------------------------------------------------
    // Version extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_version_numeric() {
        assert_eq!(
            extract_version("My WAD v1.2.3", "body"),
            Some("v1.2.3".into())
        );
        assert_eq!(
            extract_version("My WAD V2.0a", "body"),
            Some("V2.0a".into())
        );
    }

    #[test]
    fn test_extract_version_rc() {
        assert_eq!(
            extract_version("Sewerlust RC3 | 24 maps", "body"),
            Some("RC3".into())
        );
        // Body needs an explicit label now.
        assert_eq!(
            extract_version("Title", "Release: RC-5"),
            Some("RC-5".into())
        );
    }

    #[test]
    fn test_extract_version_ignores_port_version_in_body() {
        // "Tested with GZDoom v4.11.0" used to store v4.11.0 as the WAD
        // version. Without an explicit label we no longer accept it.
        assert_eq!(
            extract_version("The Warp", "Tested with GZDoom v4.11.0 and newer."),
            None
        );
        // But a labeled body version still works.
        assert_eq!(
            extract_version("Plain Title", "Version: 1.2.3"),
            Some("1.2.3".into())
        );
        // "Updated to vX" is also a valid label (no colon needed).
        assert_eq!(
            extract_version("Plain Title", "Updated to v2.1 last week."),
            Some("v2.1".into())
        );
        // "release 3.2.7" (SLADE's release number, no colon) must NOT match.
        assert_eq!(
            extract_version("Plain Title", "on release 3.2.7 of SLADE."),
            None
        );
    }

    #[test]
    fn test_extract_version_minisode() {
        // Plutatoes: `RC` (no number) doesn't hit the RC+digit pattern, so
        // we fall through to the Minisode/Episode matcher.
        assert_eq!(
            extract_version("Plutatoes (Minisode 1 RC Out Now!)", "body"),
            Some("Minisode 1".into())
        );
    }

    #[test]
    fn test_extract_version_none() {
        assert_eq!(extract_version("Just a Title", "plain body"), None);
    }

    #[test]
    fn test_extract_iwad_none() {
        assert_eq!(extract_iwad("no iwad info here"), None);
    }

    // -------------------------------------------------------------------------
    // Sourceport detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_sourceport() {
        assert_eq!(extract_sourceport("GZDoom required"), Some("gzdoom"));
        assert_eq!(extract_sourceport("Requires DSDA-Doom."), Some("dsda-doom"));
        assert_eq!(
            extract_sourceport("Requires DSDA-Doom. Tested with GZDoom."),
            Some("dsda-doom")
        );
        assert_eq!(
            extract_sourceport("Source port: GZDoom (tested with 4.11)"),
            Some("gzdoom")
        );
        assert_eq!(
            extract_sourceport("Advanced engine required: Eternity Engine"),
            Some("eternity")
        );
    }

    #[test]
    fn test_extract_sourceport_none() {
        assert_eq!(extract_sourceport("no port mentioned"), None);
        assert_eq!(extract_sourceport("tested in dsda-doom"), None);
        assert_eq!(extract_sourceport("Tested with GZDoom v4.11.0"), None);
        assert_eq!(extract_sourceport("Eternity Engine"), None);
        assert_eq!(extract_sourceport("crispy doom"), None);
        assert_eq!(
            extract_sourceport("Tested with GZDoom, requires Doom II."),
            None
        );
        assert_eq!(extract_sourceport("Recommended source port: GZDoom"), None);
        assert_eq!(extract_sourceport("GZDoom was only tested."), None);
        assert_eq!(extract_sourceport("GZDoom was not required."), None);
    }

    #[test]
    fn test_extract_sourceport_intended_label_wins() {
        // Falaz-style: GZDoom only shows up in a caveat line; Nugget is
        // declared as the intended port.
        let text = "\
            INTENDED PORT: Nugget Doom, others are untested.\n\
            FOR GZDOOM USERS: Go to Options -> Compatibility.\n";
        assert_eq!(extract_sourceport(text), Some("nugget-doom"));
    }

    #[test]
    fn test_extract_sourceport_engine_needed_label() {
        // Baby's Frist-style: "Advanced engine needed : GZDoom / VKDoom".
        let text = "Advanced engine needed : GZDoom / VKDoom";
        assert_eq!(extract_sourceport(text), Some("gzdoom"));
    }

    #[test]
    fn test_extract_sourceport_negation_excludes_port() {
        // "May Not Run With : PRBoom+" should not route us to prboom+.
        let text = "\
            May Not Run With: PRBoom+ (DSDHacked is newer).\n\
            Tested with DSDA Doom.\n";
        assert_eq!(extract_sourceport(text), None);
    }

    #[test]
    fn test_extract_sourceport_for_users_caveat_demoted() {
        let text = "\
            Designed for Nugget Doom.\n\
            For GZDoom users: enable MBF21 compat mode.\n";
        assert_eq!(extract_sourceport(text), Some("nugget-doom"));
    }

    // -------------------------------------------------------------------------
    // Download links
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_download_links_direct() {
        let text = "Download here: https://example.com/test.zip";
        let links = extract_download_links(text);
        assert_eq!(links, vec!["https://example.com/test.zip"]);
    }

    #[test]
    fn test_extract_download_links_href() {
        let text = r#"<a href="https://example.com/test.wad">Download</a>"#;
        let links = extract_download_links(text);
        assert!(!links.is_empty());
        assert!(links[0].contains("test.wad"));
    }

    #[test]
    fn test_extract_download_links_dedup() {
        let text = r#"https://example.com/test.zip https://example.com/test.zip"#;
        let links = extract_download_links(text);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_extract_download_links_hosting_services() {
        let text = "https://www.dropbox.com/s/abc/test.zip \
                     https://mega.nz/file/abc \
                     https://drive.google.com/file/d/abc";
        let links = extract_download_links(text);
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_extract_download_links_trailing_punct() {
        let text = "Get it at https://example.com/test.zip.";
        let links = extract_download_links(text);
        assert_eq!(links[0], "https://example.com/test.zip");
    }

    // -------------------------------------------------------------------------
    // HTML-to-text
    // -------------------------------------------------------------------------

    #[test]
    fn test_html_to_text_basic() {
        let html = "<p>Hello</p><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_br() {
        let html = "Line 1<br>Line 2<br/>Line 3";
        let text = html_to_text(html);
        assert!(text.contains("Line 1\nLine 2\nLine 3"));
    }

    #[test]
    fn test_html_to_text_entities() {
        let html = "&amp; &lt; &gt; &quot;";
        let text = html_to_text(html);
        assert!(text.contains("& < > \""));
    }

    #[test]
    fn test_html_to_text_empty() {
        assert_eq!(html_to_text(""), "");
    }

    // -------------------------------------------------------------------------
    // Thread ID extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_thread_id() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_thread_id("https://www.doomworld.com/forum/topic/134292-myhousewad/"),
            134292
        );
        assert_eq!(
            parser.extract_thread_id(
                "https://www.doomworld.com/forum/topic/134292-myhousewad/?page=5"
            ),
            134292
        );
        assert_eq!(
            parser.extract_thread_id("https://www.doomworld.com/vb/thread/153124"),
            153124
        );
        assert_eq!(parser.extract_thread_id("https://invalid-url.com"), 0);
    }

    // -------------------------------------------------------------------------
    // JSON-LD extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_json_ld() {
        let parser = DoomworldParser::new();
        let html = r#"
            <script type="application/ld+json">
            {
                "@type": "DiscussionForumPosting",
                "headline": "My Cool WAD",
                "author": {"name": "mapper"},
                "dateCreated": "2024-01-15"
            }
            </script>
        "#;
        let result = parser.extract_json_ld(html).unwrap();
        assert_eq!(result["headline"], "My Cool WAD");
        assert_eq!(result["author"]["name"], "mapper");
    }

    #[test]
    fn test_extract_json_ld_graph() {
        let parser = DoomworldParser::new();
        let html = r#"
            <script type="application/ld+json">
            {
                "@graph": [
                    {"@type": "WebPage"},
                    {"@type": "DiscussionForumPosting", "headline": "Found It"}
                ]
            }
            </script>
        "#;
        let result = parser.extract_json_ld(html).unwrap();
        assert_eq!(result["headline"], "Found It");
    }

    #[test]
    fn test_extract_json_ld_none() {
        let parser = DoomworldParser::new();
        assert!(
            parser
                .extract_json_ld("<html><body>no json</body></html>")
                .is_none()
        );
    }

    // -------------------------------------------------------------------------
    // HTML title extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_html_title() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_html_title("<title>Cool WAD - WADs &amp; Mods - Doomworld</title>"),
            "Cool WAD - WADs & Mods"
        );
        assert_eq!(
            parser.extract_html_title("<title>Thread Title - Doomworld</title>"),
            "Thread Title"
        );
    }

    #[test]
    fn test_extract_html_title_missing() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_html_title("<html><body>no title</body></html>"),
            ""
        );
    }

    // -------------------------------------------------------------------------
    // First post extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_first_post() {
        let parser = DoomworldParser::new();
        let html = r#"
            <div data-role="commentContent">
                <p>First post content here</p>
            </div>
            <div class="ipsSigned">
        "#;
        let post = parser.extract_first_post(html);
        assert!(post.contains("First post content here"));
    }

    #[test]
    fn test_extract_first_post_fallback() {
        let parser = DoomworldParser::new();
        let html = r#"
            <article id="comment-123">
                <div class="ipsType_richText">Fallback content</div>
            </article>
        "#;
        let post = parser.extract_first_post(html);
        assert!(post.contains("Fallback content"));
    }

    // -------------------------------------------------------------------------
    // Full parse integration
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_full() {
        let parser = DoomworldParser::new();
        let html = r#"
            <html>
            <head>
            <title>Test WAD v2 - WADs &amp; Mods - Doomworld</title>
            <script type="application/ld+json">
            {
                "@type": "DiscussionForumPosting",
                "headline": "Test WAD v2",
                "author": {"name": "TestMapper"},
                "dateCreated": "2024-06-15"
            }
            </script>
            </head>
            <body>
            <div data-role="commentContent">
                <p>A Boom compatible map for Doom II.</p>
                <p>Download: <a href="https://example.com/testwad.zip">here</a></p>
            </div>
            <div class="ipsSigned"></div>
            </body>
            </html>
        "#;

        let thread = parser.parse(
            html,
            "https://www.doomworld.com/forum/topic/99999-test-wad-v2/",
        );
        assert_eq!(thread.thread_id, 99999);
        assert_eq!(thread.title, "Test WAD v2");
        assert_eq!(thread.author, "TestMapper");
        assert_eq!(thread.posted_date, "2024-06-15");
        assert_eq!(thread.complevel, Some(9)); // "Boom compatible"
        assert_eq!(thread.iwad.as_deref(), Some("doom2")); // "Doom II"
        assert!(!thread.download_links.is_empty());
        assert!(thread.has_technical_info());
    }
}
