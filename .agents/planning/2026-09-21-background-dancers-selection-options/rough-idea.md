# Rough Idea: Background Dancer / Stage selection options (with live 3D previews)

Captured 2026-09-21 from the maintainer.

I just added a mod to revive background dancers and 3D stages for DDR World. Light PDD
pass: add in-game options under the PLAYFIELD STYLING OPTIONS decorative header. Two new
options, BACKGROUND DANCER and BACKGROUND STAGE, present only when the background dancers
mod is enabled. They let the player select the stage and dancer that's loaded. A RANDOM
option is the default first value.

The custom option's preview area should ideally include a live 3D render previewed over a
target in the template chrome, similar to how the AFP background previews render for the
WebUI background options. The game should detect the target rect in the template chrome and
render the 3D preview there so the player sees the selection in real time. Since these
options require live previews, they should be excluded from the 0-0-0 overlay menu via
configuration.

Stage selection affects both players, so that option should be mirrored between players in
a 2P session. One dancer spawns per player, so the dancer option is independent per side
(no mirroring).

The option itself is a scalar option with text formatting: the first value reads "RANDOM",
then every subsequent value reads "Character #X" starting at #1 (stage analogue: "Stage #X").

Previews should be fully animated. Stage previews include the real stage animations, plus a
camera pan — ideally the stage's existing camera choreography. Dancer animation: a random
choreography per dancer sex, if possible.
