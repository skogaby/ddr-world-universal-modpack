//! Host tests for the binding module: the pure path/digest helpers, the
//! real preflight pipeline's refusal legs (design req 24, 41), the
//! qualifying gate (RE note §5 — never gate on path alone), and the
//! registry's publish/retire/sweep reclamation protocol (req 26).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::binding::{
    dance_bank_song_code, integration_available, next_preview_generation, prepare_binding,
    qualify_bind, song_code_digest, BindQualification, BindRefusal, Binding, BindingRegistry,
    BindingState, PollOutcome, ServeOutcome, SourceView,
};
use super::generator_tests::{
    build_bank_bytes, format, replay_fixture, tone_pcm, transform_bank_oracle_target,
};
use super::lifecycle::{
    ArmRequest, EligibilityDecision, GenerationPhase, LifecycleSink, LifecycleState,
};
use super::transaction::FaultSelector;
use crate::core::xact::{adpcm, virtual_bank, xwb};

struct NullSink;
impl LifecycleSink for NullSink {
    fn publish_identity(&self, _generation: u64, _mask: u8) {}
    fn reset_identity(&self) {}
    fn set_movie_suppressed(&self, _suppressed: bool) {}
}

fn arm(lifecycle: &LifecycleState, percent: i32) {
    lifecycle.on_scene26(
        &NullSink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: percent,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    assert_eq!(lifecycle.phase(), GenerationPhase::Armed);
}

/// Bounded wait for a spawned producer to reach a condition (the real
/// thread runs at memory speed on the tiny fixture; seconds of budget are
/// paranoia, not expectation).
fn wait_for(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    false
}

// ── Path gating ──────────────────────────────────────────────────────

#[test]
fn dance_song_code_derivation_is_exact() {
    // Arming is song-agnostic: the code comes FROM the path.
    assert_eq!(
        dance_bank_song_code("sound/win/dance/diag.xwb").as_deref(),
        Some("diag")
    );
    assert_eq!(
        dance_bank_song_code("sound/win/dance/DIAG.XWB").as_deref(),
        Some("diag")
    );
    assert_eq!(
        dance_bank_song_code("sound/win/dance/other.xwb").as_deref(),
        Some("other")
    );
    // Non-XWB, non-dance, and empty stems never qualify.
    assert_eq!(dance_bank_song_code("sound/win/dance/diag.xsb"), None);
    assert_eq!(dance_bank_song_code("movies/dance/diag.xwb"), None);
    assert_eq!(dance_bank_song_code("sound/win/diag.xwb"), None);
    assert_eq!(dance_bank_song_code("sound/win/dance/.xwb"), None);
}

#[test]
fn song_code_digests_are_stable_nonzero_and_distinct() {
    let diag = song_code_digest("diag");
    // Deterministic, never the unbound sentinel (low bit forced), and
    // distinct across codes.
    assert_eq!(diag, song_code_digest("diag"));
    assert_ne!(diag, 0);
    assert_eq!(diag & 1, 1);
    assert_ne!(diag, song_code_digest("other"));
}

// ── T3: the qualifying gate ──────────────────────────────────────────

#[test]
fn qualification_gates_on_phase_and_song_digest_never_path_alone() {
    let lifecycle = LifecycleState::new();
    // Identity (and every non-Armed/Committed phase) declines silently —
    // the preview player creates slot-5 banks through the identical path.
    assert_eq!(
        qualify_bind(&lifecycle, Some("tst1")),
        BindQualification::Decline
    );

    arm(&lifecycle, 50);
    // A non-dance bank while Armed declines SILENTLY — phase untouched.
    assert_eq!(qualify_bind(&lifecycle, None), BindQualification::Decline);
    assert_eq!(lifecycle.phase(), GenerationPhase::Armed);
    // The first dance bank of the armed generation is the first bind.
    assert_eq!(
        qualify_bind(&lifecycle, Some("tst1")),
        BindQualification::FirstBind
    );

    // Committed + the SAME song's bank = Quick Restart; a different song
    // (or a non-dance bank) declines.
    let generation = lifecycle.generation();
    lifecycle.bind_song(song_code_digest("tst1"));
    lifecycle.begin_binding(generation).unwrap();
    lifecycle.mark_exposed(generation).unwrap();
    lifecycle.mark_committed(generation).unwrap();
    assert_eq!(
        qualify_bind(&lifecycle, Some("tst1")),
        BindQualification::QuickRestart
    );
    assert_eq!(
        qualify_bind(&lifecycle, Some("other")),
        BindQualification::Decline
    );
    assert_eq!(qualify_bind(&lifecycle, None), BindQualification::Decline);
    assert_eq!(lifecycle.phase(), GenerationPhase::Committed);
}

// ── T1: preflight refusal legs (pure pipeline) ───────────────────────

#[test]
fn preflight_refuses_an_unparseable_source() {
    assert!(!integration_available());
    let junk = vec![0u8; 64];
    let result = prepare_binding(
        5,
        1,
        50,
        true,
        &SourceView::new(&junk),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    );
    assert!(matches!(result, Err(BindRefusal::UnsupportedProfile)));
}

/// A parseable strict-profile bank whose MAIN entry carries a loop of
/// length 1 at frame 64 of 32,768 — valid at load, degenerate under the
/// half-up boundary map at 175% (both boundaries land on frame 37).
fn degenerate_loop_fixture() -> Vec<u8> {
    let fmt = format(8_000, 2);
    let main = adpcm::encode_interleaved(&tone_pcm(32_768, 2), fmt).expect("encode main");
    let preview = adpcm::encode_interleaved(&tone_pcm(2_048, 2), fmt).expect("encode preview");
    build_bank_bytes(
        false,
        [fmt, fmt],
        [main.as_slice(), preview.as_slice()],
        [32_768, 2_000],
        [(64, 1), (0, 2_000)],
    )
}

#[test]
fn preflight_refuses_an_unmappable_loop_as_a_plan_refusal() {
    // A loop of length 1 at frame 64 of a 32,768-frame entry maps
    // degenerate at 175% (both boundaries round half-up to frame 37):
    // parseable at load, refused by the plan — the Plan refusal leg
    // exercised through a real (small) bank rather than the 28-bit
    // ceiling, which would need a ~73 MB fixture for the same PlanError
    // surface.
    let fixture = degenerate_loop_fixture();
    // The fixture itself parses; only the plan refuses.
    assert!(xwb::parse_song_bank(&fixture).is_ok());
    let bank = xwb::parse_song_bank(&fixture).unwrap();
    assert!(
        virtual_bank::plan_virtual_bank(&bank, 175, virtual_bank::StretchTarget::Main).is_err()
    );
    drop(bank);
    let result = prepare_binding(
        5,
        1,
        175,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    );
    assert!(matches!(result, Err(BindRefusal::Plan)));
}

#[test]
fn preflight_fault_legs_inject_at_their_documented_sites() {
    let fixture = replay_fixture(false);
    let view = SourceView::new(&fixture);
    for (selector, expected) in [
        ("source-read", BindRefusal::SourceRead),
        ("header-synth", BindRefusal::HeaderSynth),
        ("generator-start", BindRefusal::ProducerStart),
        ("bind-refused", BindRefusal::Injected),
    ] {
        let fault = FaultSelector::parse(selector).expect(selector);
        let result = prepare_binding(
            5,
            1,
            50,
            true,
            &view,
            &fault,
            virtual_bank::StretchTarget::Main,
        );
        match result {
            Err(refusal) => assert_eq!(refusal, expected, "{selector}"),
            Ok(_) => panic!("{selector} must refuse"),
        }
    }
}

#[test]
fn refusal_codes_round_trip_for_the_drain_mailbox() {
    for refusal in [
        BindRefusal::SourceRead,
        BindRefusal::UnsupportedProfile,
        BindRefusal::Plan,
        BindRefusal::HeaderSynth,
        BindRefusal::SourceCopy,
        BindRefusal::ProducerStart,
        BindRefusal::SlotExpose,
        BindRefusal::Injected,
    ] {
        assert_ne!(refusal.code(), 0);
        assert_eq!(BindRefusal::from_code(refusal.code()), Some(refusal));
    }
    assert_eq!(BindRefusal::from_code(0), None);
}

// ── T2/T5: the success pipeline and the mid-song fault arm ───────────

#[test]
fn preflight_builds_a_live_binding_that_serves_and_retires() {
    let fixture = replay_fixture(false);
    let binding = prepare_binding(
        5,
        7,
        50,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    )
    .expect("preflight succeeds");
    assert_eq!(binding.file_id(), 5);
    assert_eq!(binding.generation(), 7);
    assert_eq!(binding.state(), BindingState::Active);

    // The engine's first read: 0x1000 at offset 0 (pre-data + the start of
    // entry-0 data). The spawned producer completes it promptly whether it
    // serves synchronously or defers.
    let mut buffer = vec![0u8; 0x1000];
    let mut accumulator = 0u64;
    let outcome = unsafe { binding.serve(0, 0x1000, buffer.as_mut_ptr(), &mut accumulator) };
    let bytes = match outcome {
        ServeOutcome::Served(n) => {
            // The stock protocol accumulated the synchronous serve.
            assert_eq!(accumulator, u64::from(n));
            u64::from(n)
        }
        ServeOutcome::Pending => {
            let mut reported = 0;
            assert!(
                wait_for(Duration::from_secs(10), || {
                    match unsafe { binding.poll(&mut accumulator) } {
                        PollOutcome::Complete(bytes) => {
                            reported = bytes;
                            true
                        }
                        _ => false,
                    }
                }),
                "header read never completed"
            );
            reported
        }
        ServeOutcome::Refused => panic!("live binding refused the header read"),
    };
    assert_eq!(bytes, 0x1000);
    // The served pre-data is the plan's synthesized header.
    let bank = xwb::parse_song_bank(&fixture).unwrap();
    let layout =
        virtual_bank::plan_virtual_bank(&bank, 50, virtual_bank::StretchTarget::Main).unwrap();
    assert_eq!(&buffer[..layout.pre_data.len()], &layout.pre_data[..]);

    binding.retire();
    assert_eq!(binding.state(), BindingState::Retired);
    assert!(
        wait_for(Duration::from_secs(10), || binding.reclaim_eligible()),
        "retired binding never became reclaim-eligible"
    );
}

#[test]
fn mid_song_failure_fault_arms_the_producer_kill_and_silence_fill_follows() {
    let fixture = replay_fixture(false);
    let fault = FaultSelector::parse("mid-song-failure").unwrap();
    let binding = prepare_binding(
        5,
        1,
        50,
        true,
        &SourceView::new(&fixture),
        &fault,
        virtual_bank::StretchTarget::Main,
    )
    .expect("bind succeeds");
    // The fault leg lets the bind SUCCEED and kills the producer after the
    // armed block count — the real catch_unwind → SilenceFill containment.
    assert!(
        wait_for(Duration::from_secs(10), || binding.state()
            == BindingState::SilenceFill),
        "producer death never flipped the binding to silence-fill"
    );
    binding.retire();
}

// ── T4: registry publish/retire/sweep protocol ───────────────────────

#[test]
fn registry_reclaims_only_at_quiescence_after_the_cooldown() {
    let fixture = replay_fixture(false);
    let registry = BindingRegistry::new();
    let binding = prepare_binding(
        5,
        9,
        50,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    )
    .expect("bind succeeds");
    registry.publish(binding);
    assert_eq!(registry.active_generation(), Some(9));

    // A reader inside the epoch guard (as the detours will be) blocks
    // reclamation entirely.
    let entered = registry
        .with_active(|binding| {
            binding.reader_enter();
            true
        })
        .unwrap_or(false);
    assert!(entered);

    assert!(registry.retire_by_file(5));
    assert_eq!(registry.active_generation(), None);
    assert_eq!(registry.retired_count(), 1);

    let reclaimed = AtomicUsize::new(0);
    for _ in 0..4 {
        registry.sweep(|_, _| {
            reclaimed.fetch_add(1, Ordering::AcqRel);
        });
    }
    assert_eq!(
        reclaimed.load(Ordering::Acquire),
        0,
        "a held reader must block reclamation"
    );

    // Reader leaves; the producer thread also has to be done before the
    // Arc-drop actually frees, but the REGISTRY's reclamation contract is
    // only Retired ∧ readers == 0 ∧ cooldown elapsed.
    let exited = registry
        .with_retired(5, |binding| {
            binding.reader_exit();
            true
        })
        .unwrap_or(false);
    assert!(exited);

    let mut reported = Vec::new();
    registry.sweep(|generation, _| reported.push(generation));
    assert!(
        reported.is_empty(),
        "the first eligible sweep only counts the cooldown down"
    );
    registry.sweep(|generation, metrics| {
        reported.push(generation);
        // Metrics are read before the buffers drop (the drain logs them).
        let _ = metrics.frames_produced;
    });
    assert_eq!(reported, vec![9], "freed exactly once, metrics reported");
    assert_eq!(registry.retired_count(), 0);
    registry.sweep(|generation, _| reported.push(generation));
    assert_eq!(reported, vec![9], "a freed slot never reports again");
}

#[test]
fn registry_retire_cancels_an_armed_pending_read_with_clamp_semantics() {
    let fixture = replay_fixture(false);
    let registry = BindingRegistry::new();
    let binding = prepare_binding(
        5,
        11,
        50,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    )
    .expect("bind succeeds");
    let layout_end = binding.layout().virtual_size;
    registry.publish(binding);

    // Arm a read far past anything produced yet (just below EOF so the
    // clamp cannot zero it synchronously).
    let mut buffer = vec![0u8; 64];
    let mut accumulator = 0u64;
    let outcome = registry
        .with_active(|binding| unsafe {
            binding.serve(layout_end - 64, 64, buffer.as_mut_ptr(), &mut accumulator)
        })
        .expect("binding is active");
    // The tail may already be produced (tiny fixture, fast producer):
    // Served is legal; Pending is the leg under test.
    if matches!(outcome, ServeOutcome::Pending) {
        assert!(registry.retire_by_file(5));
        // Retirement cancelled the armed slot with the EOF-clamp semantics
        // (0-byte completion permitted) — the poll must complete, not hang.
        let completed = registry
            .with_retired(5, |binding| {
                wait_for(Duration::from_secs(10), || {
                    matches!(
                        unsafe { binding.poll(&mut accumulator) },
                        PollOutcome::Complete(_)
                    )
                })
            })
            .unwrap_or(false);
        assert!(completed, "cancelled read never completed");
    } else {
        assert!(registry.retire_by_file(5));
    }
    assert_eq!(registry.retired_count(), 1);
}

#[test]
fn registry_refusal_mailbox_coalesces_for_the_drain() {
    let registry = BindingRegistry::new();
    assert_eq!(registry.take_refusal(), None);
    registry.note_refusal(BindRefusal::UnsupportedProfile, 5);
    registry.note_refusal(BindRefusal::Plan, 6);
    let (refusal, file_id, count) = registry.take_refusal().expect("mailbox has a note");
    // Coalescing keeps the LAST refusal's identity and the total count —
    // one bounded WARN per drain tick.
    assert_eq!(refusal, BindRefusal::Plan);
    assert_eq!(file_id, 6);
    assert_eq!(count, 2);
    assert_eq!(registry.take_refusal(), None);
}

#[test]
fn plan_passes_the_side_entry_through_verbatim() {
    // Preview passthrough (step05-fix v2, maintainer-approved 2026-08-10):
    // only the MAIN entry is stretched; the non-main (preview) entry keeps
    // its STOCK header values so its bytes serve verbatim from the
    // resident source — bank prepare never waits on DSP.
    for preview_first in [false, true] {
        let fixture = replay_fixture(preview_first);
        let bank = xwb::parse_song_bank(&fixture).unwrap();
        let layout =
            virtual_bank::plan_virtual_bank(&bank, 25, virtual_bank::StretchTarget::Main).unwrap();
        let main = layout.main_entry_index;
        let side = 1 - main;
        // Main: stretched (25% quadruples the frame count).
        assert_eq!(
            layout.entries[main].streamed.duration,
            bank.entries[main].duration * 4,
            "order {preview_first}"
        );
        // Side: verbatim stock values, identity rate.
        assert_eq!(
            layout.entries[side].streamed.duration,
            bank.entries[side].duration
        );
        assert_eq!(
            layout.entries[side].streamed.data_len,
            bank.entries[side].data.len()
        );
        assert_eq!(
            layout.entries[side].streamed.loop_start,
            bank.entries[side].loop_start
        );
        assert_eq!(
            layout.entries[side].streamed.loop_length,
            bank.entries[side].loop_length
        );
        assert_eq!(
            layout.entries[side].rate,
            crate::core::xact::rate::RateRatio::IDENTITY
        );
        assert!(layout.entries[side].loop_context.is_none());
    }
}

// ── Identity passthrough serving (training mode Step 1) ──────────────

use super::generator_tests::{make_identity_binding, remap_main_entry};
use crate::core::xact::rate::RateRatio;

/// Serve the RE-pinned engine read pattern against an identity binding:
/// every read must complete synchronously — no producer thread exists to
/// complete a deferral.
fn serve_identity_file(binding: &Binding) -> Vec<u8> {
    let virtual_size = binding.layout().virtual_size;
    let mut file = vec![0u8; usize::try_from(virtual_size).expect("virtual size fits")];
    let mut accumulator = 0u64;
    let mut serve_at = |file: &mut [u8], offset: u64, len: u32| -> u32 {
        let dest = file[offset as usize..].as_mut_ptr();
        match unsafe { binding.serve(offset, len, dest, &mut accumulator as *mut u64) } {
            ServeOutcome::Served(served) => {
                assert_eq!(accumulator, u64::from(served));
                accumulator = 0;
                served
            }
            other => panic!("identity serving must complete synchronously, got {other:?}"),
        }
    };

    let header = serve_at(&mut file, 0, 0x1000);
    assert_eq!(header, 0x1000, "the header read completes in full");

    for entry in 0..2 {
        let data_len = binding.layout().entries[entry].streamed.data_len as u64;
        let block_align = u64::from(binding.entry_format(entry).block_align());
        let packet = 65_536 / block_align * block_align;
        let mut cursor = 0u64;
        while cursor < data_len {
            let request = packet.min(data_len - cursor) as u32;
            let offset = binding.layout().entry_offsets[entry] + cursor;
            let served = serve_at(&mut file, offset, request);
            assert_eq!(served, request, "in-stream packet reads serve in full");
            cursor += u64::from(served);
        }
    }

    // The defensive EOF read: the stock clamp serves zero bytes.
    let mut past = [0u8; 16];
    match unsafe {
        binding.serve(
            virtual_size,
            0x1000,
            past.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    } {
        ServeOutcome::Served(0) => {}
        other => panic!("EOF read must serve zero bytes synchronously, got {other:?}"),
    }

    file
}

#[test]
fn identity_passthrough_serves_the_stock_bank_byte_for_byte() {
    for preview_first in [false, true] {
        let fixture = replay_fixture(preview_first);
        let binding = make_identity_binding(fixture.clone());
        assert_eq!(binding.rate(), RateRatio::IDENTITY);
        let file = serve_identity_file(&binding);
        assert!(
            file == fixture,
            "identity serving must be byte-identical to stock (preview_first {preview_first})"
        );
        assert_eq!(
            binding.metrics_snapshot().deferral_count,
            0,
            "identity serving never defers for production"
        );
    }
}

#[test]
fn identity_mapping_serves_silent_lead_shifted_content_and_silent_tail() {
    let fixture = replay_fixture(false);
    let binding = make_identity_binding(fixture.clone());
    const SHIFT_BLOCKS: u64 = 4;
    const LEAD_BLOCKS: u64 = 3;
    // Bind-time pre-shift (R15): the mapping lands before the first byte is
    // ever served.
    assert!(binding.set_content_mapping(SHIFT_BLOCKS, LEAD_BLOCKS));
    assert_eq!(binding.content_mapping(), (SHIFT_BLOCKS, LEAD_BLOCKS));

    let file = serve_identity_file(&binding);
    let main = binding.layout().main_entry_index;
    let offset = binding.layout().entry_offsets[main] as usize;
    let len = binding.layout().entries[main].streamed.data_len;
    let mut expected = fixture.clone();
    let reference = remap_main_entry(
        &fixture[offset..offset + len],
        binding.entry_format(main),
        SHIFT_BLOCKS,
        LEAD_BLOCKS,
    );
    expected[offset..offset + len].copy_from_slice(&reference);
    assert!(
        file == expected,
        "mapped serving diverges from the lead/content/tail reference"
    );
    assert_eq!(binding.metrics_snapshot().deferral_count, 0);
}

#[test]
fn content_mapping_rejects_out_of_range_values() {
    let binding = make_identity_binding(replay_fixture(false));
    assert!(!binding.set_content_mapping(u64::from(u32::MAX) + 1, 0));
    assert!(!binding.set_content_mapping(0, u64::from(u32::MAX) + 1));
    assert_eq!(
        binding.content_mapping(),
        (0, 0),
        "a rejected mapping must change nothing"
    );
}

// ── Identity preflight + mapping API (training mode Step 1) ──────────

#[test]
fn identity_percent_preflight_builds_a_passthrough_binding_with_no_producer() {
    // The training identity arm's bind: percent 100 plans through
    // `plan_identity_bank` and constructs an IdentityPassthrough binding —
    // NO producer thread is spawned (nothing to synthesize).
    let fixture = replay_fixture(false);
    let binding = prepare_binding(
        5,
        13,
        100,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    )
    .expect("identity preflight succeeds");
    assert_eq!(
        binding.serve_mode(),
        super::binding::ServeMode::IdentityPassthrough
    );
    assert_eq!(binding.rate(), RateRatio::IDENTITY);
    // Serving is immediate and synchronous — with no producer, a deferral
    // would hang forever.
    let mut buffer = vec![0u8; 0x1000];
    let mut accumulator = 0u64;
    let outcome = unsafe { binding.serve(0, 0x1000, buffer.as_mut_ptr(), &mut accumulator) };
    assert_eq!(outcome, ServeOutcome::Served(0x1000));
    assert_eq!(&buffer[..], &fixture[..0x1000]);
    assert_eq!(binding.metrics_snapshot().deferral_count, 0);
    // No producer exists: frames_produced can never move.
    assert_eq!(binding.metrics_snapshot().frames_produced, 0);
    binding.retire();
}

#[test]
fn identity_bind_refused_fault_leg_refuses_only_identity_binds() {
    let fixture = replay_fixture(false);
    let view = SourceView::new(&fixture);
    let fault = FaultSelector::parse("identity-bind-refused").expect("selector parses");
    // The identity arm's bind refuses (→ EarlyFailed at the caller)...
    let result = prepare_binding(
        5,
        1,
        100,
        true,
        &view,
        &fault,
        virtual_bank::StretchTarget::Main,
    );
    assert!(matches!(result, Err(BindRefusal::Injected)));
    // ...while a rate bind under the same selector is untouched.
    let binding = prepare_binding(
        5,
        2,
        50,
        true,
        &view,
        &fault,
        virtual_bank::StretchTarget::Main,
    )
    .expect("rate bind unaffected");
    binding.retire();
}

/// The song-select preview shape (preview design §Components 2):
/// `prepare_binding(.., StretchTarget::Side)` takes its rate from the SIDE
/// entry's plan and its spawned producer streams that entry — the first
/// target packet, pumped through poll, is byte-identical to the Side
/// oracle's.
#[test]
fn prepare_binding_side_target_streams_the_side_entry() {
    let fixture = replay_fixture(false);
    let oracle =
        transform_bank_oracle_target(&fixture, 50, true, virtual_bank::StretchTarget::Side);
    let binding = prepare_binding(
        5,
        21,
        50,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Side,
    )
    .expect("side-target preflight succeeds");

    // Rate wiring: the binding's rate is the SIDE entry's plan rate.
    let bank = xwb::parse_song_bank(&fixture).expect("fixture parses");
    let plan = virtual_bank::plan_virtual_bank(&bank, 50, virtual_bank::StretchTarget::Side)
        .expect("side plan");
    assert_eq!(plan.target_entry_index, 1 - plan.main_entry_index);
    assert_eq!(binding.rate(), plan.entries[plan.target_entry_index].rate);

    // The spawned producer serves the target (side) entry's first packet;
    // compare against the oracle's bytes at the same virtual offset.
    let target = binding.layout().target_entry_index;
    let offset = binding.layout().entry_offsets[target];
    let block = u32::from(binding.entry_format(target).block_align());
    let packet = block * 4;
    let mut buffer = vec![0u8; packet as usize];
    let mut accumulator = 0u64;
    let served = match unsafe {
        binding.serve(
            offset,
            packet,
            buffer.as_mut_ptr(),
            &mut accumulator as *mut u64,
        )
    } {
        ServeOutcome::Served(served) => {
            accumulator = 0;
            served
        }
        ServeOutcome::Pending => {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match unsafe { binding.poll(&mut accumulator as *mut u64) } {
                    PollOutcome::Complete(bytes) => break u32::try_from(bytes).expect("fits"),
                    PollOutcome::Incomplete => {
                        assert!(Instant::now() < deadline, "spawned producer stalled");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    PollOutcome::NotPending => panic!("pending read vanished"),
                }
            }
        }
        ServeOutcome::Refused => panic!("refused while active"),
    };
    assert_eq!(served, packet);
    assert_eq!(
        &buffer[..],
        &oracle[offset as usize..(offset + u64::from(packet)) as usize],
        "the served side-entry packet must match the Side oracle"
    );
    binding.retire();
}

/// A cheap real binding for registry slot tests: the identity
/// passthrough (no producer thread), file id per caller.
fn identity_binding_for(file_id: i32, generation: u64) -> Arc<Binding> {
    let fixture = replay_fixture(false);
    let bank = xwb::parse_song_bank(&fixture).expect("fixture parses");
    let layout = virtual_bank::plan_identity_bank(&bank).expect("identity plan");
    drop(bank);
    Arc::new(
        Binding::new_identity_passthrough(file_id, generation, layout, fixture.into_boxed_slice())
            .expect("identity binding constructs"),
    )
}

/// Preview-slot lifecycle (preview design §Components 2, AC1): publish /
/// with_preview / replace-retires-previous / force-retire.
#[test]
fn preview_slot_publish_replace_and_force_retire() {
    let registry = BindingRegistry::new();
    assert!(registry.with_preview(|_| ()).is_none());
    assert!(!registry.retire_preview(), "empty slot retires nothing");

    registry.publish_preview(identity_binding_for(7, 1));
    assert_eq!(registry.with_preview(Binding::file_id), Some(7));
    assert_eq!(registry.retired_count(), 0);

    // Replacement retires the previous preview binding.
    registry.publish_preview(identity_binding_for(7, 2));
    assert_eq!(registry.with_preview(Binding::generation), Some(2));
    assert_eq!(registry.retired_count(), 1);

    // Force-retire empties the slot (the scene-exit defense).
    assert!(registry.retire_preview());
    assert!(registry.with_preview(|_| ()).is_none());
    assert_eq!(registry.retired_count(), 2);
    assert!(!registry.retire_preview(), "second force-retire is a no-op");
}

/// Both-slot `retire_by_file` coverage (AC2): the unregister prelude
/// retires preview bindings with no new call sites.
#[test]
fn retire_by_file_covers_both_slots() {
    let registry = BindingRegistry::new();
    registry.publish(identity_binding_for(5, 1));
    registry.publish_preview(identity_binding_for(7, 2));

    // A miss touches neither slot.
    assert!(!registry.retire_by_file(9));
    assert_eq!(registry.with_active(Binding::file_id), Some(5));
    assert_eq!(registry.with_preview(Binding::file_id), Some(7));

    // The preview file retires ONLY the preview binding.
    assert!(registry.retire_by_file(7));
    assert_eq!(registry.with_active(Binding::file_id), Some(5));
    assert!(registry.with_preview(|_| ()).is_none());

    // The active file retires the active binding.
    assert!(registry.retire_by_file(5));
    assert!(registry.with_active(|_| ()).is_none());
    assert_eq!(registry.retired_count(), 2);
}

/// Routing order + the detours' fast gate (AC3): active first, preview on
/// miss, `None` for unbound files; `any_bound` truth table.
#[test]
fn bound_for_file_routes_active_first_then_preview() {
    let registry = BindingRegistry::new();
    assert!(!registry.any_bound());
    assert!(registry.with_bound_for_file(5, |_| ()).is_none());

    registry.publish(identity_binding_for(5, 1));
    assert!(registry.any_bound());
    registry.publish_preview(identity_binding_for(7, 2));
    assert!(registry.any_bound());

    assert_eq!(
        registry.with_bound_for_file(5, Binding::generation),
        Some(1),
        "file 5 resolves via the ACTIVE slot"
    );
    assert_eq!(
        registry.with_bound_for_file(7, Binding::generation),
        Some(2),
        "file 7 resolves via the PREVIEW slot"
    );
    assert!(registry.with_bound_for_file(9, |_| ()).is_none());

    assert!(registry.retire_by_file(5));
    assert!(registry.any_bound(), "preview keeps the gate open");
    assert!(registry.retire_by_file(7));
    assert!(!registry.any_bound());
}

/// The full preview cycle (AC1/AC4): publish a Side-target binding, serve
/// through the routing surface, retire via the unregister path, sweep
/// reclaims exactly once.
#[test]
fn preview_cycle_publish_serve_retire_sweep() {
    let registry = BindingRegistry::new();
    let fixture = replay_fixture(false);
    let stock = xwb::parse_song_bank(&fixture).expect("stock parse");
    let binding = prepare_binding(
        7,
        binding_generation_for_test(),
        50,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Side,
    )
    .expect("side-target preflight succeeds");
    registry.publish_preview(binding);

    // Serve the verbatim MAIN entry's first packet through the routing
    // surface (synchronous — no producer involvement needed).
    let (main, main_offset) = registry
        .with_preview(|binding| {
            let main = binding.layout().main_entry_index;
            (main, binding.layout().entry_offsets[main])
        })
        .expect("preview live");
    let read_len = 2_048u32;
    let mut buffer = vec![0u8; read_len as usize];
    let mut accumulator = 0u64;
    let served = registry
        .with_bound_for_file(7, |binding| unsafe {
            binding.serve(
                main_offset,
                read_len,
                buffer.as_mut_ptr(),
                &mut accumulator as *mut u64,
            )
        })
        .expect("read routed to the preview binding");
    assert_eq!(served, ServeOutcome::Served(read_len));
    assert_eq!(&buffer[..], &stock.entries[main].data[..read_len as usize]);

    // Natural teardown: the unregister prelude's retire, then the drain
    // sweep (cooldown re-polls first, then frees exactly once).
    assert!(registry.retire_by_file(7));
    assert!(!registry.any_bound());
    let mut reports = 0;
    for _ in 0..8 {
        registry.sweep(|_, _| reports += 1);
        if registry.retired_count() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(reports, 1, "the preview binding reclaims exactly once");
    assert_eq!(registry.retired_count(), 0);
}

/// Monotonic preview identities for the cycle test (and R15's contract).
fn binding_generation_for_test() -> u64 {
    let first = next_preview_generation();
    let second = next_preview_generation();
    assert!(first >= 1 && second > first, "preview generations ascend");
    second
}

/// The preview mailbox never masks the gameplay one, and vice versa (AC5).
#[test]
fn preview_refusal_mailbox_is_independent() {
    let registry = BindingRegistry::new();
    assert!(registry.take_preview_refusal().is_none());

    registry.note_refusal(BindRefusal::Plan, 5);
    registry.note_preview_refusal(BindRefusal::SourceRead, 7);
    registry.note_preview_refusal(BindRefusal::UnsupportedProfile, 8);

    let (refusal, file, count) = registry.take_refusal().expect("gameplay mailbox");
    assert_eq!((refusal, file, count), (BindRefusal::Plan, 5, 1));
    let (refusal, file, count) = registry.take_preview_refusal().expect("preview mailbox");
    assert_eq!(
        (refusal, file, count),
        (BindRefusal::UnsupportedProfile, 8, 2)
    );

    // Both drained: empty until the next note.
    assert!(registry.take_refusal().is_none());
    assert!(registry.take_preview_refusal().is_none());
}

#[test]
fn registry_mapping_api_targets_the_live_binding_or_fails_open() {
    let registry = BindingRegistry::new();
    // No live binding: the call reports false and changes nothing (the
    // caller — song_reset's seek — falls back per the design's R6).
    assert!(!registry.set_active_content_mapping(4, 3));

    let fixture = replay_fixture(false);
    let binding = prepare_binding(
        5,
        17,
        100,
        true,
        &SourceView::new(&fixture),
        &FaultSelector::default(),
        virtual_bank::StretchTarget::Main,
    )
    .expect("identity preflight succeeds");
    registry.publish(binding);
    assert!(registry.set_active_content_mapping(4, 3));
    let applied = registry
        .with_active(|binding| binding.content_mapping())
        .expect("binding is live");
    assert_eq!(applied, (4, 3));
    // Out-of-range values fail open without touching the mapping.
    assert!(!registry.set_active_content_mapping(u64::from(u32::MAX) + 1, 0));
    let applied = registry
        .with_active(|binding| binding.content_mapping())
        .expect("binding is live");
    assert_eq!(applied, (4, 3));
    assert!(registry.retire_by_file(5));
}
