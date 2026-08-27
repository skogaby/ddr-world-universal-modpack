# Progress — Step 7 task-01: Documentation Rewrite and Leftover Sweep

Updated: 2026-08-11
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. README.md (feature row, config example, cache section removal)
- [x] 2. AGENTS.md (Song Playback Speed row, Assist-tick row, movie-hook
      instruction)
- [x] 3. docs/xact_streaming_research.md implementation-findings append
- [x] 4. docs/song_playback_speed.md banner + Rate-aware Real Speed RE
      section
- [x] 5. Design amendment notes (preview passthrough)
- [x] 6. Sweep greps clean; full gate set green; record closed

## What landed

- **README.md:** Song Playback Speed feature row rewritten to the streaming
  truth (no generation pause, no disk cache, ~few-seconds cold load,
  preview plays at normal speed, brief Step-6 mentions: judgment-locked
  assist ticks / effective-tempo Real Speed / CSV rate columns). The
  embedded mod-config example lost the `song_playback_speed` block; the
  cache config section removed outright (see Deviations — the follow-up
  removed ALL historical/retired-key language README-wide). The Assist
  Tick section's capacity sentence updated: 20 min wall — full coverage at
  100 %, 5 min of chart content at 25 %.
- **AGENTS.md:** Song Playback Speed row rewritten to the streaming
  architecture (module pointer set incl. binding/generator/
  io_callback_hook/virtual_bank + StretchState, two-region serving +
  preview passthrough with the WSOLA-throughput rationale, Q31-LAST commit
  order, live-proven numbers, Step-6 integrations incl. `tick_domain` /
  `real_speed` (actor multiplier cluster, RE pointer to
  `docs/song_playback_speed.md` §Rate-aware Real Speed) /
  `csv_rate_cells`, validator sections {synthetic, streaming, corpus,
  identity_runtime, replays}, Config: NONE). Assist-tick row: capacity
  **1200 s wall (~28.9 MB, D15)** + the rate-aware FR-3 formula riding the
  AwaitAnchor-latched snapshot through `song_rate::tick_domain`. The
  movie-hook custom instruction now states live rate suppression
  (tentative at non-identity arm, confirmed at commit) — was "false
  through identity-only Step 3".
- **docs/xact_streaming_research.md §8 (new):** implementation-time
  findings — WSOLA ~2.4× realtime at 47 kHz under CrossOver, the preview
  passthrough / two-region serving model and why (engine primes a stream
  context for EVERY wave), the parser-rule emission contract + the
  fixture-honesty lesson.
- **docs/song_playback_speed.md:** supersession banner (historical record;
  durable streaming RE = xact_streaming_research.md) + **§16 Rate-aware
  Real Speed (2026-08-11)** — the durable copy of the step06-task-02 RE:
  Option scroll-speed cluster table (+0x8/+0xC/+0x10/+0x14/+0x80-0x90,
  vtable+0x208/+0x218, SetBPMs→SetScrollSpeed re-derivation), the
  GamePlayActor multiplier cluster + per-frame renderer copy (why
  Option+0x10 alone is inert), cross-build byte evidence, and the shipped
  recompute's shape.
- **Design amendments (dated inline notes, original text intact):** req 12
  bullet 1 and the virtual-bank component spec ("Both entries are
  stretched…") — both now carry the 2026-08-11 preview-passthrough
  amendment with record pointers.

## Sweep results

- `cache_limit_gib` outside `.agents/`: exactly two hits, BOTH the
  deliberate retirement notes (README retired-section note; the AGENTS row
  stating D9 dropped it) — see Deviations.
- `mod-config.json`: no `song_playback_speed` block (was already clean;
  asserted).
- No "300 s (~7.2 MB)" / "identity-only Step 3" remnants in AGENTS.md; the
  README's remaining "on demand" hits belong to WebUI Options (unrelated,
  correct); all remaining `_cache` mentions belong to other features' real
  caches.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed; 172/172
   in 7.50 s
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
4. `cargo fmt --check` — clean
5. `./build.sh` — release DLL OK

## Deviations

- **Maintainer-directed follow-up (2026-08-11, post-gates):** the README
  must carry NO historical recordkeeping — the project is closed-source
  (open-sourcing planned) and no reader has seen any past state. Removed:
  the Song Playback Speed retired-config section AND the feature row's
  "old config block ignored" sentence (no config = no mention at all); the
  Assist Tick `offset_ms` retired-key sentence (now just "no latency
  knob — timing derives from game state"); the WebUI Options config
  migration note (a retired-key mention of the same class). README now
  greps CLEAN for retired/earlier-builds/development-builds/
  cache_limit_gib/offset_ms. AGENTS.md and the planning/docs records KEEP
  their historical context (agent-facing; the instruction was
  README-scoped, and retired-key knowledge is load-bearing there — e.g.
  the config.rs `diagnostic` parse-but-ignore note). **Second pass
  (maintainer-directed):** the WebUI "old approach ~500–600 textures /
  ~15 s" comparison trimmed, plus the last same-class passages the sweep
  surfaced — the Sanitised-logout row's "used to be suppressed" framing
  (now states current behavior only), the Non-Native-OS networking note's
  "former promotions were removed" history (now states the `-icmphook`
  requirement directly), and the LayeredFS texture note's "legacy path has
  been removed" aside. README now greps clean for old-approach/previously/
  used-to/retired/former/legacy/migration language with all
  operator-useful content preserved. Follow-up touches README only
  (non-compiled) and postdates the gate runs — gates unaffected.
- **AC-4 refined (superseded by the follow-up for README):** originally
  two deliberate retirement notes survived the `cache_limit_gib` grep; the
  README one is now GONE (zero README hits). The AGENTS.md row's "D9
  dropped `cache_limit_gib` outright" remains — agent-facing context.
- README's Assist Tick body section gained the capacity-sentence update
  (20 min wall / 5 min chart at 25 %) — implied by the AGENTS-row
  requirement, applied for consistency.
