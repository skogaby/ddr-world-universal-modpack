# Deterministic Clock Direction

Updated: 2026-09-09
Status: Research; no correction design approved or implemented

## User Feedback

The user questioned the four-minute learning heuristic and asked whether a
deterministic approach exists. The delay belongs to the statistical relative-rate
estimator, not to synchronization itself. Prefer direct audio-clock authority.

Separate two goals:

- **Rate drift:** drive elapsed gameplay time from an audio-backed clock,
  snapshotting that clock at the existing song-start boundary. A direct,
  sufficiently precise clock removes the need to learn a ppm ratio. Preserve
  the existing start/offset convention rather than double-count output latency.
- **Onset jitter:** additionally locate the first song sample in the output
  timeline. A Play/voice-submission request does not establish that placement.

Native press timestamps must still be mapped into the same gameplay/audio
timeline. Updating only the visual playhead or timestamping at render time would
not preserve high-precision input. The paired-domain finding in context.md stands.

## Current Gap

The current observer provides a mixed-output play-byte cursor and voice
submission intervals, not a timestamped, high-resolution per-song presentation
position. Raw cursor reads showed several milliseconds of scatter. Driving the
playhead directly from those steps can exchange drift for jitter. Neither
replacing AVS time with QPC nor assuming nominal sample rate alone proves a lock
to actual audio playback.

The next RE target is the source voice's consumed/presented sample position or
its relationship to the mixer/output timeline, including the timestamp belonging
to a reported position. No extra four-minute capture is a prerequisite to that RE.
No correction code has been written.

## Static Inspection Follow-up

Engine RVAs below are relative to xactengine2_10.dll image base 0x400000.

- Runtime-selected voice wrapper vtable is at RVA 0x3CF0. The earlier xref at
  0x61C98 is a .pdata entry, not the vtable; the actual Start pointer is at 0x3D10.
- Wrapper Start (0x1E1F0) forwards through wrapper+0x28, underlying vtable+0x38,
  then sets wrapper+0x80 to 0 on success. Stop (0x1E230) calls underlying+0x40,
  then sets +0x80 to 1. Wrapper getter 0x1E0D0 returns +0x80.
  **This is play/stop state, not a sample counter.**
- Wrapper submit-buffer (0x1E0F0) forwards a descriptor to underlying+0x48;
  wrapper+0x88 counts queued buffers. Callback at 0x1E2E0 takes the secondary
  interface pointer (wrapper+8), decrementing that buffer count.
- Wrapper constructor 0x1F9A0 allocates 0x98 bytes; initialization 0x1F190 calls
  the parent engine's vtable+0x70 to create the underlying voice at wrapper+0x28,
  then underlying vtable+0x28 to obtain a secondary object at wrapper+0x30.
- Wrapper vtable+0x58/+0x30 helpers forward to underlying+0x58/+0x60; they are
  not identified sample-position APIs. No sample counter was confirmed yet.
- The previous standalone-XAudio2 proposal in audio-output-feasibility.md is
  not evidence of the actual legacy XACT voice ABI. Do not cast this interface
  to modern IXAudio2SourceVoice or import its vtable offsets by analogy.

Static inspection only; no annotations, process attachment, timing writes,
source-code changes, tests, builds, commits or deployment in this follow-up.
