# Rough Idea: Assist Tick Volume Option

There's an existing, recently-added Assist Tick mod in this modpack. Right now, the
Assist Tick mod registers a single boolean option with the game's in-game options menu
to turn assist tick on or off.

When Assist Tick is set to ON, present a child row which allows the user to adjust the
volume of the clap sound that's played with the assist tick. The volume row should not
be present when the assist tick option is turned off — there's already precedent for
predicate-driven options with the pacemaker → ms error mod.

The scroll semantics should be identical to the playback speed option in terms of the
acceptable range (25% to 175%) and the coarse vs fine jump speeds.

In the `gen_options_label.py` script, label the option as "TICK EFFECT VOLUME (%)" and
the preview image texture should basically mirror the text and layout of the playback
speed adjustment option, but with appropriate text to describe what it actually does.
