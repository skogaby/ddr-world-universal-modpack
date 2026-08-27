# Rough Idea: Suppress Score Submission

## Goal

Prevent player scores from being sent to the server when either of two conditions
holds for the just-played song:

- **(a) Autoplay enabled** — the Autoplay custom option was on during gameplay.
- **(b) Quick Failure triggered** — the player pressed `3` three times during
  gameplay to trigger a Quick Failure (see the `quick_restart_or_fail` mod).

In either scenario, the score result must not be saved/uploaded to the backend
server.

## Known starting point

We already have a hook into the score save / profile save process — it's used by
the custom-options persistence layer (`custom_options_persistence`) for sending
custom option values to the server alongside the player profile. That hook *may*
be the right place to intercept and suppress the score, or it may not — the score
save and the option-children save may travel on different code paths.

## Open question requiring RE

Definitive answer needs reverse engineering against `gamemdx.dll` (latest version,
already loaded in Ghidra): identify exactly where the per-play score result is
serialized/transmitted to the server, and whether suppressing it at the existing
hook point is correct, or whether a new/different hook is required.

## Constraints (inherited from the project)

- In-process hook DLL; no panics across FFI; allocator matching; one detour per
  target function (use shared dispatchers where a hook point is already taken).
- Cross-version-safe addressing (AOB / RTTI / RIP-relative), no hardcoded offsets.
- Validation is by cabinet deploy + log/behavior observation; no unit tests.
