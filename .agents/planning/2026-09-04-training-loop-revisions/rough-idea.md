# Rough Idea: Training Mode loop / marker / timeline revisions

Light PDD pass to spec revisions to the Training Mode options for looping a
song, setting start/end markers, and drawing the song timeline.

Predecessor: `.agents/planning/2026-08-13-training-mode/` (complete — v1
Section Practice). This project revises shipped behavior.

## (1) READY-banner hotkey lockout

At the beginning of a song's gameplay, a big READY banner flashes across the
screen for a few seconds before actual gameplay begins. Right now, if the user
presses `6` ("set song end marker") during the READY banner flash, it causes a
soft lock of the game if song looping is disabled.

Proposal: fully disable the 4/5/6 hotkeys (anything to do with looping
markers) while the READY banner is active. The quick restart / quick fail mod
already had to handle the READY banner (those used to soft-lock during READY
too) and is a good starting point — but rather than making the marker hotkeys
*work* during READY, disable them altogether until READY is no longer showing
and gameplay has actually begun (arrows scrolling, playfield rendering).

## (2) Start/end markers become children of LOOP SONG

Move "SONG START TIME" / "SONG END TIME" to be child options of "LOOP SONG".
If looping is disabled (stock state), the user shouldn't be able to play a
selected section of the song only once — setting a specific section is only
allowable when looping is enabled.

Likewise during gameplay: if looping is not enabled, disable the 4/5/6 hotkeys
(they only make sense for looping sessions), and disable the blue highlight
over the generated timeline preview (it shows the looped section). Keep the
timeline view itself regardless of the loop setting, but with looping disabled
draw neither the blue highlight nor the start/end markers. The current-position
marker stays in every scenario — it's useful regardless.
