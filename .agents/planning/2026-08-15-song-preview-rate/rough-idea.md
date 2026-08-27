# Rough Idea: Real-time rate preview at song select

Captured 2026-08-15 from the maintainer's request, verbatim intent:

> I recently added a feature to be able to adjust the playback speed for your
> songs in DDR World, and I want to make a small QoL improvement to the
> feature. I want the playback speed option row that we inject in-game to
> adjust the playback speed of the currently-playing song preview in the music
> selection wheel, and to adjust the speed in real time in the option's
> onValueChange callback. That should allow the user to preview in real-time
> how the playback speed is actually going to sound before they start the
> song. The pitch preservation option should be applied, too. The preview's
> pitch should be preserved or not preserved depending on the currently
> selected option.

In terms of the existing feature set: the `song_speed` scalar row and
`preserve_pitch` child row (mod `song-playback-speed`,
`src/mods/song_playback_speed.rs`) currently only feed desired-percent /
preserve-pitch atomics that are consumed once at scene-26 arming. The song
select preview (the `<code>_s` entry of the slot-5 dance XWB) always plays at
stock rate — the streaming engine deliberately serves the preview entry
verbatim (the "preview passthrough" deviation). This feature makes the
song-select preview itself play at the currently selected rate + DSP mode,
updating live as the player edits the rows.
