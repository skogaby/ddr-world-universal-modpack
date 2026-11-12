#!/usr/bin/env bash
# Offline validation for the five-digit combo display
# (src/services/combo_digits{,_math}.rs + the core/ap2 recipe it drives).
#
# Usage:
#   ./scripts/validate_combo_digits.sh
#
# Legs:
#   1. the pure layer's own tests (digit math, texture names, layout
#      constants) — `rustc --test`, no dependencies;
#   2. the REAL clip patch on the REAL templates: `dance_combo_root1..3` are
#      extracted from the install's `dance_combo_v3.arc`, patched with the
#      same core/ap2 recipe and the same constants the DLL uses, and checked:
#      the new label, the five placements and their positions, object ids,
#      the retargeted loop, the stock segments byte-identical, serialize /
#      re-parse fixed point;
#   3. bemaniutils `afputils parseafp` must accept each patched template and
#      see the same five-digit segment;
#   4. a rendered preview of the five-digit row (the standalone `number`
#      template, patched the same way) for a human to eyeball.
#
# Legs 2-4 need `$DDR_WORLD_INSTALL` and a bemaniutils checkout
# (`$BEMANIUTILS_DIR`, default: the sibling `../bemaniutils`); they are
# skipped with a note when either is missing. Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }
skip() { echo "[-] skipped: $*"; exit 0; }

MATH="$REPO_ROOT/src/services/combo_digits_math.rs"
[[ -r "$MATH" ]] || die "module source missing: src/services/combo_digits_math.rs"

TMP=$(mktemp -d)
trap 'if [[ -z "${KEEP_TMP:-}" ]]; then rm -rf "$TMP"; fi' EXIT

note "leg 1: pure-layer tests"
rustc --edition=2021 --test "$MATH" -o "$TMP/math-tests" 2>&1 | grep -v '^$' || true
"$TMP/math-tests" --quiet || die "pure-layer tests failed"
echo "PASS  combo_digits_math"

[[ -n "${DDR_WORLD_INSTALL:-}" ]] || skip "legs 2-4: DDR_WORLD_INSTALL is not set"
ARC="$DDR_WORLD_INSTALL/data/arc/bm2d/dance_combo_v3.arc"
[[ -r "$ARC" ]] || skip "legs 2-4: dance_combo_v3.arc not found in the install"
BEMANIUTILS_DIR="${BEMANIUTILS_DIR:-$REPO_ROOT/../bemaniutils}"
if [[ ! -x "$BEMANIUTILS_DIR/arcutils" || ! -x "$BEMANIUTILS_DIR/ifsutils" || ! -x "$BEMANIUTILS_DIR/afputils" ]]; then
  skip "legs 2-4: bemaniutils CLIs not found (set BEMANIUTILS_DIR)"
fi

note "extracting dance_combo_v3"
mkdir -p "$TMP/arc" "$TMP/out"
(cd "$BEMANIUTILS_DIR" && ./arcutils "$ARC" -d "$TMP/arc") >/dev/null 2>&1 || die "arcutils failed"
IFS_FILE=$(find "$TMP/arc" -name "*.ifs" | head -1)
[[ -n "$IFS_FILE" ]] || die "no .ifs inside dance_combo_v3.arc"
(cd "$BEMANIUTILS_DIR" && ./ifsutils "$IFS_FILE" -d "$TMP/ifs") >/dev/null 2>&1 || die "ifsutils failed"
AFP_DIR="$TMP/ifs/afp"
[[ -d "$AFP_DIR" ]] || die "no afp/ directory in the extracted package"

note "leg 2: the real recipe on the real templates"
mkdir -p "$TMP/crate/src"
cat >"$TMP/crate/Cargo.toml" <<EOF
[package]
name = "combo-digits-validate"
version = "0.0.0"
edition = "2021"

[[bin]]
name = "combocheck"
path = "src/main.rs"

[workspace]
EOF
cat >"$TMP/crate/src/main.rs" <<EOF
//! combocheck — dev-leg binary for validate_combo_digits.sh. Never committed.
#![allow(dead_code)]
#[path = "$REPO_ROOT/src/core/ap2/mod.rs"]
mod ap2;
#[path = "$REPO_ROOT/src/core/afp.rs"]
mod afp;
#[path = "$REPO_ROOT/src/services/combo_digits_math.rs"]
mod math;

use ap2::{Ap2Doc, ExtraInstance, PlaceObjectView, Tag};

/// BSI byteswap + string-table cipher removal — the "fully descrambled"
/// shape the game's afp_patcher seam hands a patch function.
fn descramble(afp_path: &str, bsi_path: &str) -> Option<Vec<u8>> {
    let mut data = std::fs::read(afp_path).ok()?;
    let bsi = std::fs::read(bsi_path).ok()?;
    afp::apply_bsi(&mut data, &bsi);
    let st_off = u32::from_le_bytes(data.get(48..52)?.try_into().ok()?) as usize;
    let st_size = u32::from_le_bytes(data.get(52..56)?.try_into().ok()?) as usize;
    let table = data.get(st_off..st_off.checked_add(st_size)?)?.to_vec();
    let plain = ap2::decode_string_table(&table);
    data[st_off..st_off + st_size].copy_from_slice(&plain);
    Some(data)
}

fn check(cond: bool, what: &str) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(what.to_string())
    }
}

/// The invariants of one patched section against its stock self.
fn check_section(doc: &Ap2Doc, stock: &Ap2Doc, path: &ap2::SpritePath) -> Result<String, String> {
    let (stock_tags, stock_frames) = {
        let sec = stock.section(path).ok_or("section")?;
        (sec.tags.len(), sec.frames.len())
    };
    let sec = doc.section(path).ok_or("section after patch")?;
    let old = stock.section(path).ok_or("stock section")?;
    // (A nested sprite that carries the label too is checked through its
    // own path.)
    let untouched = sec.tags[..stock_tags]
        .iter()
        .zip(old.tags.iter())
        .all(|(a, b)| matches!(a, Tag::DefineSprite(_)) || a == b);
    check(untouched, "stock tags untouched")?;
    check(sec.frames[..stock_frames] == old.frames[..], "stock frames untouched")?;
    check(
        sec.label_frame(math::NEW_LABEL) == Some(stock_frames as u16),
        "new label at the first appended frame",
    )?;
    let src_frame = sec.label_frame(math::SRC_LABEL).ok_or("source label")? as usize;
    check(
        sec.frames.len() == stock_frames + (stock_frames - src_frame),
        "appended frame count == source segment length",
    )?;

    // The five placements of the new segment, by instance name.
    let views: Vec<(String, PlaceObjectView)> = sec.tags[stock_tags..]
        .iter()
        .filter_map(|t| match t {
            Tag::PlaceObject(p) => p.view(),
            _ => None,
        })
        .map(|v| {
            let name = v
                .movie_name_offset
                .and_then(|o| doc.strings.get(o))
                .unwrap_or("")
                .to_string();
            (name, v)
        })
        .collect();
    check(views.len() == 5, "five placements in the new segment")?;
    // px, ones first — the three-digit stock row extended one place each way.
    let want = [(322, 59), (256, 71), (190, 83), (124, 95), (58, 107)];
    let mut depths = Vec::new();
    for (i, place) in math::PLACES.iter().enumerate() {
        let name = format!("{place}_usr");
        let hits: Vec<&PlaceObjectView> =
            views.iter().filter(|(n, _)| *n == name).map(|(_, v)| v).collect();
        check(hits.len() == 1, &format!("one {name} in the new segment"))?;
        let v = hits[0];
        check(
            v.translate == Some((want[i].0 * 20, want[i].1 * 20)),
            &format!("{name} at {:?} px (got {:?} raw)", want[i], v.translate),
        )?;
        check(v.flags & 0x1 == 0, &format!("{name} is a create record"))?;
        depths.push(v.depth);
    }
    let mut unique = depths.clone();
    unique.sort_unstable();
    unique.dedup();
    check(unique.len() == 5, "five distinct depths")?;
    check(depths[4] == math::NEW_DEPTH, "the new digit sits at NEW_DEPTH")?;
    check(
        depths.windows(2).all(|w| w[0] > w[1]),
        "a higher place is drawn behind the place to its right",
    )?;
    // The new digit is a copy of the thousands digit: same character.
    let source = |n: &str| views.iter().find(|(name, _)| name == n).and_then(|(_, v)| v.source_tag_id);
    check(
        source(math::NEW_INSTANCE).is_some() && source(math::NEW_INSTANCE) == source(math::DONOR_INSTANCE),
        "the new digit places the donor's character",
    )?;
    // Object id = death frame: every placement outlives the segment's loop.
    let ids: Vec<u16> = views.iter().map(|(_, v)| v.object_id).collect();
    check(ids.windows(2).all(|w| w[0] == w[1]), "one object id for the row")?;
    let shift = (stock_frames - src_frame) as u16;
    let stock_id = old.tags[..]
        .iter()
        .filter_map(|t| match t {
            Tag::PlaceObject(p) => p.view(),
            _ => None,
        })
        .filter(|v| {
            v.movie_name_offset.and_then(|o| stock.strings.get(o)) == Some(math::DONOR_INSTANCE)
        })
        .map(|v| v.object_id)
        .next()
        .ok_or("stock donor")?;
    check(ids[0] == stock_id + shift, "object ids rebased by the frame shift")?;

    // Removes: one per depth of the row.
    let mut removed: Vec<u16> = sec.tags[stock_tags..]
        .iter()
        .filter_map(|t| match t {
            Tag::Opaque(o) if o.tag_id == 0x80 && o.data.len() == 4 => {
                Some(u16::from_le_bytes([o.data[2], o.data[3]]))
            }
            _ => None,
        })
        .collect();
    removed.sort_unstable();
    check(removed == unique, "every depth of the row is removed at the segment end")?;
    Ok(format!(
        "{} at frame {}, object id {}",
        math::NEW_LABEL,
        stock_frames,
        ids[0]
    ))
}

fn run(afp: &str, bsi: &str, out: &str) -> Result<String, String> {
    let data = descramble(afp, bsi).ok_or("descramble")?;
    let mut doc = Ap2Doc::parse(&data).ok_or("parse")?;
    let stock = doc.clone();
    check(doc.serialize().as_deref() == Some(&data[..]), "stock round trip")?;

    // The DLL's recipe (services/combo_digits.rs::patch_template), same
    // constants: every section carrying the source label gets the new one.
    let paths = doc.sections_with_label(math::SRC_LABEL);
    check(!paths.is_empty(), "a section carries the source label")?;
    for path in &paths {
        doc.clone_segment_with_extra_instance(
            path,
            math::SRC_LABEL,
            math::NEW_LABEL,
            math::ROW_SHIFT,
            &ExtraInstance {
                donor_name: math::DONOR_INSTANCE,
                name: math::NEW_INSTANCE,
                depth: math::NEW_DEPTH,
                translate: math::NEW_TRANSLATE,
            },
        )
        .ok_or("recipe refused the template")?;
    }
    let mut summary = String::new();
    for path in &paths {
        summary = check_section(&doc, &stock, path)?;
    }

    let bytes = doc.serialize().ok_or("serialize")?;
    let re = Ap2Doc::parse(&bytes).ok_or("re-parse")?;
    check(re.serialize().as_deref() == Some(&bytes[..]), "fixed point")?;

    // Re-scramble the string table for bemaniutils (its parser always runs
    // the cipher).
    let mut scrambled = bytes.clone();
    let st_off = u32::from_le_bytes(scrambled[48..52].try_into().unwrap()) as usize;
    let st_size = u32::from_le_bytes(scrambled[52..56].try_into().unwrap()) as usize;
    let table = ap2::encode_string_table(&scrambled[st_off..st_off + st_size]);
    scrambled[st_off..st_off + st_size].copy_from_slice(&table);
    std::fs::write(out, &scrambled).map_err(|e| format!("write: {e}"))?;
    Ok(format!(
        "{} -> {} bytes, {} section(s): {summary}",
        data.len(),
        bytes.len(),
        paths.len()
    ))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: combocheck <afp> <bsi> <out_afp>");
        std::process::exit(2);
    }
    match run(&args[0], &args[1], &args[2]) {
        Ok(summary) => println!("OK   {summary}"),
        Err(why) => {
            println!("FAIL {why}");
            std::process::exit(1);
        }
    }
}
EOF
(cd "$TMP/crate" && cargo build --quiet 2>&1 | grep -v 'trim-paths' || true)
CHECK="$TMP/crate/target/debug/combocheck"
[[ -x "$CHECK" ]] || die "combocheck did not build"

TEMPLATES=(dance_combo_root1 dance_combo_root2 dance_combo_root3 number)
for tpl in "${TEMPLATES[@]}"; do
  [[ -f "$AFP_DIR/$tpl" && -f "$AFP_DIR/bsi/$tpl" ]] || die "template $tpl not found in dance_combo_v3"
  "$CHECK" "$AFP_DIR/$tpl" "$AFP_DIR/bsi/$tpl" "$TMP/out/$tpl" | sed "s/^/    [$tpl] /"
  [[ "${PIPESTATUS[0]}" == 0 ]] || die "leg 2: $tpl"
done
echo "PASS  recipe on ${#TEMPLATES[@]} real templates"

note "leg 3: bemaniutils parseafp cross-check"
# The patched output is already byte-swapped: an empty BSI.
printf '\0\0' >"$TMP/out/empty.bsi"
for tpl in "${TEMPLATES[@]}"; do
  (cd "$BEMANIUTILS_DIR" && ./afputils parseafp "$TMP/out/$tpl" "$TMP/out/empty.bsi") >"$TMP/out/$tpl.json" 2>/dev/null \
    || die "leg 3: afputils parseafp rejected the patched $tpl"
  python3 - "$TMP/out/$tpl.json" "$tpl" <<'PYEOF' || die "leg 3: $tpl"
import json, sys
doc = json.load(open(sys.argv[1]))
def sprites(tags):
    for t in tags:
        if t.get("type") == "AP2DefineSpriteTag":
            yield t
            yield from sprites(t["tags"])
cands = [doc] + list(sprites(doc["tags"]))
hits = [s for s in cands if "loop_10000" in (s.get("labels") or {})]
assert hits, "no section carries loop_10000"
want = {"1_usr": (322.0, 59.0), "10_usr": (256.0, 71.0), "100_usr": (190.0, 83.0),
        "1000_usr": (124.0, 95.0), "10000_usr": (58.0, 107.0)}
for sec in hits:
    frame = sec["labels"]["loop_10000"]
    first = sec["frames"][frame]
    tags = sec["tags"][first["start_tag_offset"]:first["start_tag_offset"] + first["num_tags"]]
    row = {t["movie_name"]: (t["transform"]["tx"], t["transform"]["ty"]) for t in tags
           if t["type"] == "AP2PlaceObjectTag"}
    assert row == want, row
    # The segment's loop action targets the new label.
    loops = [json.dumps(t) for t in sec["tags"][first["start_tag_offset"]:] if t["type"] == "AP2DoActionTag"]
    assert len(loops) == 1 and "'loop_10000'" in loops[0] and "'loop_1000'" not in loops[0], loops
print(f"    [{sys.argv[2]}] loop_10000 @ frame {frame} in {len(hits)} section(s): {sorted(want)} (bemaniutils)")
PYEOF
done
echo "PASS  bemaniutils accepts the patched templates"

note "leg 4: rendered preview of the five-digit row"
PREVIEW_DIR="${TMPDIR:-/tmp}/combo_digits_preview"
mkdir -p "$PREVIEW_DIR"
if python3 - "$BEMANIUTILS_DIR" "$IFS_FILE" "$TMP/out/number" "$PREVIEW_DIR/five_digits.png" <<'PYEOF'
import io, os, sys
bemaniutils_dir, ifs_path, patched, out_png = sys.argv[1:5]
sys.path.insert(0, bemaniutils_dir)
from PIL import Image
from bemani.format import IFS
from bemani.format.afp import SWF, Shape, AFPRenderer

ifs = IFS(open(ifs_path, "rb").read(), decode_textures=True)
shapes, textures, swfs = {}, {}, {}
sep = os.sep
for fname in ifs.filenames:
    if fname.startswith(f"geo{sep}"):
        shape = Shape(fname[4:], ifs.read_file(fname))
        shape.parse()
        shapes[fname[4:]] = shape
    elif fname.startswith(f"tex{sep}") and fname.endswith(".png"):
        textures[fname[4:-4]] = Image.open(io.BytesIO(ifs.read_file(fname)))
    elif fname.startswith(f"afp{sep}") and not fname.startswith(f"afp{sep}bsi{sep}") and not fname.endswith(".xml"):
        name = fname[4:]
        if name == "number":
            continue  # replaced by the patched template below
        swf = SWF(name, ifs.read_file(fname), ifs.read_file(f"afp{sep}bsi{sep}{name}"))
        swf.parse()
        swfs[name] = swf

edited = SWF("number", open(patched, "rb").read(), b"")
edited.parse()
swfs["number"] = edited
frame = edited.labels["loop_10000"]

# In the game the sprite is driven by label jumps; neutralise the loop
# actions on the render copy so the timeline free-runs into the new segment.
def blank_actions(tags):
    for t in tags:
        if getattr(t, "bytecode", None) is not None:
            t.bytecode.actions = []
        if hasattr(t, "tags"):
            blank_actions(t.tags)
        for trigger in (getattr(t, "triggers", None) or {}).values():
            for code in trigger:
                code.actions = []

blank_actions(edited.tags)
renderer = AFPRenderer(shapes=shapes, textures=textures, swfs=swfs, single_threaded=True)
frames = list(renderer.render_path("number", only_frames=[frame + 2]))
assert frames, "nothing rendered"
frames[0].save(out_png)
print(f"    [render] frame {frame + 1} of the patched number sprite -> {out_png}")
PYEOF
then
  note "leg 4 OK — open the preview: ${PREVIEW_DIR/#$HOME/~}/five_digits.png"
else
  echo "[-] leg 4: render failed (preview only; legs 1-3 are the gate)"
fi

note "ALL GREEN"
