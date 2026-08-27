# Implementation Plan: Assist Tick Volume Option

Status: Approved 2026-08-12

Design: `.agents/planning/20260812-assist-tick-volume/design/detailed-design.md`.
Requirements R1–R11 referenced below are defined there.

Per-step gates (modpack repo): `cargo check --target x86_64-pc-windows-msvc` clean →
`cargo fmt` (whole crate, no file args) → `./build.sh` clean. Per the maintainer's
direction, there is **no per-step cabinet demo** — all live validation is consolidated
into Step 6, one manual end-to-end pass after everything lands.

## Checklist

- [x] Step 1: Label and preview textures
- [x] Step 2: Option row, latch, and per-song threading
- [x] Step 3: Volume application in synthesis
- [x] Step 4: bemani-buddy persistence field
- [x] Step 5: Documentation updates
- [x] Step 6: End-to-end cabinet validation

## Steps

### Step 1: Label and preview textures

**Objective:** The row's label and preview panel assets exist (R9).

**Implementation guidance:** Add the `LABELS` entry after `("assist_tick", "ASSIST TICK")`
and the WIDE `Preview` after the `assist_tick`/`on` panel, exactly as specified in the
design (§Components 3). Run `scripts/gen_option_labels.py`; commit the script change plus
the two generated PNGs (`seop_item_assist_tick_volume.png`,
`seop_image_assist_tick_volume.png`) under
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.

**Tests:** Script runs clean; the two PNGs exist at the expected dimensions (176×16,
368×172); `git status` shows only the script and the two new PNGs. Visually spot-check
the PNGs locally (label text, two-line preview copy in the song_speed panel's layout).

**Integration:** None — additive assets; the game ignores textures no row references.

### Step 2: Option row, latch, and per-song threading

**Objective:** The child row exists with full scroll semantics, visibility predicate, and
persistence; the chosen side's latched volume reaches the per-song state and the song-build
log — no audio change yet (R1, R2, R6, R7, R8).

**Implementation guidance:** In `src/mods/assist_tick.rs` only, per design §Components 1:
constants, `TICK_VOLUME`/`LATCHED_VOLUME` statics, `normalize_volume`, `on_volume_change`,
the gated child registration block (with `Duplicate` reseed and the two WARN paths), the
GAMEPLAY-entry latch copy, `SONG.volume_percent` (reset to 100 in `clear()`), the
`rebuild_for` read of `LATCHED_VOLUME[chosen.side]`, and `volume=` in the "song build"
INFO line. Reset both volume statics to default in `disable()` beside the enable-latch
resets.

**Tests:** Per-step build gates. Live behavior is validated in Step 6 (checklist items
1, 2, 5, 6, 7).

**Integration:** Registers against the existing parent row; consumes the Step 1 textures
for label + preview.

### Step 3: Volume application in synthesis

**Objective:** The latched volume scales the claps in the pre-mixed track (R3, R4, R5).

**Implementation guidance:** Per design §Components 1–2: add `scale_pcm` to
`src/services/se_bank_synth/containers.rs` (re-export via the service's `mod.rs` like its
siblings); extend `Action::Anchor` and `spawn_synthesis` with `volume_percent`; apply the
identity-shortcut-else-scale block in the synthesis closure; add `volume={}%` to the
"synthesis done" INFO line.

**Tests:** Per-step build gates, plus `scripts/validate_se_bank_synth.sh` still passes
(no existing se_bank_synth path is modified; `scale_pcm` is additive). Live audibility is
validated in Step 6 (checklist items 3, 4, 5).

**Integration:** Consumes `SONG.volume_percent` threaded in Step 2; completes the
end-to-end feature on the DLL side.

### Step 4: bemani-buddy persistence field

**Objective:** Network persistence round-trips the value with the card profile (R10).

**Implementation guidance:** In the sibling bemani-buddy repository, per design
§Components 4, scoped from the per-option precedents there (commit `04ddbc2` and the
uncommitted `mod_song_speed` working-tree change with migration
`012_ddr_world_song_speed.sql` — the same file set, one option wide):

1. `models/ddr_world/playdata_3.json`: add `"mod_assist_tick_volume": "s32?"` to both
   option blocks (load-response and save-request shapes). The model JSON is the wire
   source of truth.
2. Re-run the codegen tool (`cargo run -p codegen -- <input> <output-dir>`) to regenerate
   `crates/bemani-protocol/src/ddr_world/playdata_3.rs`. **Never hand-edit the
   `@generated` wire structs** (that repo's AGENTS.md rule).
3. New sqlx migration (next free number after `012`): nullable
   `opt_mod_assist_tick_volume INT NULL DEFAULT NULL` on `ddr_world_profiles`, carrying
   the migration-008/011 convention comment (nullable because stock clients never send
   the field).
4. `crates/db/src/models/ddr_world/profile.rs` + `crates/db/src/mysql/ddr_world/profile.rs`:
   profile model field and query columns.
5. `crates/game-server/src/handlers/ddr_world/playdata.rs`: load-path emission beside
   `mod_song_speed`, `None` in the fresh-profile default block, save-path `child_i32`
   capture — stored verbatim.
6. Regenerate the committed `.sqlx/` offline query cache per that repo's AGENTS.md
   (`sqlx migrate run --source migrations/` against the local DB, then the prepare step).

Coordinate with the in-flight `mod_song_speed` working-tree change — don't clobber it;
this change stacks on top with its own migration number.

**Tests:** bemani-buddy's own gates: `cargo build`, `cargo test`, `cargo clippy
--workspace --all-targets` clean, `cargo fmt`. Codegen output diff is limited to the new
field. Live round-trip is validated in Step 6 (checklist item 6).

**Integration:** The DLL (Steps 2–3) already emits/reads the wire field generically; this
step makes the server remember it.

### Step 5: Documentation updates

**Objective:** Operator/user docs describe the new row (R11).

**Implementation guidance:** Per design §Components 5: README `row_order` complete example
+ option-id bullet list (insert `assist_tick_volume` after `assist_tick`), one sentence in
the README Assist Tick feature row, and the AGENTS.md Assist Tick entry clause (volume
child row + `scale_pcm` application point).

**Tests:** Per-step build gates (repo convention even for docs-only changes); proofread
that the documented range/steps/default match the shipped constants.

**Integration:** Documents the behavior shipped in Steps 1–4.

### Step 6: End-to-end cabinet validation

**Objective:** The single consolidated manual test pass (maintainer-run) covering the
design's full Testing Strategy checklist.

**Implementation guidance:** Deploy the DLL (`./scripts/deploy.sh`) against a bemani-buddy
instance carrying Step 4 (with its migration applied). Walk the design's cabinet
checklist:

1. Row visibility: volume row appears/hides same-frame with the parent, per side in
   versus; label + preview panel render.
2. Scroll semantics: fine 5 / coarse 10, clamps 25/175, default 100, bare-number display.
3. Audibility: same song at 25 / 100 / 175 — quieter / identical to pre-feature /
   louder; synthesis log reports the latched `volume=`.
4. Chosen side: versus with differing volumes follows P1; solo-on-P2 follows P2.
5. Next-song latch: a song-select change applies next song; a mid-song quick-restart
   keeps the current song's volume.
6. Persistence: network round-trip (card-out/in, `persist_json` off) restores the value;
   JSON cache path restores it offline; a hand-edited off-step JSON value snaps on load;
   a fresh profile defaults to 100.
7. Fail-open: parent-only behavior at unity volume with one WARN if the child can't
   register (verify opportunistically via logs if such a build/condition arises — not a
   blocking leg).

**Tests:** The checklist above **is** the test. Record outcomes in this feature's
`progress.md` deploy-and-test log; regressions loop back into the owning step.

**Integration:** Validates Steps 1–5 as one shipped feature.
