# Plan — step04 task-02 dance_judge asset synthesis and AFP patch

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain)

Task: `.agents/tasks/2026-08-29-s-marvelous-judgement/step04/task-02-dance-judge-patch.code-task.md`
Context: `context.md` (same directory — reading record + probe results + decisions).

## Decisions and justification

### D1 — Pure transform lives in `core/ap2/edit.rs` (task option c)

`Ap2Doc::clone_word_segment_with_new_shape(&mut self, src_label, new_label,
word_shape_id) -> Option<WordSegmentClone>` — a generic "clone a labeled word
segment onto a fresh shape" recipe taking name/id parameters only (no
dance_judge knowledge). Justification over options (a)/(b):

- The harness already mounts `core/ap2` (lib test suite AND the ap2check dev
  binary) — the recipe is host-tested and dev-leg-callable with ZERO harness
  restructuring; options (a)/(b) require faking a `crate::core::ap2` path in
  two generated files or `#[path]` gymnastics in a mod-owned file.
- The recipe genuinely is game-agnostic: "find the sprite the segment places
  that internally places shape X; add a shape; clone the sprite onto it;
  placements-only-clone the segment onto the sprite copy" — nothing
  dance_judge-specific once the caller supplies `word_shape_id` (which needs
  GEO knowledge core/ap2 must not have).
- Keeps the module contract (std-only, self-contained) intact.

Failure contract: `None` may leave the doc PARTIALLY edited (the recipe is a
composition of atomic primitives, not itself atomic) — callers must discard
the doc on `None`. Documented on the fn; the patch fn parses a fresh doc per
call so this costs nothing.

### D2 — GE2D rewriter promotion to `src/core/geo.rs` (std-only)

- `labels(data) -> Option<Vec<String>>` (decoded label texts — the word-chain
  resolver and future patches need reads).
- `rewrite_labels(data, f: impl Fn(&str) -> Option<String>) -> Option<Vec<u8>>`
  — endian-aware (D2EG/GE2D); obfuscation preserved per label (detect
  `first_byte >= 0xA0`, bemaniutils-exact; codec ±0x80 mod 256); new name fits
  old slot ⇒ in-place write + NUL fill (byte-identical to folder_expansion's
  shipped equal/shorter behavior); longer ⇒ 4-aligned append at EOF + label
  pointer repoint + **filesize@12 update** (bemaniutils validates it; keeps the
  mod file self-consistent). `None` = structural failure OR nothing rewritten
  (the old contract). Old strings left in place as dead bytes on append.
- folder_expansion's `patch_ge2d_labels` becomes a thin wrapper: substring
  closure `label.contains(SOURCE_KEY) → replace` + the existing per-label INFO
  log. (Old code truncated longer keys — the promotion fixes that latent bug;
  equal/shorter output stays byte-identical.)
- std-only so the harness mounts it (`geo` module) and its `#[cfg(test)]`
  suite runs on host.

### D3 — Geo id resolution: deterministic at enable + patch-time verification

At enable, `assets.rs` extracts the stock template from the arc, descrambles
(core::afp::apply_bsi + core::ap2::decode_string_table — the harness-proven
pipeline), parses, resolves the word chain (segment placements → sprite whose
nested section places shape Y → geo `{exported}_shape{Y}` whose label ends
with `_marvelous`), then runs the REAL recipe on a scratch copy to learn the
exact ids the patch will allocate (`new_shape_id`, `new_sprite_id` — allocation
is `max_character_id()+1` at call time, deterministic for fixed input bytes).
The geo is written and registered under `dance_judge_shape{new_shape_id}`
right there. The patch fn later verifies its allocated ids match the staged
ones (byte-equal input ⇒ must match; mismatch ⇒ WARN + None, fail-open).

Rejected alternative (lazy registration from the patch fn): registration is
thread-safe (Mutex map insert) so it WOULD work, but the geo FILE must exist
on disk before the game opens it anyway — which forces the enable-time
extraction/rewrite regardless; computing the id there too is strictly simpler
and lets `patch_ready()` mean "assets staged AND names consistent".

### D4 — Names / paths (single constants block in assets.rs, §10 derivation)

| constant | value | derivation |
|---|---|---|
| arc | `data/arc/bm2d/dance_judge0000_v0.arc` | §10 / research |
| ifs (in arc + norm key) | `dance_judge0000_v0.ifs` | arc-contained IFS normalizes bare (folder_expansion precedent) |
| ifs mod path | `dance_judge0000_v0_ifs` | `.ifs → _ifs` (ifs_textures rule) |
| mod root | `./data_mods/s_marvelous` | mod's data dir |
| template | `dance_judge` | exported name (§10) |
| labels | `in_marvelous` → `in_smarvelous` | §10 |
| donor region | read from donor geo (ends `_marvelous`) | §10 skin caveat — never hardcoded |
| new region | donor with trailing `marvelous` → `smarvelous` | task naming convention (`dance_judge0000_smarvelous` for 0000) |
| word PNG | `./data_mods/s_marvelous/dance_judge/smarvelous.png` | committed placeholder 344×61 (donor uvrect exact) |
| atlas prefix | `smarv_dj` | short + unique per cloner docs |

### D5 — Skin gate (v1 scope)

The patch fn only sees template BYTES (no IFS identity). Gate: byte-compare
input against the enable-staged stock descrambled bytes; mismatch ⇒ one
latched WARN + None. Residual caveat (documented): if a skin-suffixed IFS
carries a byte-identical template, the patch applies but the new geo/region
only resolve for the 0000 IFS — the cloned word degrades to invisible there.
v1 = default skin per the task; noted for cabinet validation.

### D6 — Patch registration + flags

`afp_patches::install()` called from mod enable (first time only, static
Once — afp_patcher has no unregister). The fn body: ACTIVE check (disabled ⇒
None) → staged check → skin gate → parse → recipe → id verify → serialize.
`patch_ready()` = assets staged + patch registered; `patch_applied()` =
latched true when the fn returned Some this session (stays true after
disable: the loaded template REMAINS patched in game memory — task-03 must
gate re-drives on `patch_applied() && mod-active`).

## Steps

1. Baseline: run `./scripts/validate_s_marvelous.sh` → logs/00-baseline.log.
2. `src/core/geo.rs` (stubs + tests first, RED) + harness mount + `core/mod.rs`
   registration → implement → GREEN. Tests: LE+BE fixtures, obfuscated+plain,
   equal/shorter in-place (byte-layout assertions vs old algorithm), longer
   append+repoint+filesize, multi-label partial rewrite, no-change ⇒ None,
   corrupt magic/filesize/offsets ⇒ None, round-trip via `labels()`.
3. `core/ap2/edit.rs` recipe + tests in tests.rs (template_fixture): happy path
   (dynamic ids, output re-parses, in_smarvelous present, new shape + cloned
   sprite reference chain, no duplicated definitions), unknown shape id ⇒ None,
   missing label ⇒ None, ambiguous word sprite ⇒ None, round-trip fixed point.
4. folder_expansion: `patch_ge2d_labels` → wrapper over `core::geo`.
5. `src/mods/s_marvelous/afp_patches.rs` + `assets.rs` + `mod.rs` wiring.
6. `scripts/validate_s_marvelous.sh`: mount geo; ap2check modes `smarv-patch` +
   `geo-rewrite`; new Leg D (resolve chain via bemaniutils → run the REAL
   recipe + REAL geo rewriter → render `in_smarvelous` with the texture dict
   carrying the placeholder under the new region name → assert frames
   non-empty → GIF `${TMPDIR}/s_marvelous_preview/in_smarvelous_patched.gif`).
7. Gates: full validate green → `cargo check --target x86_64-pc-windows-msvc`
   → `cargo fmt` (whole crate) → re-run validate → `./build.sh`.
