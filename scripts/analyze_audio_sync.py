#!/usr/bin/env python3
"""Summarize v1/v2 timing captures without inferring physical audio or pad timing."""
import argparse
import csv
import json
import re
import statistics
from collections import Counter, defaultdict
from typing import Any

V1_COLUMNS = ("kind,qpc,end_qpc,attempt,scene_epoch,scene,context_valid,id,result,side,name,"
              "valid,actor,anchor,frame_tick,judge_mc,stored_mc,sound_ms,input_ms,render_ms,"
              "bomb_frames,option_ms,rate_q31,segment,observations,max_gap_qpc,counter0,"
              "counter1,counter2,counter3,progress_wall_ms,progress_mc").split(",")
V2_COLUMNS = ("trace_id,parent_id,thread_id,origin_attempt,origin_scene,origin_known,"
              "detail_valid,detail0,detail1,detail2,detail3,detail4,detail5,detail6,detail7").split(",")
SCOPES = dict(enumerate([
    "Frame", "Poll", "Jobs", "Job", "LayerOriginal", "Topmost", "Judge", "JudgePre",
    "JudgeOriginal", "JudgePost", "JudgePreCallback", "JudgePostCallback", "FrameCallback",
    "InputCallback", "InputExclusive", "SubmitPre", "SubmitOriginal", "SubmitPost",
    "JudgeSample", "HitSample", "FrameSample", "Submit"], 1))
ENGINE_STATUS = {0: "unavailable", 1: "armed", 2: "installed", 3: "missed window",
                 4: "unsupported engine", 5: "install failed", 6: "factory unavailable",
                 7: "manager unavailable"}


def integer(value):
    return int(value, 16 if str(value).startswith("0x") else 10)


def detail(row, index):
    return row.get(f"detail{index}") if (row.get("detail_valid") or 0) & (1 << index) else None


def interval(row, scale):
    begin, end = row.get("qpc"), row.get("end_qpc")
    return (end - begin) * scale if begin is not None and begin >= 0 and end is not None and end >= begin else None


def distribution(values):
    if not values:
        return {"count": 0}
    return {"count": len(values), "mean_ms": statistics.mean(values),
            "median_ms": statistics.median(values), "stddev_ms": statistics.pstdev(values),
            "min_ms": min(values), "max_ms": max(values)}


def line_fit(xs, ys):
    mx, my = statistics.mean(xs), statistics.mean(ys)
    variance = sum((x - mx) ** 2 for x in xs)
    if not variance:
        return None
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / variance
    return slope, [y - my - slope * (x - mx) for x, y in zip(xs, ys)]


def analyze(stream, chart_start_ms=None, chart_end_ms=None):
    first = stream.readline().strip()
    match = re.match(r"# audio-sync/v([12])\s", first)
    if not match:
        raise ValueError("unsupported or missing audio-sync/v1 or v2 header")
    version = int(match.group(1))
    metadata = dict(re.findall(r"(\w+)=(\S+)", first))
    frequency = integer(metadata.get("qpc_frequency", "0"))
    if frequency <= 0:
        raise ValueError("QPC frequency must be positive")
    scale = 1000 / frequency
    columns = next(csv.reader([stream.readline().strip()]))
    expected = V1_COLUMNS + (V2_COLUMNS if version == 2 else [])
    if columns != expected:
        raise ValueError("CSV columns do not match the declared schema")
    rows, losses, warnings = [], {}, []
    channels = integer(metadata.get("channels", "0"))
    for line_number, line in enumerate(stream, 3):
        if line.startswith("#"):
            if line.startswith("# loss"):
                fields = dict(re.findall(r"(\w+)=(\S+)", line))
                for key, value in fields.items():
                    losses[key] = max(losses.get(key, 0), integer(value))
                channels |= losses.get("channels", 0)
            continue
        if not line.strip():
            continue
        cells = next(csv.reader([line]))
        if len(cells) != len(columns):
            raise ValueError(f"line {line_number}: incomplete/malformed record ({len(cells)} columns)")
        row: dict[str, Any] = dict(zip(columns, cells))
        for key in columns:
            if key not in ("kind", "name", "context_valid", "origin_known"):
                try:
                    row[key] = (float(row[key]) if key == "progress_wall_ms" else integer(row[key])) if row[key] else None
                except ValueError as error:
                    raise ValueError(f"line {line_number}: invalid {key}") from error
        if row["qpc"] is None or row["qpc"] < 0:
            warnings.append(f"line {line_number}: invalid QPC; record omitted")
            continue
        row["line"] = line_number
        rows.append(row)
    groups = defaultdict(list)
    for row in rows:
        groups[row.get("attempt")].append(row)
    attempts = []
    for attempt, group in sorted(groups.items(), key=lambda item: item[0] or 0):
        frames = [r for r in group if r["kind"] == "frame" and r.get("scene") == 28 and r.get("counter0") is not None]
        anchors = [r for r in group if r["kind"] == "anchor_delivered" and r.get("scene") == 28 and r.get("result") == 1]
        if not frames and not anchors:
            continue
        frames.sort(key=lambda r: r["qpc"])
        if anchors:
            frames = [r for r in frames if r["qpc"] >= min(a["qpc"] for a in anchors)]
        hz = None
        if len(frames) >= 2 and frames[-1]["qpc"] > frames[0]["qpc"]:
            hz = (frames[-1]["counter0"] - frames[0]["counter0"]) * frequency / (frames[-1]["qpc"] - frames[0]["qpc"])
        paired = []
        for anchor in anchors:
            starts = [r for r in group if r["kind"] == "start_request" and r.get("scene") == 28 and r["qpc"] <= anchor["qpc"]]
            if starts:
                start = max(starts, key=lambda r: r["qpc"])
                paired.append((anchor["qpc"] - start["qpc"]) * scale)
        attempts.append({"attempt": attempt, "frame_hz": hz,
                         "anchor_start_nearest_ms": paired,
                         "note": "Nearest preceding game request is a software milestone, not an audio identity proof."})

    domains = defaultdict(list)
    for r in rows:
        if r["kind"] == "gameplay_sample" and r.get("context_valid") == "true" and (r.get("valid") or 0) & 35 == 35 and (r.get("judge_mc") or 0) >= 3000:
            key = tuple(r.get(k) for k in ("attempt", "scene_epoch", "segment", "actor", "anchor", "rate_q31", "sound_ms", "input_ms", "render_ms", "bomb_frames", "option_ms"))
            domains[key].append(r)
    game_clock_segments = []
    for key, samples in domains.items():
        samples.sort(key=lambda r: r["qpc"])
        if len(samples) < 2 or not key[5] or any(a["judge_mc"] > b["judge_mc"] for a, b in zip(samples, samples[1:])):
            continue
        xs = [(r["qpc"] - samples[0]["qpc"]) * scale for r in samples]
        ys = [r["judge_mc"] - samples[0]["judge_mc"] for r in samples]
        fitted = line_fit(xs, ys)
        if fitted is None:
            continue
        slope, residual = fitted
        game_clock_segments.append({"attempt": key[0], "segment": key[2], "samples": len(samples),
                                    "duration_s": xs[-1] / 1000, "rate_q31": key[5],
                                    "error_vs_committed_rate_ppm": (slope / (key[5] / 2**31) - 1) * 1e6,
                                    "fit_residual_ms": distribution(residual)})

    hit_groups = defaultdict(list)
    for r in rows:
        if r["kind"] != "judgement" or r.get("context_valid") != "true":
            continue
        note = detail(r, 2)
        if chart_start_ms is not None and (note is None or note < chart_start_ms):
            continue
        if chart_end_ms is not None and (note is None or note > chart_end_ms):
            continue
        hit_groups[(r.get("attempt"), r.get("side"))].append(r)
    judgements = []
    for (attempt, side), hits in sorted(hit_groups.items()):
        eligible = [detail(r, 1) for r in hits if detail(r, 0) in range(5)
                    and detail(r, 4) == 0 and detail(r, 1) is not None]
        judgements.append({"attempt": attempt, "side": side, "records": len(hits),
                           "grades": dict(Counter(detail(r, 0) for r in hits if detail(r, 0) is not None)),
                           "unknown_dead_state": sum(detail(r, 4) is None for r in hits),
                           "timing_eligible": distribution(eligible)})

    latest = {}
    examples = []
    for r in rows:
        duration = interval(r, scale)
        if r["kind"] == "span_summary" and duration is not None:
            previous = latest.get(r["id"])
            if previous is None or (r.get("observations") or 0) >= (previous.get("observations") or 0):
                latest[r["id"]] = r
        elif r["kind"] == "span" and duration is not None:
            examples.append({"scope": SCOPES.get(r["id"], str(r["id"])),
                             "duration_ms": duration, "line": r["line"], "trace_id": r.get("trace_id"),
                             "parent_id": r.get("parent_id"), "qpc": r["qpc"],
                             "callback_id": r.get("counter0"),
                             "origin_attempt": r.get("origin_attempt") if r.get("origin_known") == "true" else None})
    scopes = []
    for scope, r in sorted(latest.items()):
        count = r.get("observations") or 0
        scopes.append({"scope": SCOPES.get(scope, str(scope)), "observations": count,
                       "mean_ms": r["counter1"] * scale / count if count and r.get("counter1") is not None else None,
                       "max_ms": interval(r, scale), "max_qpc": r["qpc"],
                       "max_trace_id": r.get("trace_id"), "max_callback_id": r.get("counter0"),
                       "slow_count": r.get("counter2"), "suppressed_examples": r.get("counter3")})

    voices = []
    for voice in rows:
        if voice["kind"] != "voice_start":
            continue
        known = voice.get("origin_known") == "true" and bool(voice.get("trace_id"))
        origin = voice.get("origin_attempt") if known else None
        group = groups.get(origin, []) if known else []
        preparations = [r for r in group if r["kind"] == "prepare_request" and r.get("result") == 5
                        and r.get("id") == voice.get("id") and r["qpc"] <= voice["qpc"]]
        prep = max(preparations, key=lambda r: r["qpc"]) if preparations else None
        starts = [r for r in group if prep and r["kind"] == "start_request" and r.get("id") == voice.get("id")
                  and prep["qpc"] <= r["qpc"] <= voice["qpc"]]
        start = max(starts, key=lambda r: r["qpc"]) if starts else None
        envelope = None
        if start and detail(voice, 7) == 1 and interval(voice, scale) is not None:
            envelope = [(voice["qpc"] - start["qpc"]) * scale, (voice["end_qpc"] - start["qpc"]) * scale]
        voices.append({"line": voice["line"], "token": voice.get("trace_id"), "matched": known,
                       "origin_attempt": origin, "origin_scene": voice.get("origin_scene") if known else None,
                       "name": voice.get("name"), "handle": voice.get("id"), "branch": detail(voice, 7),
                       "request_to_submission_ms": envelope,
                       "pre_observer_ms": voice["counter0"] * scale if (voice.get("counter3") or 0) & 1 and voice.get("counter0") is not None else None})

    statuses = []
    for r in rows:
        if r["kind"] == "engine_status":
            status = ENGINE_STATUS.get(r.get("result"), "unknown status")
            statuses.append({"line": r["line"], "status": status, "critical_update_losses": detail(r, 2)})
            if r.get("result") in range(3, 8):
                warnings.append(f"XACT engine probes: {status}")

    segments, current = [], {}
    def finish(backend):
        samples = current.pop(backend, [])
        if len(samples) < 2:
            return
        first, last = samples[0], samples[-1]
        elapsed = (last[0] - first[0]) / frequency
        if elapsed <= 0:
            return
        audio_seconds = (last[1][4] - first[1][4]) / (last[1][6] * last[1][7])
        xs = [(stamp - first[0]) / frequency for stamp, _ in samples]
        ys = [(value[4] - first[1][4]) / (value[6] * value[7]) for _, value in samples]
        fitted = line_fit(xs, ys)
        if fitted is None:
            return
        slope, residual = fitted
        segments.append({"backend": backend, "samples": len(samples), "duration_s": elapsed,
                         "cursor_minus_qpc_ms": (audio_seconds - elapsed) * 1000,
                         "cursor_rate_error_ppm": (slope - 1) * 1e6,
                         "fit_residual_ms": distribution([r * 1000 for r in residual])})
    for r in sorted((r for r in rows if r["kind"] == "output_cursor"), key=lambda r: r["qpc"]):
        values = [detail(r, i) for i in range(8)]
        backend = values[0]
        if any(v is None for v in values) or r.get("result") == -1 or interval(r, scale) is None or not values[6] or not values[7]:
            finish(backend)
            continue
        previous = current.get(backend, [])
        identity = lambda v: (v[0], v[1], v[5], v[6], v[7])
        if not (r.get("detail_valid", 0) & 256) or (previous and
                (identity(previous[-1][1]) != identity(values) or values[4] < previous[-1][1][4])):
            finish(backend)
        midpoint = (r["qpc"] + r["end_qpc"]) / 2
        current.setdefault(backend, []).append((midpoint, values))
    for backend in list(current):
        finish(backend)

    # Deterministic audio clock arms (gameplay-timing-fixes): one row per arm.
    # detail0..7 = F0, W_k0, P_k0, Wc_k0, t_k0, lead_frames, margin_frames,
    # delta_vs_stock_micro_ms; counters = fit_n, fit_resid_sd_micro_ms,
    # C_micro_ms, voice_generation; id = Hz; result = content offset (wall ms).
    onsets = []
    for r in sorted((r for r in rows if r["kind"] == "onset"), key=lambda r: r["qpc"]):
        hz = r.get("id") or 0
        values = [detail(r, i) for i in range(8)]
        if not hz or any(v is None for v in values):
            continue
        per_frame_ms = 1000.0 / hz
        onsets.append({"line": r["line"], "generation": r.get("counter3"),
                       "f0": values[0], "lead_ms": values[5] * per_frame_ms,
                       "margin_ms": values[6] * per_frame_ms,
                       "delta_vs_stock_ms": values[7] / 1000.0,
                       "c_ms": (r.get("counter2") or 0) / 1000.0,
                       "offset_ms": r.get("result"),
                       "fit_n": r.get("counter0"),
                       "fit_resid_sd_ms": (r.get("counter1") or 0) / 1000.0})
    onset_summary = None
    if onsets:
        deltas = [o["delta_vs_stock_ms"] for o in onsets]
        onset_summary = {"arms": len(onsets), "delta_vs_stock_ms": distribution(deltas),
                         "fit_resid_sd_ms": distribution([o["fit_resid_sd_ms"] for o in onsets]),
                         "lead_plus_margin_ms": distribution([o["lead_ms"] + o["margin_ms"] for o in onsets])}
    for key in ("full", "contention", "span_contention", "invalid_spans", "sample_capacity", "context_contention"):
        if losses.get(key, 0):
            warnings.append(f"Diagnostic {key}: {losses[key]} (coverage incomplete)")
    if version == 1:
        warnings.append("v1 has no duration, individual-hit or engine-output channels")
    elif channels & sum(1 << bit for bit in range(17, 22)) != sum(1 << bit for bit in range(17, 22)):
        warnings.append("XACT engine channel metadata is incomplete; absence of events is not zero latency")
    return {"version": version, "metadata": metadata, "channels": channels,
            "records": len(rows), "kinds": dict(Counter(r["kind"] for r in rows)),
            "losses": losses, "warnings": warnings, "attempts": attempts, "judgements": judgements,
            "scopes": scopes, "slow_examples": sorted(examples, key=lambda e: e["duration_ms"], reverse=True)[:20],
            "voices": voices, "engine_status": statuses, "output_cursor_segments": segments,
            "game_clock_segments": game_clock_segments,
            "audio_clock_onsets": onsets, "audio_clock_summary": onset_summary,
            "limits": ["Spans are inclusive wall time, not GPU duration or pristine game CPU.",
                       "Span summaries are cumulative process totals; do not sum scopes or successive summaries.",
                       "Timing-eligible hits exclude Miss/OK/pre-dead/unknown state, but do not prove human input.",
                       "Mixed-output cursor is not per-song or physical presentation; midpoint timestamps have call-width uncertainty.",
                       "Cursor rate uses an all-sample linear fit; short segments and quantized/backend cursors can mislead.",
                       "Voice submission intervals enclose an internal Start call, not audible onset.",
                       "Slow examples and ordinary samples are decimated; final counters may not have flushed."]}


def text(result):
    out = [f"Audio-sync v{result['version']}: {result['records']} records, channels=0x{result['channels']:X}"]
    out.extend("WARNING: " + warning for warning in result["warnings"])
    for attempt in result["attempts"]:
        hz = attempt["frame_hz"]
        out.append(f"Attempt {attempt['attempt']}: frame-driver Hz={hz:.3f}" if hz is not None else f"Attempt {attempt['attempt']}: frame cadence unavailable")
    out.append("\nCumulative scope wall times (inclusive, non-additive):")
    for scope in result["scopes"]:
        mean = scope["mean_ms"]
        mean_text = f"{mean:.4f}" if mean is not None else "unavailable"
        out.append(f"  {scope['scope']}: n={scope['observations']} mean={mean_text}ms max={scope['max_ms']:.4f}ms suppressed_examples={scope['suppressed_examples']}")
    out.append("\nJudgement timing (engine-reported integer ms):")
    for hit in result["judgements"]:
        out.append(f"  Attempt {hit['attempt']} side {hit['side']}: grades={hit['grades']} eligible={hit['timing_eligible']} unknown_dead={hit['unknown_dead_state']}")
    out.append("\nXACT submission intervals relative to game start request:")
    for voice in result["voices"]:
        out.append(f"  line {voice['line']} token={voice['token']} attempt={voice['origin_attempt']} matched={voice['matched']} branch={voice['branch']} request_to_submission_ms={voice['request_to_submission_ms']}")
    out.append("\nMixed-output cursor segments:")
    out.extend("  " + str(segment) for segment in result["output_cursor_segments"])
    out.append("\nGame clock versus committed rate/QPC (chart time >=3 seconds):")
    out.extend("  " + str(segment) for segment in result["game_clock_segments"])
    out.append("\nDeterministic audio clock arms (delta_vs_stock = this play's stock onset error):")
    if result["audio_clock_summary"]:
        out.append("  summary: " + str(result["audio_clock_summary"]))
    out.extend("  " + str(onset) for onset in result["audio_clock_onsets"])
    out.append("\nLimits:")
    out.extend("  " + limit for limit in result["limits"])
    return "\n".join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--chart-start-ms", type=int)
    parser.add_argument("--chart-end-ms", type=int)
    args = parser.parse_args()
    if args.chart_start_ms is not None and args.chart_end_ms is not None and args.chart_start_ms > args.chart_end_ms:
        parser.error("chart start must not exceed chart end")
    try:
        with open(args.capture, encoding="utf-8") as stream:
            result = analyze(stream, args.chart_start_ms, args.chart_end_ms)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2) if args.json else text(result))


if __name__ == "__main__":
    main()
