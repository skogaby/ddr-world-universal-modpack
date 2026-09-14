//! Self-contained HTML report (inline CSS/JS, embedded JSON — works offline
//! from `file://`) plus a plain JSON dump.
//!
//! The page renders three views from the embedded data: a per-level overview
//! (curve constants, mean/median score, EX %, FC/MFC/S-MFC/fail rates,
//! judgement mix), level × difficulty heatmaps, and a filterable grid of
//! synthesized scorecards.

use crate::bot::skill::Curve;
use crate::scoring::Scorecard;

pub struct Meta {
    pub ssq_dir: String,
    pub chart_count: usize,
    pub file_count: usize,
    pub parse_errors: Vec<(String, String)>,
    pub seeds: u32,
    pub fps: u32,
    pub smarv_ms: i32,
    pub levels: Vec<u8>,
    pub curves: Vec<(u8, Curve)>,
    pub curve_override: bool,
    /// The effective constants (stock or overridden), for the console table.
    pub curve_description: String,
    pub elapsed_secs: f64,
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"), // never close the <script> block
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Compact per-card array (see the `COLS` legend in the JS):
/// `[chart, diff, level, seed, notes, taps, freezes, shocks, dur_ms,
///   marv, perf, great, good, miss, ok, ng, smarv, maxcombo, score, ex, exmax,
///   fc(-1..3), smfc(0/1), fast, slow, gauge_final, gauge_min, failed(0/1),
///   failed_at_ms(-1), rank, mismatches, [hist×25]]`
fn card_json(c: &Scorecard) -> String {
    let hist: Vec<String> = c.delta_hist.iter().map(|v| v.to_string()).collect();
    format!(
        "[{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},[{}]]",
        json_str(&c.chart),
        c.difficulty as u8,
        c.level,
        c.seed_idx,
        c.note_count,
        c.taps,
        c.freezes,
        c.shocks,
        c.duration_ms,
        c.counts[0],
        c.counts[1],
        c.counts[2],
        c.counts[3],
        c.counts[5],
        c.counts[6],
        c.counts[7],
        c.smarv,
        c.max_combo,
        c.score,
        c.ex,
        c.ex_max,
        c.fc.map(|f| f.code() as i32).unwrap_or(-1),
        u8::from(c.smfc),
        c.fast,
        c.slow,
        c.gauge_final,
        c.gauge_min,
        u8::from(c.failed),
        c.failed_at_ms.unwrap_or(-1),
        json_str(c.rank),
        c.mismatches,
        hist.join(",")
    )
}

fn meta_json(m: &Meta) -> String {
    let curves: Vec<String> = m
        .curves
        .iter()
        .map(|(l, c)| format!("[{},{:.3},{:.5}]", l, c.sigma_ms, c.p_miss))
        .collect();
    let errs: Vec<String> = m
        .parse_errors
        .iter()
        .map(|(n, e)| format!("[{},{}]", json_str(n), json_str(e)))
        .collect();
    format!(
        "{{\"ssq_dir\":{},\"chart_count\":{},\"file_count\":{},\"parse_errors\":[{}],\"seeds\":{},\"fps\":{},\"smarv_ms\":{},\"levels\":[{}],\"curves\":[{}],\"curve_override\":{},\"elapsed_secs\":{:.1},\"generated\":{}}}",
        json_str(&m.ssq_dir),
        m.chart_count,
        m.file_count,
        errs.join(","),
        m.seeds,
        m.fps,
        m.smarv_ms,
        m.levels.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(","),
        curves.join(","),
        m.curve_override,
        m.elapsed_secs,
        json_str(&timestamp())
    )
}

fn timestamp() -> String {
    // Seconds since the epoch — the page formats it; no time-zone dependency.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Plain-text per-level table (what the HTML's section 1 shows), for tuning
/// loops on the console. "MFC+" counts MFC and S-MFC together.
pub fn summary_table(meta: &Meta, cards: &[Scorecard]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} songs · {} charts · {} seed(s) · {}",
        cards.len(),
        meta.chart_count,
        meta.seeds,
        meta.curve_description
    );
    let _ = writeln!(
        out,
        "{:>3} {:>6} {:>7} {:>9} {:>8} {:>6} {:>6} {:>6} {:>6} {:>6} | fail% b/B/D/E/C",
        "L", "sigma", "p_miss", "mean", "median", "EX%", "FC%", "MFC+%", "SMFC%", "fail%"
    );
    for &(level, c) in &meta.curves {
        let cs: Vec<&Scorecard> = cards.iter().filter(|x| x.level == level).collect();
        if cs.is_empty() {
            continue;
        }
        let n = cs.len() as f64;
        let mut scores: Vec<i32> = cs.iter().map(|x| x.score).collect();
        scores.sort_unstable();
        let mean = scores.iter().map(|&v| v as f64).sum::<f64>() / n;
        let median = scores[scores.len() / 2];
        let ex = cs
            .iter()
            .filter(|x| x.ex_max > 0)
            .map(|x| 100.0 * x.ex as f64 / x.ex_max as f64)
            .sum::<f64>()
            / n;
        let pct =
            |f: &dyn Fn(&&Scorecard) -> bool| 100.0 * cs.iter().filter(|x| f(x)).count() as f64 / n;
        let fc = pct(&|x| x.fc.is_some());
        let mfc = pct(&|x| matches!(x.fc, Some(crate::scoring::FullCombo::Marvelous)));
        let smfc = pct(&|x| x.smfc);
        let fail = pct(&|x| x.failed);
        let by_diff: Vec<String> = crate::chart::Difficulty::ALL
            .iter()
            .map(|d| {
                let ds: Vec<&&Scorecard> = cs.iter().filter(|x| x.difficulty == *d).collect();
                if ds.is_empty() {
                    "  -".to_string()
                } else {
                    format!(
                        "{:3.0}",
                        100.0 * ds.iter().filter(|x| x.failed).count() as f64 / ds.len() as f64
                    )
                }
            })
            .collect();
        let _ = writeln!(
            out,
            "{:>3} {:>6.1} {:>6.2}% {:>9.0} {:>8} {:>6.1} {:>6.1} {:>6.1} {:>6.2} {:>6.1} | {}",
            level,
            c.sigma_ms,
            c.p_miss * 100.0,
            mean,
            median,
            ex,
            fc,
            mfc,
            smfc,
            fail,
            by_diff.join("/")
        );
    }
    out
}

pub fn json_dump(meta: &Meta, cards: &[Scorecard]) -> String {
    let rows: Vec<String> = cards.iter().map(card_json).collect();
    format!(
        "{{\"meta\":{},\"columns\":{},\"cards\":[\n{}\n]}}\n",
        meta_json(meta),
        COLUMNS_JSON,
        rows.join(",\n")
    )
}

const COLUMNS_JSON: &str = "[\"chart\",\"diff\",\"level\",\"seed\",\"notes\",\"taps\",\"freezes\",\"shocks\",\"dur_ms\",\"marv\",\"perf\",\"great\",\"good\",\"miss\",\"ok\",\"ng\",\"smarv\",\"maxcombo\",\"score\",\"ex\",\"exmax\",\"fc\",\"smfc\",\"fast\",\"slow\",\"gauge_final\",\"gauge_min\",\"failed\",\"failed_at_ms\",\"rank\",\"mismatches\",\"hist\"]";

pub fn render_html(meta: &Meta, cards: &[Scorecard]) -> String {
    let rows: Vec<String> = cards.iter().map(card_json).collect();
    let mut html = String::with_capacity(rows.iter().map(|r| r.len() + 2).sum::<usize>() + 40_000);
    html.push_str(HTML_HEAD);
    html.push_str("<script>\nconst META = ");
    html.push_str(&meta_json(meta));
    html.push_str(";\nconst COLS = ");
    html.push_str(COLUMNS_JSON);
    html.push_str(";\nconst CARDS = [\n");
    html.push_str(&rows.join(",\n"));
    html.push_str("\n];\n</script>\n");
    html.push_str(HTML_BODY);
    html
}

const HTML_HEAD: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>Multiplayer Bot — simulated spread</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root{--bg:#0e1117;--panel:#161b22;--line:#30363d;--fg:#e6edf3;--dim:#8b949e;--acc:#a030ff;
--marv:#e0d0ff;--smarv:#a030ff;--perf:#ffd54a;--great:#4ade80;--good:#60a5fa;--miss:#f87171;--ok:#c084fc;--ng:#fb923c;--fail:#ef4444;--pass:#22c55e}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 -apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif}
header{padding:18px 24px;border-bottom:1px solid var(--line);background:var(--panel)}
h1{margin:0 0 4px;font-size:20px}h2{font-size:16px;margin:28px 0 10px;color:var(--fg)}
.meta{color:var(--dim);font-size:12px}
main{padding:0 24px 40px;max-width:1500px;margin:0 auto}
table{border-collapse:collapse;width:100%;font-size:13px}th,td{padding:5px 8px;border-bottom:1px solid var(--line);text-align:right;white-space:nowrap}
th{color:var(--dim);font-weight:600;position:sticky;top:0;background:var(--bg)}td:first-child,th:first-child{text-align:left}
.bar{display:flex;height:14px;border-radius:3px;overflow:hidden;min-width:160px;background:#222}.bar span{display:block;height:100%}
.heat td{text-align:center;min-width:56px}.heat td.v{color:#000;font-weight:600}
.controls{display:flex;gap:12px;flex-wrap:wrap;align-items:end;margin:10px 0 14px}
.controls label{display:flex;flex-direction:column;font-size:12px;color:var(--dim);gap:3px}
input,select{background:var(--panel);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:6px 8px;font:inherit}
button{background:var(--acc);color:#fff;border:0;border-radius:6px;padding:7px 12px;font:inherit;cursor:pointer}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(330px,1fr));gap:14px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:12px 14px;position:relative;overflow:hidden}
.card.failed{border-color:var(--fail)}.card .title{display:flex;justify-content:space-between;align-items:baseline;gap:8px}
.card .song{font-weight:700;font-size:15px;letter-spacing:.3px}.card .diff{font-size:11px;padding:2px 7px;border-radius:999px;background:#2b3240;color:var(--fg)}
.card .who{color:var(--dim);font-size:12px;margin:2px 0 8px}.card .score{font-size:30px;font-weight:800;letter-spacing:1px;font-variant-numeric:tabular-nums}
.card .rank{font-size:26px;font-weight:900;color:var(--perf);margin-left:8px}.card .rank.E{color:var(--fail)}
.card .badges{display:flex;gap:6px;flex-wrap:wrap;margin:6px 0 8px}.badge{font-size:11px;padding:2px 7px;border-radius:5px;background:#2b3240;color:var(--fg);font-weight:700}
.badge.mfc{background:var(--marv);color:#000}.badge.smfc{background:var(--smarv);color:#fff}.badge.pfc{background:var(--perf);color:#000}.badge.gfc{background:var(--great);color:#000}.badge.fc{background:var(--good);color:#000}.badge.fail{background:var(--fail);color:#fff}.badge.pass{background:var(--pass);color:#000}
.judg{display:grid;grid-template-columns:1fr auto;gap:2px 10px;font-size:12px;font-variant-numeric:tabular-nums}.judg .k{color:var(--dim)}.judg .v{text-align:right;font-weight:600}
.smarvk{color:var(--smarv)}.marvk{color:var(--marv)}.perfk{color:var(--perf)}.greatk{color:var(--great)}.goodk{color:var(--good)}.missk{color:var(--miss)}.okk{color:var(--ok)}.ngk{color:var(--ng)}
.foot{display:flex;justify-content:space-between;color:var(--dim);font-size:11px;margin-top:8px}.gauge{height:6px;background:#222;border-radius:3px;overflow:hidden;margin-top:8px}.gauge span{display:block;height:100%;background:var(--pass)}
.gauge.low span{background:var(--fail)}svg.hist{width:100%;height:36px;margin-top:6px}svg.hist rect{fill:#556}svg.hist rect.c{fill:var(--acc)}
.note{color:var(--dim);font-size:12px}.warn{color:var(--ng)}details{margin:8px 0}summary{cursor:pointer;color:var(--dim)}
.pager{display:flex;gap:8px;align-items:center;margin:10px 0}
</style></head><body>
"##;

const HTML_BODY: &str = r##"<header><h1>Multiplayer Bot — simulated spread</h1><div class="meta" id="meta"></div></header>
<main>
<h2>1. Level overview</h2>
<div class="note">Every value is over all simulated songs of the level (all selected charts × difficulties × seeds). Judgement mix bar: <span class="smarvk">S-MARV</span> / <span class="marvk">MARV</span> / <span class="perfk">PERF</span> / <span class="greatk">GREAT</span> / <span class="goodk">GOOD</span> / <span class="missk">MISS</span> as a share of tap notes (O.K./N.G. excluded).</div>
<table id="levels"></table>

<h2>2. Level × difficulty</h2>
<div class="controls"><label>Metric<select id="heatMetric">
<option value="score">mean score</option><option value="pfail">fail rate %</option><option value="pfc">FC rate % (any)</option><option value="pmfc">MFC rate %</option><option value="miss">mean miss %</option><option value="ex">mean EX %</option><option value="gauge">mean final gauge %</option>
</select></label></div>
<table class="heat" id="heat"></table>

<h2>3. Scorecards</h2>
<div class="controls">
<label>Chart contains<input id="fChart" placeholder="e.g. sabm" size="12"></label>
<label>Difficulty<select id="fDiff"><option value="">all</option><option value="0">BEGINNER</option><option value="1">BASIC</option><option value="2">DIFFICULT</option><option value="3">EXPERT</option><option value="4">CHALLENGE</option></select></label>
<label>Level<select id="fLevel"><option value="">all</option></select></label>
<label>Seed<select id="fSeed"><option value="">all</option></select></label>
<label>Outcome<select id="fOut"><option value="">all</option><option value="fail">failed</option><option value="pass">passed</option><option value="fc">any FC</option><option value="mfc">MFC</option><option value="smfc">S-MFC</option><option value="mismatch">model mismatch &gt; 0</option></select></label>
<label>Sort<select id="fSort"><option value="chart">chart</option><option value="score">score ↓</option><option value="scoreAsc">score ↑</option><option value="miss">misses ↓</option><option value="gauge">final gauge ↑</option></select></label>
<button id="apply">Apply</button><span class="note" id="count"></span></div>
<div class="grid" id="cards"></div>
<div class="pager"><button id="prev">◀ prev</button><span id="page" class="note"></span><button id="next">next ▶</button></div>

<h2>4. Model &amp; known gaps</h2>
<div class="note">
<p>The simulation runs the DLL's real <code>skill.rs</code> / <code>planner.rs</code> against every chart, through a model of <code>GamePlayActor::judgeNotes</code>, the freeze judge, <code>judge_submit</code> and the NORMAL gauge transcribed from <code>gamemdx.dll</code> 20260825 (<code>docs/gauge_and_judge_scoring_research.md</code>). Judge: one accepted note per frame; earliest-unjudged attribution; graded windows Marvelous ±17 / Perfect ±34 / Great ±84 / Good ±124 ms (World has no Boo — a 125..160 ms event is matched, rejected and later Missed at +160); shocks O.K. unless a pad panel is pressed in [−34, +84]. Gauge: exact integer formula incl. combo/max-combo recovery, consecutive-miss divisor, ¾-measure streak reset and the 20 s "bell". Score/EX/FC: exact.</p>
<p><b>Simplifications:</b> a freeze whose head was Missed is resolved N.G. (the game may tap-Miss the tail instead: gauge severity 9 vs 13, Miss column +1); a note that is both the frame's accepted candidate and already past +160 is taken as a Miss (the game double-submits); the freeze judge is modelled as "O.K. iff the head was hit" (the bot always holds); rank letters use the community threshold table, not RE'd. <b>Mismatch</b> = judged grades the planner did not predict. Expected ≈0: the residual cases (a few per 100k judgements, dense Challenge streams) are late-planned Goods that lose the judge's one-accepted-note-per-frame race to better notes on other panels until the +160 Miss mark — real game behaviour the planner cannot pre-empt, and the same rate the DLL's cabinet self-check will report. Ticks → ms uses the game's <code>round(seconds_ticks·1000/TPS)</code> normalisation.</p>
</div>
</main>
<script>
const DIFFS=["BEGINNER","BASIC","DIFFICULT","EXPERT","CHALLENGE"];
const C={};COLS.forEach((c,i)=>C[c]=i);
const FCL=["MFC","PFC","GFC","FC"];
function fmt(n){return n.toLocaleString("en-US")}
function pct(a,b){return b?(100*a/b):0}
function mean(a){return a.length?a.reduce((x,y)=>x+y,0)/a.length:0}
function median(a){if(!a.length)return 0;const s=[...a].sort((x,y)=>x-y);const m=s.length>>1;return s.length%2?s[m]:(s[m-1]+s[m])/2}
(function(){
  const d=new Date(META.generated*1000);
  const ov=META.curve_override?' <span class="warn">(skill constants OVERRIDDEN on the command line)</span>':'';
  document.getElementById('meta').innerHTML=
   `${META.chart_count} single charts from ${META.file_count} files in <code>${META.ssq_dir}</code> · levels ${META.levels.join(',')} · ${META.seeds} seed(s) · ${CARDS.length} songs simulated in ${META.elapsed_secs}s · judge ${META.fps} fps · S-Marv window ±${META.smarv_ms} ms · ${d.toISOString().slice(0,19).replace('T',' ')} UTC${ov}`+
   (META.parse_errors.length?`<br><span class="warn">${META.parse_errors.length} file(s) skipped: ${META.parse_errors.slice(0,5).map(e=>e[0]).join(', ')}${META.parse_errors.length>5?'…':''}</span>`:'');
})();
// ---- 1. level overview
(function(){
  const rows=[];
  const hdr=`<tr><th>Level</th><th>σ ms</th><th>p_miss</th><th>songs</th><th>mean score</th><th>median</th><th>EX %</th><th>any FC %</th><th>MFC %</th><th>S-MFC %</th><th>fail %</th><th>mean miss %</th><th>judgement mix</th></tr>`;
  for(const L of META.levels){
    const cs=CARDS.filter(c=>c[C.level]===L);if(!cs.length)continue;
    const cv=META.curves.find(x=>x[0]===L)||[L,0,0];
    const scores=cs.map(c=>c[C.score]);
    const ex=cs.map(c=>pct(c[C.ex],c[C.exmax]));
    const anyfc=cs.filter(c=>c[C.fc]>=0).length,mfc=cs.filter(c=>c[C.fc]===0).length,smfc=cs.filter(c=>c[C.smfc]).length,fail=cs.filter(c=>c[C.failed]).length;
    const tot=cs.reduce((s,c)=>s+c[C.taps],0);
    const sum=k=>cs.reduce((s,c)=>s+c[C[k]],0);
    const sm=sum('smarv'),ma=sum('marv')-sm,pe=sum('perf'),gr=sum('great'),go=sum('good'),mi=sum('miss');
    const seg=(v,cls)=>`<span class="${cls}" style="width:${pct(v,tot).toFixed(2)}%;background:var(--${cls})" title="${cls} ${pct(v,tot).toFixed(1)}%"></span>`;
    rows.push(`<tr><td><b>LV ${L}</b></td><td>${cv[1].toFixed(1)}</td><td>${(cv[2]*100).toFixed(2)}%</td><td>${cs.length}</td><td>${fmt(Math.round(mean(scores)))}</td><td>${fmt(Math.round(median(scores)))}</td><td>${mean(ex).toFixed(1)}</td><td>${pct(anyfc,cs.length).toFixed(1)}</td><td>${pct(mfc,cs.length).toFixed(1)}</td><td>${pct(smfc,cs.length).toFixed(2)}</td><td>${pct(fail,cs.length).toFixed(1)}</td><td>${pct(mi,tot).toFixed(2)}</td><td><div class="bar">${seg(sm,'smarv')}${seg(ma,'marv')}${seg(pe,'perf')}${seg(gr,'great')}${seg(go,'good')}${seg(mi,'miss')}</div></td></tr>`);
  }
  document.getElementById('levels').innerHTML=hdr+rows.join('');
})();
// ---- 2. heatmap
function heat(){
  const m=document.getElementById('heatMetric').value;
  const val=(cs)=>{if(!cs.length)return null;switch(m){
    case 'score':return mean(cs.map(c=>c[C.score]));
    case 'pfail':return pct(cs.filter(c=>c[C.failed]).length,cs.length);
    case 'pfc':return pct(cs.filter(c=>c[C.fc]>=0).length,cs.length);
    case 'pmfc':return pct(cs.filter(c=>c[C.fc]===0).length,cs.length);
    case 'miss':return mean(cs.map(c=>pct(c[C.miss],c[C.taps])));
    case 'ex':return mean(cs.map(c=>pct(c[C.ex],c[C.exmax])));
    case 'gauge':return mean(cs.map(c=>c[C.gauge_final]/100));}};
  const fmtv=v=>m==='score'?fmt(Math.round(v)):v.toFixed(1)+(m==='gauge'||m.startsWith('p')||m==='miss'||m==='ex'?'%':'');
  const good=(m==='pfail'||m==='miss')?(v)=>1-v/100:(m==='score')?(v)=>v/1e6:(v)=>v/100;
  let h=`<tr><th>Level</th>${DIFFS.map(d=>`<th>${d}</th>`).join('')}<th>ALL</th></tr>`;
  for(const L of META.levels){
    h+=`<tr><td style="text-align:left"><b>LV ${L}</b></td>`;
    for(let d=0;d<=5;d++){
      const cs=CARDS.filter(c=>c[C.level]===L&&(d===5||c[C.diff]===d));
      const v=val(cs);
      if(v===null){h+='<td>–</td>';continue}
      const g=Math.max(0,Math.min(1,good(v)));
      const col=`hsl(${Math.round(g*120)},70%,${Math.round(45+g*20)}%)`;
      h+=`<td class="v" style="background:${col}" title="${cs.length} songs">${fmtv(v)}</td>`;
    }
    h+='</tr>';
  }
  document.getElementById('heat').innerHTML=h;
}
document.getElementById('heatMetric').addEventListener('change',heat);heat();
// ---- 3. scorecards
const PAGE=60;let page=0,filtered=[];
(function(){
  const fl=document.getElementById('fLevel');META.levels.forEach(L=>{const o=document.createElement('option');o.value=L;o.textContent='LV '+L;fl.appendChild(o)});
  const fs=document.getElementById('fSeed');for(let s=0;s<META.seeds;s++){const o=document.createElement('option');o.value=s;o.textContent='seed '+s;fs.appendChild(o)}
})();
function hist(h){const mx=Math.max(1,...h);const w=100/h.length;
  return `<svg class="hist" viewBox="0 0 100 20" preserveAspectRatio="none">${h.map((v,i)=>`<rect class="${i===12?'c':''}" x="${(i*w).toFixed(2)}" y="${(20-20*v/mx).toFixed(2)}" width="${(w*0.9).toFixed(2)}" height="${(20*v/mx).toFixed(2)}"><title>${(i*10-125)}..${(i*10-115)} ms: ${v}</title></rect>`).join('')}</svg>`}
function card(c){
  const fc=c[C.fc];const failed=c[C.failed];
  const badges=[];
  badges.push(failed?'<span class="badge fail">FAILED</span>':'<span class="badge pass">CLEAR</span>');
  if(c[C.smfc])badges.push('<span class="badge smfc">S-MFC</span>');else if(fc>=0)badges.push(`<span class="badge ${FCL[fc].toLowerCase()}">${FCL[fc]}</span>`);
  if(c[C.mismatches])badges.push(`<span class="badge fail" title="planner/judge disagreement">MISMATCH ${c[C.mismatches]}</span>`);
  const g=c[C.gauge_final]/100;
  const row=(k,cls,v)=>`<div class="k ${cls}">${k}</div><div class="v">${fmt(v)}</div>`;
  return `<div class="card ${failed?'failed':''}">
  <div class="title"><span class="song">${c[C.chart]}</span><span class="diff">${DIFFS[c[C.diff]]}</span></div>
  <div class="who">BOT LV${c[C.level]} · seed ${c[C.seed]} · ${c[C.notes]} notes (${c[C.taps]} taps, ${c[C.freezes]} freezes, ${c[C.shocks]} shocks) · ${(c[C.dur_ms]/1000).toFixed(0)} s</div>
  <div><span class="score">${fmt(c[C.score])}</span><span class="rank ${c[C.rank]==='E'?'E':''}">${c[C.rank]}</span></div>
  <div class="badges">${badges.join('')}<span class="badge">EX ${c[C.ex]}/${c[C.exmax]}</span><span class="badge">MAX COMBO ${c[C.maxcombo]}</span></div>
  <div class="judg">${row('S-MARVELOUS','smarvk',c[C.smarv])}${row('MARVELOUS','marvk',c[C.marv]-c[C.smarv])}${row('PERFECT','perfk',c[C.perf])}${row('GREAT','greatk',c[C.great])}${row('GOOD','goodk',c[C.good])}${row('MISS','missk',c[C.miss])}${row('O.K.','okk',c[C.ok])}${row('N.G.','ngk',c[C.ng])}</div>
  ${hist(c[C.hist])}
  <div class="gauge ${g<28?'low':''}"><span style="width:${g.toFixed(1)}%"></span></div>
  <div class="foot"><span>gauge ${g.toFixed(1)}% (min ${(c[C.gauge_min]/100).toFixed(1)}%)${failed?` · died at ${(c[C.failed_at_ms]/1000).toFixed(0)} s`:''}</span><span>FAST ${c[C.fast]} · SLOW ${c[C.slow]}</span></div>
  </div>`;
}
function applyFilter(){
  const ch=document.getElementById('fChart').value.trim().toLowerCase();
  const d=document.getElementById('fDiff').value,L=document.getElementById('fLevel').value,s=document.getElementById('fSeed').value,o=document.getElementById('fOut').value,so=document.getElementById('fSort').value;
  filtered=CARDS.filter(c=>(!ch||c[C.chart].toLowerCase().includes(ch))&&(d===''||c[C.diff]==+d)&&(L===''||c[C.level]==+L)&&(s===''||c[C.seed]==+s)&&
    (o===''||(o==='fail'&&c[C.failed])||(o==='pass'&&!c[C.failed])||(o==='fc'&&c[C.fc]>=0)||(o==='mfc'&&c[C.fc]===0)||(o==='smfc'&&c[C.smfc])||(o==='mismatch'&&c[C.mismatches]>0)));
  const cmp={chart:(a,b)=>a[C.chart].localeCompare(b[C.chart])||a[C.diff]-b[C.diff]||a[C.level]-b[C.level]||a[C.seed]-b[C.seed],
    score:(a,b)=>b[C.score]-a[C.score],scoreAsc:(a,b)=>a[C.score]-b[C.score],miss:(a,b)=>b[C.miss]-a[C.miss],gauge:(a,b)=>a[C.gauge_final]-b[C.gauge_final]}[so];
  filtered.sort(cmp);page=0;render();
}
function render(){
  const n=filtered.length,pages=Math.max(1,Math.ceil(n/PAGE));page=Math.min(page,pages-1);
  document.getElementById('count').textContent=`${n} scorecard(s)`;
  document.getElementById('page').textContent=`page ${page+1} / ${pages}`;
  document.getElementById('cards').innerHTML=filtered.slice(page*PAGE,(page+1)*PAGE).map(card).join('');
}
document.getElementById('apply').addEventListener('click',applyFilter);
document.getElementById('prev').addEventListener('click',()=>{page=Math.max(0,page-1);render()});
document.getElementById('next').addEventListener('click',()=>{page++;render()});
document.querySelectorAll('.controls input,.controls select').forEach(e=>e.addEventListener('keydown',ev=>{if(ev.key==='Enter')applyFilter()}));
applyFilter();
</script></body></html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_escapes_script_terminators() {
        assert_eq!(json_str("a</script>b"), "\"a\\u003c/script>b\"");
        assert_eq!(json_str("q\"\\\n"), "\"q\\\"\\\\\\n\"");
    }

    #[test]
    fn card_json_has_the_column_count() {
        let card = Scorecard {
            chart: "aaaa".into(),
            difficulty: crate::chart::Difficulty::Expert,
            level: 5,
            seed_idx: 0,
            note_count: 10,
            taps: 9,
            freezes: 1,
            shocks: 0,
            duration_ms: 90_000,
            counts: [5, 2, 1, 0, 1, 0, 1, 0],
            smarv: 4,
            max_combo: 6,
            score: 812_340,
            ex: 20,
            ex_max: 30,
            fc: None,
            smfc: false,
            fast: 1,
            slow: 2,
            gauge_final: 7_500,
            gauge_min: 4_100,
            failed: false,
            failed_at_ms: None,
            rank: "A",
            mismatches: 0,
            delta_hist: [0; 25],
        };
        let j = card_json(&card);
        // 31 scalar columns + the histogram array.
        let cols: usize = COLUMNS_JSON.matches(',').count() + 1;
        assert_eq!(cols, 32);
        assert!(j.starts_with("[\"aaaa\",3,5,0,10,9,1,0,90000,5,2,1,0,0,1,0,4,6,812340,20,30,-1,0,1,2,7500,4100,0,-1,\"A\",0,["));
    }
}
