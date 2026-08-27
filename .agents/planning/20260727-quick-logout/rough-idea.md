# Rough Idea — Quick Logout

**As given (2026-07-27):**

> A quick *logout* feature, wherein I could trigger the logout / end-of-credit
> sequence at-will from music selection.

**Motivating case** (from the research doc): Premium Free freezes the per-stage
counter, so turning it off mid-session leaves the counter far below the
operator's `max_stage`, and the session will not end for several more songs.
There is no vanilla way to abandon a session from song select — the only exits
are playing out the remaining stages or a hard restart, neither of which runs
the e-amusement logout save that carries the profile / customize write-back.

**What "logout" means here** — three separable things happen at the end of a
vanilla session, and the feature is about reaching them on demand:

1. **TOTAL RESULTS** — `TotalResultSequence`, 0-indexed scene 32 (session summary).
2. **The logout save** — `EAmExitRootSequence`, 0-indexed scene 34: expires the
   credit / PASELI session, then per side runs
   `SavePlayerDataActor(side, stage = -1)` → `PlayerDataSaveLogoutRequest` →
   `ReflectSavePlayerData(side, savekind = 3)`. **This** is the save that carries
   the profile/customize write-back (the modpack's `SAVEKIND_LOGOUT`).
3. **THANK YOU FOR PLAYING** — `GameOverSequence`, 0-indexed scene 35, then back
   to attract.

**Source of truth for the mechanism:** `docs/quick_logout_research.md`
(Ghidra static analysis against `gamemdx_20260721.dll`; cross-version anchors
verified on 20260324 / 20260616 / 20260721; **nothing cabinet-validated yet**).
