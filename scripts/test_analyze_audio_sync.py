"""Host-only report tests; fixtures contain no game assets or personal data."""
import csv
import io
import unittest
import re
from pathlib import Path

import analyze_audio_sync as report


def capture(version, records, comments=()):
    output = io.StringIO()
    output.write(f"# audio-sync/v{version} qpc_frequency=1000000 channels=127\n")
    columns = report.V1_COLUMNS + (report.V2_COLUMNS if version == 2 else [])
    writer = csv.DictWriter(output, fieldnames=columns)
    writer.writeheader()
    for record in records:
        writer.writerow(record)
    for comment in comments:
        output.write(comment + "\n")
    output.seek(0)
    return report.analyze(output)


def row(kind, qpc, **kw):
    return dict(kind=kind, qpc=qpc, attempt=1, scene=28,
                context_valid="true", side=0, **kw)


class CaptureTests(unittest.TestCase):
    def test_schema_and_scopes_match_production_declarations(self):
        root = Path(__file__).resolve().parents[1]
        model = (root / "src/services/audio_sync_diag/model.rs").read_text()
        declaration = re.search(r'pub const CSV_COLUMNS: &str = "([^"]+)";', model)
        assert declaration is not None
        columns = declaration.group(1)
        self.assertEqual(report.V1_COLUMNS + report.V2_COLUMNS, columns.split(","))
        spans = (root / "src/services/audio_sync_diag/spans.rs").read_text()
        declaration = re.search(r'pub enum Scope\s*\{([^}]+)\}', spans)
        assert declaration is not None
        body = declaration.group(1)
        scopes = {int(value): name for name, value in re.findall(r'(\w+)\s*=\s*(\d+)', body)}
        self.assertEqual(report.SCOPES, scopes)

    def test_v1_retained_rows_are_not_frame_count(self):
        result = capture(1, [row("frame", 100, counter0=1),
                             row("frame", 1000100, counter0=121)])
        self.assertEqual(result["version"], 1)
        self.assertEqual(result["attempts"][0]["frame_hz"], 120)
        self.assertEqual(result["judgements"], [])
        self.assertEqual(result["voices"], [])

    def test_hit_population_excludes_miss_ok_predead_and_unknown(self):
        hits = []
        for i, (grade, error, dead) in enumerate([(0, -2, 0), (1, 4, 0),
                (5, 180, 0), (6, None, 0), (1, 20, 1), (0, 0, None)]):
            valid = 1 | 4 | (2 if error is not None else 0) | (16 if dead is not None else 0)
            hits.append(row("judgement", 100+i, detail_valid=valid,
                            detail0=grade, detail1=error, detail2=1000+i,
                            detail4=dead))
        value = capture(2, hits)["judgements"][0]
        self.assertEqual(value["records"], 6)
        self.assertEqual(value["timing_eligible"]["count"], 2)
        self.assertEqual(value["timing_eligible"]["mean_ms"], 1)
        self.assertEqual(value["unknown_dead_state"], 1)

    def test_scope_summaries_are_cumulative_not_additive(self):
        result = capture(2, [
            row("span_summary", 1000, end_qpc=1500, id=2,
                observations=2, counter1=750, counter2=1, counter3=0),
            row("span_summary", 1000, end_qpc=1500, id=2,
                observations=3, counter1=1000, counter2=2, counter3=1)])
        scope = result["scopes"][0]
        self.assertEqual(scope["observations"], 3)
        self.assertAlmostEqual(scope["mean_ms"], 1/3)
        self.assertEqual(scope["max_ms"], .5)
        self.assertEqual(scope["suppressed_examples"], 1)

    def test_submillisecond_examples_retain_precision(self):
        result = capture(2, [row("span", 1000, end_qpc=1250, id=4, trace_id=7)])
        self.assertEqual(result["slow_examples"][0]["duration_ms"], .25)

    def test_game_clock_fit_requires_same_valid_domain_and_accounts_for_rate(self):
        records = [row("gameplay_sample", 100 + i * 1000000, actor=7, anchor=99,
                       valid=63, scene_epoch=1, segment=1, judge_mc=5000+i*2000,
                       rate_q31=4294967296, sound_ms=0, input_ms=0, render_ms=0,
                       bomb_frames=0, option_ms=0) for i in range(3)]
        records.append(row("gameplay_sample", 5000100, actor=7, anchor=100,
                           valid=1, scene_epoch=1, segment=2, judge_mc=50000,
                           rate_q31=4294967296))
        result = capture(1, records)["game_clock_segments"]
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["samples"], 3)
        self.assertAlmostEqual(result[0]["error_vs_committed_rate_ppm"], 0)

    def test_voice_join_requires_explicit_origin_and_handle(self):
        records = [row("prepare_request", 100, end_qpc=110, id=3, result=5, name="song"),
                   row("start_request", 200, end_qpc=250, id=3),
                   row("voice_start", 230, end_qpc=240, id=3, result=0,
                       trace_id=9, origin_attempt=1, origin_scene=28, origin_known="true",
                       detail_valid=128, detail7=1),
                   row("voice_start", 231, end_qpc=241, id=3, result=0,
                       trace_id=0, origin_known="false", detail_valid=128, detail7=1)]
        voices = capture(2, records)["voices"]
        self.assertEqual(voices[0]["request_to_submission_ms"], [.03, .04])
        self.assertIsNone(voices[1]["request_to_submission_ms"])
        self.assertFalse(voices[1]["matched"])

    def test_reused_handle_does_not_join_a_later_prepare_to_old_start(self):
        records = [row("prepare_request", 100, end_qpc=110, id=3, result=5),
                   row("start_request", 200, end_qpc=210, id=3),
                   row("prepare_request", 300, end_qpc=310, id=3, result=5),
                   row("voice_start", 330, end_qpc=340, id=3, result=0,
                       trace_id=9, origin_attempt=1, origin_scene=28, origin_known="true",
                       detail_valid=128, detail7=1)]
        self.assertIsNone(capture(2, records)["voices"][0]["request_to_submission_ms"])

    def test_cursor_identity_and_continuity_bound_segments(self):
        def cursor(qpc, played, buffer=2, continuous=True):
            return row("output_cursor", qpc, end_qpc=qpc+10, result=0,
                       detail_valid=255 | (256 if continuous else 0),
                       **{f"detail{i}": v for i, v in enumerate(
                           [1, buffer, 0, 4, played, 16000, 1000, 4])})
        result = capture(2, [cursor(100, 0, continuous=False), cursor(1000100, 4000),
                             cursor(2000100, 8000), cursor(3000100, 12000, buffer=3)])
        spans = result["output_cursor_segments"]
        self.assertEqual(len(spans), 1)
        self.assertEqual(spans[0]["samples"], 3)
        self.assertAlmostEqual(spans[0]["cursor_minus_qpc_ms"], 0)
        self.assertNotIn("audible_start", spans[0])

    def test_engine_failure_and_losses_are_reported(self):
        result = capture(2, [row("engine_status", 100, result=3)],
                         ["# loss qpc=200 full=2 contention=3 channels=255",
                          "# loss qpc=300 full=2 contention=4 channels=511"])
        self.assertEqual(result["losses"]["full"], 2)
        self.assertEqual(result["losses"]["contention"], 4)
        self.assertEqual(result["channels"], 511)
        self.assertTrue(any("missed" in w for w in result["warnings"]))

    def test_cursor_rate_fit_uses_all_samples_not_only_endpoints(self):
        samples = [row("output_cursor", 100 + second * 1000000,
                       end_qpc=110 + second * 1000000, result=0,
                       detail_valid=255 | (256 if second else 0),
                       **{f"detail{i}": v for i, v in enumerate(
                           [1, 2, 0, 4, played, 16000, 1000, 4])})
                   for second, played in enumerate([0, 4000, 8000, 12000, 16040])]
        segment = capture(2, samples)["output_cursor_segments"][0]
        self.assertAlmostEqual(segment["cursor_rate_error_ppm"], 2000)
        self.assertAlmostEqual(segment["cursor_minus_qpc_ms"], 10)

    def test_audio_clock_onsets_are_decoded_and_summarised(self):
        def onset(qpc, delta_micro, generation):
            return row("onset", qpc, end_qpc=qpc - 5000, id=44100, result=0,
                       detail_valid=255, counter0=1000, counter1=310, counter2=55100,
                       counter3=generation,
                       **{f"detail{i}": v for i, v in enumerate(
                           [500000, 500000, 497795, 498236, qpc - 5000, 1764, 441, delta_micro])})
        result = capture(2, [onset(1000000, 3200, 1), onset(2000000, -4100, 2)])
        onsets = result["audio_clock_onsets"]
        self.assertEqual([o["generation"] for o in onsets], [1, 2])
        self.assertAlmostEqual(onsets[0]["delta_vs_stock_ms"], 3.2)
        self.assertAlmostEqual(onsets[1]["delta_vs_stock_ms"], -4.1)
        self.assertAlmostEqual(onsets[0]["lead_ms"], 40.0)
        self.assertAlmostEqual(onsets[0]["margin_ms"], 10.0)
        self.assertAlmostEqual(onsets[0]["c_ms"], 55.1)
        self.assertAlmostEqual(onsets[0]["fit_resid_sd_ms"], 0.31)
        self.assertEqual(result["audio_clock_summary"]["arms"], 2)
        # A capture without arms reports None, never an empty distribution.
        self.assertIsNone(capture(2, [])["audio_clock_summary"])

    def test_malformed_and_future_formats_are_not_silently_accepted(self):
        with self.assertRaises(ValueError):
            report.analyze(io.StringIO("# audio-sync/v3 qpc_frequency=1000\na,b\n1,2\n"))
        with self.assertRaises(ValueError):
            report.analyze(io.StringIO("# audio-sync/v1 qpc_frequency=1000\n" +
                                      ",".join(report.V1_COLUMNS) + "\nframe,1\n"))


if __name__ == "__main__":
    unittest.main()
