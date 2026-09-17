//! Random selection for a song — the PURE half of the per-song pick (design
//! §4.3.2): a seeded xorshift64* RNG, stage/dancer candidates from the two
//! `startup.arc` rlists, the A3 fixed choreography pools with Fisher–Yates
//! playlists, the camera main/`_non` split and the developer PIN override.
//!
//! Dependency-free (std only) so the host harness mounts it. Everything that
//! touches the engine or the filesystem (rlist bytes, arc existence, QPC)
//! is injected by the caller.
//!
//! A3 facts ported (research `a3-runtime-rules.md` §1/§4/§5):
//! - the choreography pool is a CODE list — `tu01` exists on disk but is not
//!   in it; `ne01_loop` is dead data;
//! - every set (playlist, main camera list, `_non` list) is Fisher–Yates
//!   shuffled once per song;
//! - dancer `x = (i − (n−1)·0.5) · 1.6`, stage at the origin.
//!
//! World data facts (formats-and-data §1): `map_resources.rlist` repeats keys
//! (`boom00` rows 0 and 32 differ in parts/priorities, `monitor00` 18/24) and
//! carries seven `dummy00` rows; `stage_camera_resources.rlist` is
//! row-parallel; `chara_resources.rlist` rows are
//! `[pl, sex, class, model_scale, shadow_scale, unlock_id]`.

/// A3's fixed male pool (14; `br03` included, `tu01` deliberately NOT).
pub const POOL_MALE: &[&str] = &[
    "br01", "br02", "br03", "hh01", "hh02", "ht01", "ht02", "ht03", "ht04", "ja01", "ja02", "sf01",
    "sf02", "sf03",
];
/// A3's fixed female pool (13).
pub const POOL_FEMALE: &[&str] = &[
    "br01", "br02", "hh01", "hh02", "hh03", "ht01", "ht02", "ht03", "ja01", "ja02", "sf01", "sf02",
    "sf03",
];

/// Lateral pitch between dancers, metres (`DAT_180294188`).
pub const DANCER_X_PITCH: f32 = 1.6;

/// Stage rows that are placeholders, never pickable.
pub const DUMMY_STAGE_KEY: &str = "dummy00";

// ---------------------------------------------------------------------------
// RNG
// ---------------------------------------------------------------------------

/// xorshift64* — small, fast, good enough for picks/shuffles, and trivially
/// reproducible from a `u64` seed (the schedule's purity depends on that).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    /// A zero state would stick at zero — remap it.
    pub const ZERO_SEED_REPLACEMENT: u64 = 0x9E37_79B9_7F4A_7C15;

    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 {
            Self::ZERO_SEED_REPLACEMENT
        } else {
            seed
        })
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)` from the top 24 bits (exact in f32).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / 16_777_216.0
    }

    /// Unbiased uniform in `0..n` (Lemire's multiply-and-reject); `n == 0`
    /// returns 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n <= 1 {
            return 0;
        }
        loop {
            let x = (self.next_u64() >> 32) as u32;
            let m = (x as u64) * (n as u64);
            let low = m as u32;
            if low >= n {
                return (m >> 32) as u32;
            }
            // Rejection zone: low < (2^32 mod n)
            let threshold = n.wrapping_neg() % n;
            if low >= threshold {
                return (m >> 32) as u32;
            }
        }
    }

    /// Fisher–Yates (Durstenfeld), `i` from `len−1` down to 1.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        let mut i = v.len();
        while i > 1 {
            let j = self.below(i as u32) as usize;
            i -= 1;
            v.swap(i, j);
        }
    }
}

/// Per-song seed: `qpc ^ (scene_id << 32)` (design §4.3.2).
pub fn seed_from(qpc: u64, scene_id: u32) -> u64 {
    qpc ^ ((scene_id as u64) << 32)
}

// ---------------------------------------------------------------------------
// Candidates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

impl Sex {
    pub fn from_field(f: &str) -> Option<Sex> {
        match f.trim() {
            "M" | "m" => Some(Sex::Male),
            "F" | "f" => Some(Sex::Female),
            _ => None,
        }
    }
    /// `mc_male` / `mc_female` — the choreography arc stem and clip prefix.
    pub fn arc_stem(self) -> &'static str {
        match self {
            Sex::Male => "mc_male",
            Sex::Female => "mc_female",
        }
    }
    pub fn pool(self) -> &'static [&'static str] {
        match self {
            Sex::Male => POOL_MALE,
            Sex::Female => POOL_FEMALE,
        }
    }
}

/// One `map_resources.rlist` row that can be shown.
#[derive(Debug, Clone, PartialEq)]
pub struct StageCandidate {
    pub key: String,
    /// Row index — the stage id; also the row of `stage_camera_resources`.
    pub row: usize,
    /// `(part name, :N priority)` from fields `[2..]`.
    pub parts: Vec<(String, Option<i32>)>,
}

impl StageCandidate {
    pub fn arc_name(&self) -> String {
        format!("mapset_{}.arc", self.key)
    }
    /// `gm_<stage>_<part>` model names in row order.
    pub fn model_names(&self) -> Vec<String> {
        self.parts
            .iter()
            .map(|(p, _)| format!("gm_{}_{}", self.key, p))
            .collect()
    }
}

/// One `chara_resources.rlist` row with a body arc.
#[derive(Debug, Clone, PartialEq)]
pub struct DancerCandidate {
    pub key: String,
    pub row: usize,
    pub sex: Sex,
    pub class: String,
    pub model_scale: f32,
    pub shadow_scale: f32,
}

impl DancerCandidate {
    pub fn body_arc_name(&self) -> String {
        format!("pl_{}.arc", self.key)
    }
    pub fn body_model_name(&self) -> String {
        format!("pl_{}", self.key)
    }
    /// `pl_<key>_<part>.arc` — the game's `%s_%s00.arc` part-arc rule.
    pub fn part_arc_name(&self, part: &str) -> String {
        format!("pl_{}_{}.arc", self.key, part)
    }
    /// `pl_<key>_<part>` — the ResourceManager key of a part model.
    pub fn part_model_name(&self, part: &str) -> String {
        format!("pl_{}_{}", self.key, part)
    }
    /// The part names whose arc exists in the install (A3: a missing part
    /// arc is silently skipped; faces 02/03 are never shown).
    pub fn parts_present(&self, exists: impl Fn(&str) -> bool) -> Vec<String> {
        PART_NAMES
            .iter()
            .filter(|p| exists(&self.part_arc_name(p)))
            .map(|p| p.to_string())
            .collect()
    }
}

/// The five accessory parts A3's dancer actor attaches (format doc §8).
pub const PART_NAMES: [&str; 5] = ["head00", "hips00", "chest00", "forearm00", "face01"];

/// The body bone a part hangs off (looked up by name in the body's `.b2it`).
pub fn part_attach_bone(part: &str) -> Option<&'static str> {
    match part {
        "head00" | "face01" => Some("Head"),
        "hips00" => Some("Hips"),
        "chest00" => Some("Spine2"),
        "forearm00" => Some("LeftForeArmRoll"),
        _ => None,
    }
}

/// The forearm's SECOND instance: the same model mirrored onto the right arm.
pub const MIRROR_PART: &str = "forearm00";
pub const MIRROR_ATTACH_BONE: &str = "RightForeArmRoll";

/// The ground-contact bones whose MODEL-space positions drive the shadow.
pub const GROUND_BONES: [&str; 5] = ["Hips", "Spine2", "Head", "LeftToeBase", "RightToeBase"];
/// The bone whose height delta vs bind scales the shadow.
pub const HIPS_BONE: &str = "Hips";

/// The shared floor-shadow quad (one instance per dancer).
pub const SHADOW_ARC: &str = "pl_shadow00.arc";
pub const SHADOW_MODEL: &str = "pl_shadow00";

/// Parse `name[:prio]`. A malformed priority makes the whole field the name
/// (stock data never does that).
pub fn parse_part_field(field: &str) -> (String, Option<i32>) {
    if let Some((name, prio)) = field.rsplit_once(':') {
        if let Ok(p) = prio.trim().parse::<i32>() {
            return (name.to_string(), Some(p));
        }
    }
    (field.to_string(), None)
}

/// Stage candidates: every non-`dummy00` row with ≥ 1 part whose
/// `mapset_<key>.arc` exists.
pub fn stage_candidates(
    map_rows: &[(String, Vec<String>)],
    exists: impl Fn(&str) -> bool,
) -> Vec<StageCandidate> {
    let mut out = Vec::new();
    for (row, (key, fields)) in map_rows.iter().enumerate() {
        if key == DUMMY_STAGE_KEY || key.is_empty() {
            continue;
        }
        if fields.len() < 3 {
            continue;
        }
        if !exists(&format!("mapset_{key}.arc")) {
            continue;
        }
        let parts: Vec<(String, Option<i32>)> = fields[2..]
            .iter()
            .filter(|f| !f.is_empty())
            .map(|f| parse_part_field(f))
            .collect();
        if parts.is_empty() {
            continue;
        }
        out.push(StageCandidate {
            key: key.clone(),
            row,
            parts,
        });
    }
    out
}

/// Distinct keys in first-appearance order.
pub fn distinct_stage_keys(cands: &[StageCandidate]) -> Vec<&str> {
    let mut keys: Vec<&str> = Vec::new();
    for c in cands {
        if !keys.contains(&c.key.as_str()) {
            keys.push(&c.key);
        }
    }
    keys
}

/// Uniform over DISTINCT keys, then uniform over that key's rows (so a
/// stage with two rows is not twice as likely, but each of its rows is
/// equally likely once the stage is chosen).
pub fn pick_stage<'a>(rng: &mut Rng, cands: &'a [StageCandidate]) -> Option<&'a StageCandidate> {
    let keys = distinct_stage_keys(cands);
    if keys.is_empty() {
        return None;
    }
    let key = keys[rng.below(keys.len() as u32) as usize];
    let rows: Vec<&StageCandidate> = cands.iter().filter(|c| c.key == key).collect();
    let i = rng.below(rows.len() as u32) as usize;
    rows.get(i).copied()
}

/// Dancer candidates: rows with ≥ 5 fields, a parseable sex and scales, and
/// an existing `pl_<key>.arc`. Unlock ids are ignored (every dancer).
pub fn dancer_candidates(
    chara_rows: &[(String, Vec<String>)],
    exists: impl Fn(&str) -> bool,
) -> Vec<DancerCandidate> {
    let mut out = Vec::new();
    for (row, (key, fields)) in chara_rows.iter().enumerate() {
        if key.is_empty() || fields.len() < 5 {
            continue;
        }
        let Some(sex) = Sex::from_field(&fields[1]) else {
            continue;
        };
        let (Ok(model_scale), Ok(shadow_scale)) = (
            fields[3].trim().parse::<f32>(),
            fields[4].trim().parse::<f32>(),
        ) else {
            continue;
        };
        if !(model_scale > 0.0) || !(shadow_scale >= 0.0) {
            continue;
        }
        if !exists(&format!("pl_{key}.arc")) {
            continue;
        }
        out.push(DancerCandidate {
            key: key.clone(),
            row,
            sex,
            class: fields[2].trim().to_string(),
            model_scale,
            shadow_scale,
        });
    }
    out
}

/// `n` independent uniform picks (repeats allowed — A3's random kinds).
pub fn pick_dancers(rng: &mut Rng, cands: &[DancerCandidate], n: usize) -> Vec<DancerCandidate> {
    if cands.is_empty() {
        return Vec::new();
    }
    (0..n)
        .map(|_| cands[rng.below(cands.len() as u32) as usize].clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Playlists, camera lists, paths, placement
// ---------------------------------------------------------------------------

/// Shuffled pool as clip names `mc_<sex>_<name>_exec`.
pub fn playlist(rng: &mut Rng, sex: Sex) -> Vec<String> {
    let mut names: Vec<&str> = sex.pool().to_vec();
    rng.shuffle(&mut names);
    names
        .iter()
        .map(|n| format!("{}_{}_exec", sex.arc_stem(), n))
        .collect()
}

/// Arc member path of a choreography clip: `data/chara/mc_<sex>/<clip>.anm`.
pub fn clip_member_path(sex: Sex, clip: &str) -> String {
    format!("data/chara/{}/{}.anm", sex.arc_stem(), clip)
}

/// The stage-mode camera lists: names containing `_non` are the cut-away
/// list, everything else the main list; both shuffled (A3 `FUN_18005b490` +
/// `FUN_18005b830`). Empty fields are skipped.
pub fn camera_lists(rng: &mut Rng, camera_row_fields: &[String]) -> (Vec<String>, Vec<String>) {
    let mut main = Vec::new();
    let mut non = Vec::new();
    for f in camera_row_fields {
        let name = f.trim();
        if name.is_empty() {
            continue;
        }
        if name.contains("_non") {
            non.push(name.to_string());
        } else {
            main.push(name.to_string());
        }
    }
    rng.shuffle(&mut main);
    rng.shuffle(&mut non);
    (main, non)
}

/// `data/camera/long/<name[..5]>/<name>.camanm` (`st001x2_st06` → dir
/// `st001`; `floor_st04` → `floor`).
pub fn camanm_member_path(name: &str) -> String {
    let dir: String = name.chars().take(5).collect();
    format!("data/camera/long/{dir}/{name}.camanm")
}

/// A3 placement: `x = (i − (n−1)·0.5) · 1.6`.
pub fn dancer_x(i: usize, n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    (i as f32 - (n as f32 - 1.0) * 0.5) * DANCER_X_PITCH
}

// ---------------------------------------------------------------------------
// Developer pin
// ---------------------------------------------------------------------------

/// `DDR_DANCERS_PIN=<stage>[,<chara>[,<chara>]]`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pin {
    pub stage: Option<String>,
    pub dancers: Vec<String>,
}

/// Empty segments are ignored; all-empty ⇒ `None`.
pub fn parse_pin(s: &str) -> Option<Pin> {
    let mut segs = s.split(',').map(str::trim).filter(|x| !x.is_empty());
    let stage = segs.next().map(str::to_string);
    let dancers: Vec<String> = segs.map(str::to_string).collect();
    if stage.is_none() && dancers.is_empty() {
        return None;
    }
    Some(Pin { stage, dancers })
}

/// Resolve a pin against the candidate tables. Stage = the FIRST row of the
/// pinned key; dancers = the pinned keys in order, the last one repeated to
/// fill `n`; an unknown key ⇒ `None` (caller logs + falls back to random).
/// A pin with no dancer keys leaves `dancers` empty (caller picks randomly).
pub fn apply_pin(
    pin: &Pin,
    stages: &[StageCandidate],
    dancers: &[DancerCandidate],
    n: usize,
) -> Option<(Option<StageCandidate>, Vec<DancerCandidate>)> {
    let stage = match &pin.stage {
        None => None,
        Some(key) => Some(stages.iter().find(|s| &s.key == key)?.clone()),
    };
    let mut picked = Vec::new();
    for key in &pin.dancers {
        picked.push(dancers.iter().find(|d| &d.key == key)?.clone());
    }
    if !picked.is_empty() {
        while picked.len() < n {
            let last = picked.last().cloned()?;
            picked.push(last);
        }
        picked.truncate(n.max(1));
    }
    Some((stage, picked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(v: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        v.iter()
            .map(|(k, f)| (k.to_string(), f.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    /// The real World `map_resources.rlist` key/part multiset (34 rows).
    fn real_map_rows() -> Vec<(String, Vec<String>)> {
        fn p<'a>(parts: &[&'a str]) -> Vec<&'a str> {
            let mut v = vec!["000000", "000000"];
            v.extend_from_slice(parts);
            v
        }
        let table: Vec<(&str, Vec<&str>)> = vec![
            ("boom00", p(&["bg:-2", "ripple", "sp", "spot", "stage:-1"])),
            (
                "monitor01",
                p(&[
                    "back:-5",
                    "stage:-1",
                    "stage2:-2",
                    "stage3:-3",
                    "stage4:-4",
                    "monitor1",
                    "monitor2",
                    "monitor3",
                ]),
            ),
            (
                "disco00",
                p(&[
                    "bg",
                    "field",
                    "light00",
                    "light01",
                    "mirrorball",
                    "spot00",
                    "spot01",
                ]),
            ),
            (
                "crystaldium00",
                p(&[
                    "bg:-2", "ring", "block", "tile", "speaker", "pole:-1", "stage:-1",
                ]),
            ),
            (
                "dawnstreet00",
                p(&["bg", "building", "dec", "ble:-1", "glo"]),
            ),
            (
                "lovesweets00",
                p(&["bg", "star1", "ble:-1", "candy", "star2:-2"]),
            ),
            ("dummy00", p(&["dummy"])),
            (
                "floor00",
                p(&[
                    "bg", "light0", "light1", "light2", "door", "kanban", "gaikan",
                ]),
            ),
            ("dummy00", p(&["dummy"])),
            ("dummy00", p(&["dummy"])),
            ("dummy00", p(&["dummy"])),
            ("dummy00", p(&["dummy"])),
            ("dummy00", p(&["dummy"])),
            ("dummy00", p(&["dummy"])),
            (
                "boom01",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "cyber00",
                p(&[
                    "bg", "light", "bglight", "stage", "pole", "bar", "ring1", "ring2",
                ]),
            ),
            (
                "speaker00",
                p(&["bg", "arrow", "ripple", "roof", "sp", "spot", "stage:-1"]),
            ),
            (
                "club00",
                p(&["bg:-2", "roof", "stage:-1", "spot", "speaker"]),
            ),
            (
                "monitor00",
                p(&["back", "chain:-1", "monitor", "stage", "stagering"]),
            ),
            (
                "replicant00",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
            (
                "replicant01",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
            (
                "replicant02",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
            (
                "monitor02",
                p(&["bg", "monitor", "stage1", "stage2", "stage3", "sp1", "sp2"]),
            ),
            (
                "monitor03",
                p(&["bg", "monitor", "stage1", "stage2", "stage3", "sp1", "sp2"]),
            ),
            (
                "monitor00",
                p(&["back", "chain:-1", "monitor", "stage", "stagering"]),
            ),
            (
                "replicant03",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
            (
                "replicant04",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
            (
                "boom02",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "boom03",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "boom04",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "boom05",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "boom06",
                p(&["bg", "sp", "stage:-1", "ripple", "back", "dodai"]),
            ),
            (
                "boom00",
                p(&["bg", "ripple", "sp", "spot", "stage", "footpanel"]),
            ),
            (
                "replicant05",
                p(&[
                    "bg", "sp", "floor", "hexa1", "hexa2", "hexa3", "monitor1", "monitor2",
                ]),
            ),
        ];
        table
            .into_iter()
            .map(|(k, f)| (k.to_string(), f.into_iter().map(String::from).collect()))
            .collect()
    }

    #[test]
    fn rng_basics() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        assert_eq!(a.next_u64(), b.next_u64());
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
        assert_eq!(Rng::new(0), Rng(Rng::ZERO_SEED_REPLACEMENT));
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
        assert_eq!(r.below(0), 0);
        assert_eq!(r.below(1), 0);
        for _ in 0..1000 {
            assert!(r.below(7) < 7);
        }
        assert_eq!(seed_from(0x1234, 26), 0x1234 | (26u64 << 32));
        assert_eq!(seed_from(0x1234, 26), seed_from(0x1234, 26));
    }

    #[test]
    fn below_is_uniform() {
        let mut r = Rng::new(99);
        let n = 25u32;
        let draws = 250_000;
        let mut counts = vec![0u32; n as usize];
        for _ in 0..draws {
            counts[r.below(n) as usize] += 1;
        }
        let expected = draws as f64 / n as f64;
        let chi2: f64 = counts
            .iter()
            .map(|&c| (c as f64 - expected).powi(2) / expected)
            .sum();
        // 24 dof, 99.9 % critical value ≈ 51.2
        assert!(chi2 < 51.2, "chi2 {chi2} counts {counts:?}");
    }

    #[test]
    fn shuffle_edge_cases_and_permutation() {
        let mut r = Rng::new(3);
        let mut e: [u8; 0] = [];
        r.shuffle(&mut e);
        let mut one = [5u8];
        r.shuffle(&mut one);
        assert_eq!(one, [5]);
        let mut seen_swapped = false;
        for _ in 0..50 {
            let mut two = [1u8, 2];
            r.shuffle(&mut two);
            if two == [2, 1] {
                seen_swapped = true;
            }
            let mut s = two;
            s.sort();
            assert_eq!(s, [1, 2]);
        }
        assert!(seen_swapped);
    }

    #[test]
    fn stage_candidates_exclude_dummy_and_missing_arcs() {
        let rows = real_map_rows();
        let cands = stage_candidates(&rows, |arc| arc != "mapset_cyber00.arc");
        assert!(cands.iter().all(|c| c.key != "dummy00"));
        assert!(cands.iter().all(|c| c.key != "cyber00"));
        assert_eq!(cands.len(), 34 - 7 - 1);
        let boom: Vec<&StageCandidate> = cands.iter().filter(|c| c.key == "boom00").collect();
        assert_eq!(boom.len(), 2);
        assert_eq!(boom[0].row, 0);
        assert_eq!(boom[1].row, 32);
        assert_eq!(boom[0].parts[0], ("bg".to_string(), Some(-2)));
        assert_eq!(boom[0].parts[4], ("stage".to_string(), Some(-1)));
        assert_eq!(boom[1].parts[5], ("footpanel".to_string(), None));
        assert_eq!(boom[1].parts[0], ("bg".to_string(), None));
        assert_eq!(boom[1].arc_name(), "mapset_boom00.arc");
        assert_eq!(boom[1].model_names()[5], "gm_boom00_footpanel");
        let keys = distinct_stage_keys(&cands);
        assert_eq!(keys.len(), 24);
        assert_eq!(keys[0], "boom00");
        // nothing exists → nothing pickable
        assert!(stage_candidates(&rows, |_| false).is_empty());
        assert!(pick_stage(&mut Rng::new(1), &[]).is_none());
    }

    #[test]
    fn pick_stage_is_uniform_over_distinct_keys_then_rows() {
        let rows = real_map_rows();
        let cands = stage_candidates(&rows, |_| true);
        let keys = distinct_stage_keys(&cands);
        assert_eq!(keys.len(), 25);
        let mut r = Rng::new(0xC0FFEE);
        let draws = 100_000usize;
        let mut per_key = vec![0usize; keys.len()];
        let mut boom_rows = [0usize; 2];
        for _ in 0..draws {
            let c = pick_stage(&mut r, &cands).unwrap();
            let ki = keys.iter().position(|k| *k == c.key).unwrap();
            per_key[ki] += 1;
            if c.key == "boom00" {
                boom_rows[if c.row == 0 { 0 } else { 1 }] += 1;
            }
        }
        let expected = draws as f64 / keys.len() as f64;
        let chi2: f64 = per_key
            .iter()
            .map(|&c| (c as f64 - expected).powi(2) / expected)
            .sum();
        // 24 dof, 99.9 % critical value ≈ 51.2
        assert!(chi2 < 51.2, "chi2 {chi2} per_key {per_key:?}");
        let boom_total = (boom_rows[0] + boom_rows[1]) as f64;
        let share = boom_rows[0] as f64 / boom_total;
        assert!(
            (0.45..=0.55).contains(&share),
            "boom00 row split {boom_rows:?}"
        );
        // a repeated key is NOT twice as likely
        assert!((per_key[0] as f64 - expected).abs() < 0.15 * expected);
    }

    #[test]
    fn dancer_candidates_parse_and_require_arcs() {
        let rows = rows(&[
            ("yuni00", &["pl", "F", "A", "0.9", "0.75", "0.0"]),
            ("rage00", &["pl", "M", "A", "1.0", "0.8", "0.0"]),
            ("babylon00", &["pl", "M", "B", "0.4", "0.5", "0.0"]),
            ("emi00", &["pl", "F", "A", "0.9", "0.75", "-1.0"]),
            ("broken", &["pl", "X", "A", "0.9", "0.75", "0.0"]),
            ("short", &["pl", "F"]),
            ("missing", &["pl", "M", "A", "1.0", "1.0", "0.0"]),
        ]);
        let cands = dancer_candidates(&rows, |arc| arc != "pl_missing.arc");
        let keys: Vec<&str> = cands.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["yuni00", "rage00", "babylon00", "emi00"]);
        assert_eq!(cands[2].sex, Sex::Male);
        assert_eq!(cands[2].class, "B");
        assert_eq!(cands[2].model_scale, 0.4);
        assert_eq!(cands[2].shadow_scale, 0.5);
        assert_eq!(cands[3].row, 3);
        assert_eq!(cands[3].body_arc_name(), "pl_emi00.arc");
        assert_eq!(cands[3].body_model_name(), "pl_emi00");
        // unlock -1 rows are candidates too (emi00 present above)
        let picks = pick_dancers(&mut Rng::new(5), &cands, 2);
        assert_eq!(picks.len(), 2);
        assert!(picks.iter().all(|p| cands.contains(p)));
        assert!(pick_dancers(&mut Rng::new(5), &[], 2).is_empty());
        // repeats allowed: with one candidate both picks are that candidate
        let one = pick_dancers(&mut Rng::new(5), &cands[..1], 2);
        assert_eq!(one[0], one[1]);
    }

    #[test]
    fn playlists_are_permutations_of_the_pool() {
        for sex in [Sex::Male, Sex::Female] {
            let a = playlist(&mut Rng::new(11), sex);
            let b = playlist(&mut Rng::new(12), sex);
            assert_eq!(a.len(), sex.pool().len());
            let mut names: Vec<String> = a
                .iter()
                .map(|c| {
                    c.trim_start_matches(&format!("{}_", sex.arc_stem()))
                        .trim_end_matches("_exec")
                        .to_string()
                })
                .collect();
            names.sort();
            let mut pool: Vec<String> = sex.pool().iter().map(|s| s.to_string()).collect();
            pool.sort();
            assert_eq!(names, pool);
            assert!(!a.iter().any(|c| c.contains("tu01")));
            assert!(a
                .iter()
                .all(|c| c.starts_with(sex.arc_stem()) && c.ends_with("_exec")));
            assert_ne!(a, b, "two seeds should shuffle differently");
            // same seed → same order
            assert_eq!(playlist(&mut Rng::new(11), sex), a);
        }
        assert_eq!(POOL_MALE.len(), 14);
        assert_eq!(POOL_FEMALE.len(), 13);
        assert_eq!(
            clip_member_path(Sex::Female, "mc_female_sf01_exec"),
            "data/chara/mc_female/mc_female_sf01_exec.anm"
        );
    }

    #[test]
    fn camera_lists_split_on_non_and_paths() {
        let row: Vec<String> = [
            "st001_st02",
            "st001_st03",
            "st001_st04",
            "st001_st05",
            "st001x2_st06",
            "st001_st07",
            "st001_non01",
            "st001_non02",
            "st001_non03",
            "st001_non04",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (main, non) = camera_lists(&mut Rng::new(8), &row);
        assert_eq!(main.len(), 6);
        assert_eq!(non.len(), 4);
        assert!(main.iter().all(|n| !n.contains("_non")));
        assert!(non.iter().all(|n| n.contains("_non")));
        let mut m = main.clone();
        m.sort();
        assert_eq!(
            m,
            [
                "st001_st02",
                "st001_st03",
                "st001_st04",
                "st001_st05",
                "st001_st07",
                "st001x2_st06"
            ]
        );
        let (main2, _) = camera_lists(&mut Rng::new(9), &row);
        assert_ne!(main, main2);
        assert_eq!(
            camanm_member_path("st001x2_st06"),
            "data/camera/long/st001/st001x2_st06.camanm"
        );
        assert_eq!(
            camanm_member_path("floor_st04"),
            "data/camera/long/floor/floor_st04.camanm"
        );
        assert_eq!(
            camanm_member_path("st006_non04"),
            "data/camera/long/st006/st006_non04.camanm"
        );
        assert_eq!(camanm_member_path("abc"), "data/camera/long/abc/abc.camanm");
        let (m0, n0) = camera_lists(&mut Rng::new(1), &[]);
        assert!(m0.is_empty() && n0.is_empty());
    }

    #[test]
    fn dancer_placement() {
        assert_eq!(dancer_x(0, 1), 0.0);
        assert!((dancer_x(0, 2) + 0.8).abs() < 1e-6);
        assert!((dancer_x(1, 2) - 0.8).abs() < 1e-6);
        assert!((dancer_x(0, 3) + 1.6).abs() < 1e-6 && dancer_x(1, 3) == 0.0);
        assert_eq!(dancer_x(0, 0), 0.0);
    }

    #[test]
    fn pin_parse_and_apply() {
        assert_eq!(parse_pin(""), None);
        assert_eq!(parse_pin(" , ,"), None);
        assert_eq!(
            parse_pin("boom00"),
            Some(Pin {
                stage: Some("boom00".into()),
                dancers: vec![]
            })
        );
        assert_eq!(
            parse_pin("club00, emi00 ,rage00"),
            Some(Pin {
                stage: Some("club00".into()),
                dancers: vec!["emi00".into(), "rage00".into()]
            })
        );
        let stages = stage_candidates(&real_map_rows(), |_| true);
        let dancers = dancer_candidates(
            &rows(&[
                ("yuni00", &["pl", "F", "A", "0.9", "0.75", "0.0"]),
                ("rage00", &["pl", "M", "A", "1.0", "0.8", "0.0"]),
            ]),
            |_| true,
        );
        let pin = parse_pin("boom00,rage00").unwrap();
        let (s, d) = apply_pin(&pin, &stages, &dancers, 2).unwrap();
        assert_eq!(s.as_ref().map(|s| s.row), Some(0), "first row of the key");
        assert_eq!(d.len(), 2);
        assert!(
            d.iter().all(|x| x.key == "rage00"),
            "last dancer repeated to fill n"
        );
        let pin = parse_pin("boom00,yuni00,rage00").unwrap();
        let (_, d) = apply_pin(&pin, &stages, &dancers, 1).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].key, "yuni00");
        // stage only → dancers left for the random path
        let pin = parse_pin("floor00").unwrap();
        let (s, d) = apply_pin(&pin, &stages, &dancers, 2).unwrap();
        assert_eq!(s.unwrap().key, "floor00");
        assert!(d.is_empty());
        // unknown keys refuse
        assert!(apply_pin(&parse_pin("nope").unwrap(), &stages, &dancers, 1).is_none());
        assert!(apply_pin(&parse_pin("boom00,nobody").unwrap(), &stages, &dancers, 1).is_none());
    }
}
