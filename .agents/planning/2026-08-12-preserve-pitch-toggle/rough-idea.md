# Rough Idea: Preserve Song Pitch sub-option for Song Playback Speed

Add a sub-option to the in-game SONG SPEED (song playback speed) option on the
MODS tab.

- The sub-option is a **boolean**: whether to **preserve the song's pitch** when
  the playback speed is adjusted.
- **Conditional rendering:** the sub-option row is shown only when the playback
  speed percentage is set to a **non-100 %** value. At 100 % there is no pitch
  alteration to preserve, so the row is hidden.
- **Visual label:** the row reads **"PRESERVE SONG PITCH"** with a boolean
  (OFF/ON-style) value set.
- **Preview image text:** "Decides whether the song's pitch should be preserved
  when the playback speed is adjusted."
- `scripts/gen_options_labels.py` can be used to generate the texture assets
  (row label + preview image).

Source: user request, 2026-08-12.
