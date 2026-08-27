# Idea Honing: Suppress Score Submission

Requirements clarification log. One question at a time; final decisions recorded
under each.

---

## Q1: What exactly should be suppressed — the score only, or the entire play record / profile update?

When a song ends, DDR typically writes several things back to the server profile:
the per-play **score/result** (clear lamp, EX score, money score, ranking), but
also **play statistics** (play count, calories, last-played, unlocks/EXP,
mission/event progress, etc.). These may or may not all travel on the same code
path.

What's the intended blast radius when Autoplay/Quick-Fail is active?

- **Option A — Score result only:** suppress just the per-play scorewrite so it
  doesn't pollute rankings/best-score, but let everything else (play count,
  unlocks, EXP, events) save normally.
- **Option B — Entire play record:** suppress the whole end-of-song save so the
  play "didn't happen" as far as the profile is concerned (no score, no play
  count, no unlocks/EXP for that play).
- **Option C — Whatever the cleanest single hook gives us:** treat the exact
  boundary as an RE finding — suppress at the most natural chokepoint and accept
  whatever falls inside it, documenting the actual blast radius.

(My lean: this is partly an RE-dependent question, but your *intent* matters for
choosing the hook. Knowing whether you care about play-count/unlocks vs. just
score steers where we cut.)

**Answer (Q1): Option B — suppress the entire play record / end-of-song save.**
The intent is that the play "didn't happen" as far as the server profile is
concerned (no score, no play count, no unlocks/EXP for that play). Maintainer's
suspicion: the cleanest hook path naturally suppresses all of it anyway, because
a lot of things travel together in one upload (i.e. Option B and Option C likely
converge on the same chokepoint). RE will confirm the actual boundary.

---

## Q2: Per-player granularity — must P1 and P2 be suppressed independently?

DDR World is a 2-player cabinet. A session can have P1 and P2 carded in
simultaneously, each with their own score upload. Autoplay (a per-player custom
option) could be on for only one side, and the Quick-Fail gesture (triple-`3`) is
detected per-pinpad. So in a mixed session, one player could legitimately earn a
score while the other autoplayed or quick-failed.

Should suppression be **per-player** (suppress only the offending side's score,
leave the clean side's score intact), or is **all-or-nothing** acceptable
(if either side triggers, suppress the whole upload)?

- **Option A — Per-player:** only the offending side is suppressed. Cleaner for
  the honest player, but depends on the upload being separable per side at the
  hook point (RE-dependent — the batched save may be a single profile object per
  player called twice, or one combined call).
- **Option B — All-or-nothing:** if either side triggers, suppress the entire
  end-of-session save. Simpler, but penalizes a clean co-player.

(My lean: Option A is the "correct" behavior and DDR's save path is almost
certainly invoked per-player — but I want to confirm you want us to do the work to
keep it per-side rather than accept all-or-nothing.)

**Answer (Q2): Split by trigger.**
- **Autoplay → per-player.** Suppress only the side that had Autoplay enabled;
  the clean co-player's score saves normally.
- **Quick Failure → both players.** The triple-`3` gesture fails the song out for
  *both* players at once (it's a shared song-end), so a Quick Failure suppresses
  the upload for **both** sides, regardless of which pinpad triggered it.

Implication: suppression state is the union of (a) per-side Autoplay flags and
(b) a session-wide Quick-Fail flag that forces both sides on. RE should confirm
the assumption that triple-`3` ends the song for both players (vs. only the
gesturing side) — if it turns out per-side, revisit.

---

## Q3: Scope of "not sent" — network upload only, or also local score recording / results display?

"Sent to the server" points at the network profile upload, but a song-end touches
several score sinks:

1. **Network profile upload** (the e-amusement / backend save — the stated target).
2. **Local best-score / play-history** written to the machine (if any persists
   locally between the upload).
3. **The results screen itself** — the score the player sees immediately after the
   song.

Where does suppression stop?

- **Option A — Network upload only:** the player still *sees* their results screen
  normally; we only stop the data from reaching the server. Cleanest scope, and
  matches "not sent to the server" literally.
- **Option B — Network + local persistence:** also prevent any locally-persisted
  best-score / unlock state, but still show the results screen.
- **Option C — Suppress results screen too:** the play shows no score at all.

(My lean: **Option A**. The results screen is a transient display, and on a
networked cabinet local persistence is usually just a cache of what's uploaded —
killing the upload is the meaningful action. Confirm you don't also want the
results screen blanked.)

**Maintainer clarification:** DDR does **not** save scores locally — scores are
purely server-side. So sink #2 (local best-score / play-history) does not exist,
and Option B is moot. The meaningful action is killing the network upload. This
leaves only one open sub-question: does the player still see a normal results
screen?

**Answer (Q3): Option A — suppress the network upload only; the results screen
displays normally.** Since there is no local persistence, "not sent to the server"
== "killed the network save," and that is the entire scope. The immediate
post-song results screen is left untouched (the player still sees their result;
it just never reaches the server). RE target is therefore the network profile/
score upload path specifically.

---

## Q4: When is "this play is tainted" decided, and what resets it?

Autoplay is a per-player toggle (RE will confirm whether it's a custom option vs.
a global mod) and the song can be quick-restarted (triple-`1`) mid-play. So we
need a rule for *when* the tainted decision is latched and *when* it clears:

- **Autoplay:** Should we taint the play if Autoplay was on **at any point** during
  the song, or only its state **at song-end** (when the save fires)? On this
  cabinet Autoplay is a per-player option set in the menu and is unlikely to be
  toggled mid-song, so "state at song-end" is probably equivalent in practice.
- **Quick Failure:** the triple-`3` gesture is itself the song-end trigger, so the
  taint is naturally latched at the moment the gesture fires.
- **Quick Restart (triple-`1`):** restarts the current song. A play that was
  restarted but then completed *honestly* (no autoplay, no quick-fail) should
  still save normally — so any per-song taint state must **reset on restart** and
  on each new song start.

Proposed rule (confirm or adjust):
- Maintain per-song taint flags that **reset at song/chart start** (and on quick
  restart).
- Autoplay side flag = Autoplay's live state, read **at save time** (simplest and
  robust; if mid-song toggling ever matters we can latch at start instead).
- Quick-Fail flag = set when the triple-`3` gesture fires; forces **both** sides.
- At the network save hook, suppress side X if (autoplay[X] || quick_fail).

Does this latching/reset model match your intent?

**Answer (Q4): Confirmed — adopt the proposed latching/reset model as written.**
- Per-song taint flags reset at song/chart start and on quick restart (triple-`1`).
- Autoplay side flag = Autoplay's live per-player state read at save time.
- Quick-Fail flag = set when triple-`3` fires; forces both sides.
- At the network save hook: suppress side X if `autoplay[X] || quick_fail`.

---

## Q5: Is this a standalone, toggleable mod, or always-on behavior baked into the existing mods?

The repo's pattern is that each behavior is a `Mod` with an enable/disable entry
in `mod-config.json` and the in-game mod menu. This feature could be:

- **Option A — Its own mod** (e.g. `score-submission-guard`): one mod that owns the
  network-save hook + taint state, reads Autoplay's and Quick-Fail's state, and
  suppresses. Toggleable on/off like every other mod; disabled → scores always
  send. Cleanest separation; the hook + taint logic lives in one place.
- **Option B — Baked into the trigger mods:** Autoplay and `quick_restart_or_fail`
  each set their own taint flag and the suppression hook lives in a shared service.
  No separate user-facing toggle — suppression is an implicit, non-optional
  consequence of using Autoplay / Quick-Fail.
- **Option C — Always-on, no toggle, standalone module:** like A but with no
  enable/disable surface (always active when the DLL is loaded).

Two coupled sub-questions:
1. Should there be a **user-facing on/off toggle** for the suppression behavior at
   all (config + mod menu), or is it an implicit consequence of using the triggers?
2. **Failure-mode default:** if the save hook's signature fails to resolve (or the
   mod is disabled), the project convention is graceful degradation — which here
   means **scores send normally** (we never block a save we can't positively
   identify as tainted). Confirm that "fail open = score sends" is the right
   default (vs. "fail closed = suppress on doubt").

(My lean: **Option A** — a standalone `Mod` with its own toggle, fitting the
existing registry/mod-menu/config pattern, reading the two triggers' state through
small accessors. And **fail open**: a missing signature disables suppression and
scores upload normally, consistent with every other mod's graceful degradation.)

**Answer (Q5.1): Option B — hard-baked into the existing `autoplay` and
`quick_restart_or_fail` mods. NOT a standalone mod, and NO user-facing toggle.**
Rationale: integrity. The maintainer does not want users sending faked
(autoplayed) or incomplete (quick-failed) scores to the server. Suppression is a
non-optional, implicit consequence of using Autoplay / Quick-Fail — there must be
no way to turn it off while still using those mods.

Design implication: the suppression hook (on the network save path) and the
shared taint state live in a small **shared service** (e.g.
`services::score_submission_guard` / a taint-state module), because the
one-detour-per-target rule means a single owner must install the save hook.
`autoplay` sets its per-side taint flag; `quick_restart_or_fail` sets the
session-wide quick-fail flag. The service reads those flags at save time and
suppresses. The service installs its detour unconditionally at init (whenever
either trigger mod is registered) rather than behind a config gate.

**5.2 still open** — failure-mode behavior when the save hook can't be installed;
see Q6.

---

## Q6: Failure-mode default — if the score-submission hook can't be installed, fail open or fail closed (and is it the same for both triggers)?

If the network-save signature fails to resolve at init (e.g. a game update moved
the function), the suppression detour can't be installed. Choices: **fail open**
(triggers still work, tainted scores upload) vs. **fail closed via gating the
trigger** (the trigger refuses to enable unless the guard is live, so no tainted
play is ever produced).

**Answer (Q6): Asymmetric — Autoplay fails closed, Quick-Fail fails open.**
- **Autoplay → fail closed.** If the score-submission hook cannot be installed,
  **Autoplay must not enable.** Gate Autoplay's activation on the guard being live
  (save hook successfully installed). Rationale: an autoplayed score is a
  *fabricated* result — the high-integrity risk for leaderboards — so we must never
  allow it to be produced if we can't guarantee it won't be uploaded.
- **Quick-Fail → fail open.** Quick-Fail (triple-`3`) may still operate even if the
  score hook can't be found. Rationale: a quick-failed score is merely an
  *incomplete/failed* play, far less harmful than a faked high score, so it's
  acceptable for it to upload if suppression is unavailable.

Design implications:
- The guard service must expose an `is_available()` / "save hook installed"
  predicate. `autoplay`'s enable path checks it and refuses to enable (logging a
  warning) if false — the existing `required_signatures()` / graceful-degradation
  machinery is the natural vehicle (treat the save signature as **required** for
  the autoplay mod, optional for quick_fail).
- `quick_restart_or_fail` does not gate on the guard; it sets the quick-fail taint
  flag opportunistically, and if the hook is absent the flag simply has no effect.

---

## Q7: Observability — how do we log/validate a suppression, and does the player get any indication?

There's no test harness; validation is cabinet deploy + log observation. Score
suppression is invisible by nature (a thing that *doesn't* happen on a remote
server), so we need a way to confirm it worked.

1. **Developer logging:** Log each suppression decision via the project logger
   (`log_info!`/`log_warn!`), e.g. `"[score-guard] P1 save SUPPRESSED
   (autoplay=true, quick_fail=false)"` and the inverse `"P2 save allowed"`. This
   is the primary cabinet-validation signal. (Assumed yes unless you object.)
2. **Player-facing indication:** Should the player see anything (e.g. a small
   on-results-screen note that the score wasn't submitted), or is it silent?

- **Option A — Silent + logged:** no player-facing UI; just developer logs.
  Simplest, matches the integrity framing (the feature is about *us* enforcing a
  rule, not about informing the player). Player already knows they autoplayed /
  quick-failed.
- **Option B — Logged + a small player indication:** also surface a subtle
  "SCORE NOT SAVED" widget/text on the results screen. More work (widget on the
  results scene), more user clarity.

(My lean: **Option A — silent + logged.** Developer logs for validation, no
player-facing UI. The player who enabled Autoplay or triggered Quick-Fail already
knows what they did, and a results-screen widget is extra render-thread work for
little benefit. Confirm, or say if you want the on-screen indication.)

**Answer (Q7): Option A — silent + logged.** Developer logs (`log_info!`/`log_warn!`)
on every suppression decision (both SUPPRESSED and allowed, with the contributing
flags) are the primary validation signal on cabinet. No player-facing UI / results-
screen widget.

---

## Requirements summary (settled)

| Ref | Decision |
|-----|----------|
| Q1 | Suppress the **entire end-of-song server save** (Option B); likely the same chokepoint as "score-only" anyway — RE confirms boundary. |
| Q2 | **Autoplay → per-player** suppression; **Quick-Fail → both players** (shared song-end). Suppression for side X = `autoplay[X] || quick_fail`. |
| Q3 | **Network upload only.** No local score persistence exists in DDR. Results screen left untouched. |
| Q4 | Per-song taint flags **reset at song/chart start and on quick restart**. Autoplay flag read at save time; Quick-Fail flag set when triple-`3` fires. |
| Q5 | **Hard-baked into `autoplay` + `quick_restart_or_fail`** via a shared guard service; **no user toggle** (integrity: never send faked/incomplete scores). |
| Q6 | **Asymmetric failure mode:** Autoplay **fails closed** (won't enable if save hook unavailable); Quick-Fail **fails open** (operates regardless). |
| Q7 | **Silent + logged.** Developer logs only; no player-facing UI. |

**Key open item for research (RE):** the actual network score/profile-save
chokepoint in `gamemdx.dll` (latest), and whether the existing
`custom_options_persistence` save hook (`save_sender` on ess.dll) sits on that
path or a sibling path. This is the load-bearing unknown that the design depends
on. Secondary RE confirmations: triple-`3` ends the song for both players; how
Autoplay's per-side "enabled" state is best read at save time; whether the save
is invoked once per carded-in player (enabling per-side suppression).

---

## Q8 (emerged from research): How should the logout (card-out) save handle a session where only some songs were tainted?

RE finding: DDR saves a song's score at **two** moments — a per-stage save right
after each song (`PlayerDataSaveStageRequest`, kind=2) and a **logout save** at
card-out (`PlayerDataSaveLogoutRequest`, kind=3) that **re-bundles ALL up-to-5
stages' `/result` blocks in one request**. The per-stage save is cleanly
suppressible per-song; the logout save is a single all-or-nothing request covering
every stage. So a session like {song1 clean, song2 autoplay, song3 clean} can't
have just song2 dropped from the logout save without per-stage surgery.

**Answer (Q8): Suppress the logout save entirely (for that side) if ANY song in
the session was tainted.** Rationale: clean songs were already uploaded by their
own per-stage saves, so the only loss is the logout-only delta/checkpoint for
clean songs. Stays at the single ess `save_sender` chokepoint — no second
(gamemdx-side, per-build-fragile) hook. Integrity-first and simplest-correct.

Implication for taint state: in addition to per-(side, current-stage) taint for
the per-stage save, maintain a **session-sticky "any tainted stage this session"
flag per side** that, once set, suppresses that side's logout save. Both reset on
card-in / new session (and the per-stage taint resets per song / quick restart per
Q4).

**Deploy-time validation (carry to implementation):** confirm on cabinet that the
per-stage (kind=2) save is the authoritative score write (so suppressing the
logout delta for clean songs is harmless). If the backend turns out to commit
scores only on logout, revisit toward per-stage surgical dropping
(`ReflectSavePlayerData` kind=3 hook).
