//! Host tests for the selected-song publication cell (Training Mode v1,
//! Step 3 — design §4.6/§5): the seqlock torn-read guard and the
//! dance-bank header → `{code_digest, audio_len_ms}` composition the
//! create detour publishes through.

use super::binding::song_code_digest;
use super::generator_tests::replay_fixture;
use super::selected_song::{digests_coherent, publication_from_bank, SelectedSongCell};

/// `replay_fixture`'s main entry: 32 768 frames at 8 kHz — exactly 4096 ms.
const FIXTURE_MAIN_MS: u32 = 4_096;

#[test]
fn never_published_cell_reads_none() {
    let cell = SelectedSongCell::new();
    assert!(cell.read().is_none());
}

#[test]
fn publish_read_round_trip_with_even_generations() {
    let cell = SelectedSongCell::new();
    cell.publish(0x1234_5678_9abc_def1, 90_000);
    let info = cell.read().expect("published");
    assert_eq!(info.code_digest, 0x1234_5678_9abc_def1);
    assert_eq!(info.audio_len_ms, 90_000);
    assert_eq!(info.generation % 2, 0, "settled generations are even");

    // A fresh publication supersedes: generation advances by one write
    // cycle (+2) and the reader sees the new values.
    cell.publish(0xfeed_beef_feed_bee1, 120_000);
    let next = cell.read().expect("republished");
    assert_eq!(next.code_digest, 0xfeed_beef_feed_bee1);
    assert_eq!(next.audio_len_ms, 120_000);
    assert_eq!(next.generation, info.generation + 2);
}

#[test]
fn torn_write_is_never_observable() {
    let cell = SelectedSongCell::new();
    cell.publish(0x1111_1111_1111_1111, 60_000);
    // Freeze the cell mid-write (odd generation): the reader must reject
    // rather than return a mixed generation (AC 1's torn-read guard).
    cell.begin_write();
    assert!(
        cell.read().is_none(),
        "mid-write state must not be readable"
    );
    cell.finish_write(0x2222_2222_2222_2222, 61_000);
    let info = cell.read().expect("write completed");
    assert_eq!(info.code_digest, 0x2222_2222_2222_2222);
    assert_eq!(info.audio_len_ms, 61_000);
}

#[test]
fn publication_from_fixture_bank_both_entry_orders() {
    for preview_first in [false, true] {
        let bytes = replay_fixture(preview_first);
        let publication =
            publication_from_bank("data/sound/win/dance/tst1.xwb", &bytes).expect("dance bank");
        assert_eq!(publication.0, song_code_digest("tst1"));
        assert_eq!(
            publication.1, FIXTURE_MAIN_MS,
            "main-entry duration in ms (preview_first={preview_first})"
        );
    }
}

#[test]
fn non_dance_paths_and_corrupt_banks_publish_nothing() {
    let bytes = replay_fixture(false);
    // Not a dance-bank path: no publication.
    assert!(publication_from_bank("data/sound/win/system/tst1.xwb", &bytes).is_none());
    // Dance path but unparseable bytes: no publication (the detour keeps
    // the previous publication).
    assert!(publication_from_bank("data/sound/win/dance/tst1.xwb", &[0u8; 64]).is_none());
}

#[test]
fn digest_coherence_declines_exactly_the_stale_stamp() {
    let (song_a, song_b) = (song_code_digest("tsta"), song_code_digest("tstb"));
    // Matching stamp: coherent (the ordinary case).
    assert!(digests_coherent(song_a, Some(song_a)));
    // Stale stamp against a fresh different song: declined (the
    // fast-confirm race).
    assert!(!digests_coherent(song_a, Some(song_b)));
    // No stamp (publication-less cabinet armed the state): fail open.
    assert!(digests_coherent(0, Some(song_a)));
    // No fresh digest (this create didn't publish): fail open.
    assert!(digests_coherent(song_a, None));
    assert!(digests_coherent(0, None));
}
