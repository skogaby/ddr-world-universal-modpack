# Orientation: Assist Tick Volume Option

Findings from the Step 2 blind-spot pass. All paths repo-relative; line numbers as of 2026-08-12.

## The idea in context

Add a per-player scalar child row "TICK EFFECT VOLUME (%)" under the existing `assist_tick`
bool row, visible only while ASSIST TICK is ON, with song_speed's exact scroll semantics
(25–175, fine 5, coarse 10), plus label + preview textures in `scripts/gen_option_labels.py`.

## Custom options framework (src/services/custom_options/)

- `RegisterSpec` (api.rs:190–229): `id`, `ui_kind: UiKind`, `default_value`, `on_change:
  fn(side: u8, new_value: i32)`, `show_when: ShowWhen`, `persist: PersistMode`,
  `save_transform`/`load_transform`.
- Scalar builder: `RegisterSpec::scalar(id, min, max, step_fine, format)` (api.rs:297–320),
  `.step_coarse(n)` (api.rs:324), `.default_value()`, `.on_change()`, `.show_when()`,
  `.persist_transform(save, load)` (api.rs:377).
- `ScalarFormat` (api.rs:89–103): `Integer`, `FixedPoint`, `OffsetInteger`. The row renders a
  **bare number** (rows.rs:1604–1678) — no "%" suffix; the unit lives in the label text.
  song_speed uses `Integer` with label "SONG PLAYBACK SPEED (%)".
- **Predicate-driven child rows**: `ShowWhen::Equals { parent_id, value }` (api.rs:158–164).
  Declarative, per-side, evaluated in rows.rs (`is_show_when_satisfied` rows.rs:281–297;
  live toggle → `update_children_visibility` rows.rs:2113–2129 → same-frame reflow).
  Parent must be registered first or registration fails `RegisterError::UnknownParent`
  (registry.rs:150).
- Precedents:
  - `src/mods/power_user_statistics/mod.rs:75–80` — `pacemaker_threshold` scalar child of
    `pacemaker_to_mserror` (the precedent the user named).
  - `src/mods/webui_options/profile_fields.rs:130–138` — `weight` scalar child of
    `is_disp_weight`, with `.step_coarse(10)`.
- Gates: `custom_options::is_available()` (registration possible),
  `row_injection_available()` (mod.rs:161–166 — strict: includes `rows::is_scalar_ready()`,
  needed by scalar rows specifically), `set_option_available(id, bool)` (mod.rs:174 —
  injection visibility only).
- `register_option` primes both sides to default and fires `on_change` for sides 0/1
  immediately (mod.rs:244–245). Re-enable ⇒ `Duplicate` (no on_change re-fire) — callers
  reseed their atomics from `get_value` (assist_tick.rs:1180–1187 does this today).

## song_speed row — the semantics to copy

`src/mods/song_playback_speed.rs:101–145`:

```rust
RegisterSpec::scalar(OPT_SONG_SPEED, MIN_RATE_PERCENT, MAX_RATE_PERCENT,
                     RATE_PERCENT_STEP, ScalarFormat::Integer)
    .step_coarse(COARSE_STEP)          // 10
    .default_value(IDENTITY_PERCENT)   // 100
    .persist_transform(|_id, v| v, load_normalize)  // load: clamp + snap to step
    .on_change(on_song_speed_change);
```

Constants (src/services/song_rate/lifecycle.rs:71–74): MIN 25, MAX 175, STEP 5, IDENTITY 100.
`snap_rate_percent` (lifecycle.rs:90–95) clamps then rounds to the nearest 5. Registration
is gated on `row_injection_available()` and finished with `set_option_available(id, true)`.

## assist_tick mod — current state (src/mods/assist_tick.rs)

- Bool row registered in `enable()` (lines 1172–1198): `bool_toggle(OPT_ID)` (OPT_ID =
  `"assist_tick"`, line 135), default 0, `on_change` → `ASSIST_TICK_ENABLED[2]` atomics.
- Per-song latch: `on_scene_change` (338–362) at GAMEPLAY entry copies
  `ASSIST_TICK_ENABLED` → `LATCHED_ENABLED` **before** arming the rebuild, so the first
  judge dispatch sees one consistent snapshot. Mid-session changes apply next song.
- Song build (`rebuild_for`, 801–904): first judge dispatch chooses the tick side (FR-5,
  `choose_actor`), reads the chosen actor's `sound_offset` (+0x16c) and the chosen side's
  `judgment_timing`, stores them in `SONG`, phase → AwaitAnchor.
- Synthesis (`spawn_synthesis`, 502–594): background thread per song (generation-tokened);
  parameters are **latched at spawn** — a volume value must be threaded the same way.
- Mixer: `se_bank_synth::synthesize_track(clap_pcm, track_ms)` —
  `src/services/se_bank_synth/containers.rs:96–131`. Mix loop (114–119) sums raw clap
  samples with i32 headroom, saturating to i16:
  `*slot = (*slot as i32 + s as i32).clamp(-32768, 32767) as i16;`
  **There is currently no gain/volume application anywhere in the pipeline.** The natural
  application point is pre-scaling the clap samples (once per song, on the synthesis
  thread) or scaling `s` in the loop — both are pure CPU, no engine surface involved.
- Clap asset: raw mono i16 44.1 kHz, loaded once at init (`load_clap_pcm`,
  se_bank_synth/mod.rs:72–111), stored as `Arc<Vec<i16>>` in `CLAP`.
- One tick track per song, following the FR-5 **chosen side** — versus follows P1 (or the
  only enabled side). A per-player volume can only apply via the chosen side's value.
- Rewind/commit reuse the cached `song.encoded` — volume is constant within a song by
  construction. Quick-restart's in-place reset (`on_song_reset`, 1139–1148) clears song
  state and re-arms the rebuild but keeps the latches — "same song, same latch".
- XACT alternative considered: the `game_audio` surface (register/rewrite/play/stop) has no
  volume call, and adding a live cue/bank volume API would be new RE work. Baking gain into
  the mix matches the existing architecture (everything about the track is per-song baked).

## gen_option_labels.py (scripts/gen_option_labels.py)

- Generates three families into `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`:
  - `LABELS` (line 69) → `seop_item_<id>.png` 176×16 row labels. Existing:
    `("assist_tick", "ASSIST TICK")` line 74; `("song_speed", "SONG PLAYBACK SPEED (%)")`
    line 110.
  - `RIBBONS` (line 134) → value chips; **not needed** for scalar rows (native digit path).
  - `PREVIEW`s (line ~155+) → `seop_image_<id>.png` 368×172 explainer panels; scalar rows
    use `value=None` (single fallback panel). song_speed's entry (lines 423–431), WIDE
    layout:
    ```python
    Preview("song_speed", None, WIDE, [
        "Adjusts the rate at which the song will be played during gameplay.",
        "Less than 100% slows the song down. Greater than 100% speeds the song up.",
    ]),
    ```
- A new scalar row needs exactly: one `LABELS` entry + one `Preview` entry, then a script
  rerun (outputs are committed under `data_mods/`).

## Persistence

- DLL side is fully generic: save emits `<mod_{id}>` per registered option
  (custom_options_persistence.rs:958–1024), load reads it back with `load_transform`
  applied (1061–1167); offline JSON cache (`custom_options.p1/p2`) is generic
  (config.rs:408/532). **Zero framework changes needed.**
- Backend (bemani-buddy, sibling checkout): stores each option in a dedicated nullable
  profile column — verified in `crates/game-server/src/handlers/ddr_world/playdata.rs`
  (e.g. line 343 `mod_song_speed: profile.opt_mod_song_speed`, line 705 save-side
  `child_i32(option, "mod_song_speed")`). A new option needs a matching column +
  round-trip there for **network** persistence; without it the offline JSON cache still
  works (precedent: song_speed's backend change was done as companion work).

## Unknowns / risks surfaced

1. **Whose volume applies** — one tick track, per-player option: only the FR-5 chosen
   side's value can apply. Needs an explicit decision (recommend: chosen side's latched
   value; that's whose chart drives the claps anyway).
2. **>100 % gain vs. clap headroom** — the source clap may sit near full scale; gains
   up to 1.75× rely on the existing i16 saturation (audible soft-clip at the extreme).
   Acceptable-by-design, worth stating.
3. **Latch timing** — mirror `LATCHED_ENABLED`: latch volume at GAMEPLAY entry; applies
   next song. In-place restart keeps the latch (consistent with enables).
4. **Scalar gate** — the child needs `row_injection_available()` (scalar donor readiness),
   a stricter gate than the parent bool's `is_available()`. If it fails, fail open:
   row absent, unity volume.
5. **Row order** — `custom_options.row_order` docs (README complete example) list every id;
   the new id should slot in after `assist_tick`.
