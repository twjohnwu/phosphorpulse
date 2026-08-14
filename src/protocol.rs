use std::{fs, io::{self, Read}, path::PathBuf};
use serde_json::Value;

const TRANSCRIPT_SUFFIX: &str = ".jsonl";

/// Frozen TS parity: `agentType`, then `role`, then the stdin task `name`.
/// Transcript metadata is best-effort: unreadable or malformed files simply
/// leave the stdin fallback in place.
pub fn resolve_subagent_role_name(task: &Value, transcript_path: Option<&str>) -> Option<String> {
    let task_name = task.get("name").and_then(Value::as_str).map(str::to_owned);
    let Some(task_id) = task.get("id").and_then(Value::as_str) else { return task_name; };
    let Some(transcript_path) = transcript_path else { return task_name; };
    if !transcript_path.ends_with(TRANSCRIPT_SUFFIX)
        || !task_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return task_name;
    }

    let base = &transcript_path[..transcript_path.len() - TRANSCRIPT_SUFFIX.len()];
    let meta_path = PathBuf::from(base).join("subagents").join(format!("agent-{task_id}.meta.json"));
    let resolved = fs::read_to_string(meta_path).ok()
        .and_then(|meta| serde_json::from_str::<Value>(&meta).ok())
        .and_then(|meta| {
            meta.get("agentType").and_then(Value::as_str).filter(|value| !value.is_empty())
                .or_else(|| meta.get("role").and_then(Value::as_str).filter(|value| !value.is_empty()))
                .map(str::to_owned)
        });
    resolved.or(task_name)
}

#[derive(Clone, Debug, Default)]
pub struct Window { pub used_percentage: Option<f64>, pub resets_at: Option<i64> }
#[derive(Clone, Debug, Default)]
pub struct RenderContext {
    pub model_display_name: Option<String>, pub effort_level: Option<String>, pub cwd: Option<String>,
    pub path_depth: usize, pub version: Option<String>, pub session_id: Option<String>,
    pub context_used_percentage: Option<f64>, pub total_input_tokens: Option<f64>, pub total_output_tokens: Option<f64>,
    pub cache_read_input_tokens: Option<f64>, pub cache_creation_input_tokens: Option<f64>,
    pub five_hour: Window, pub seven_day: Window, pub total_cost_usd: Option<f64>, pub total_duration_ms: Option<f64>,
    pub gauge_width: usize, pub warn_pct: f64, pub hot_pct: f64,
}
fn num(v: Option<&Value>) -> Option<f64> { match v? { Value::Number(n) => n.as_f64().filter(|x| x.is_finite()), Value::String(s) if !s.is_empty() => s.parse().ok().filter(|x: &f64| x.is_finite()), _ => None } }
fn reset(v: Option<&Value>) -> Option<i64> {
 let value=v?;
 match value {
  Value::Number(_) => num(Some(value)).map(|x| (x * 1000.0) as i64),
  // Claude Code has emitted both epoch seconds and ISO-8601 strings.
  Value::String(s) if !s.is_empty() => s.parse::<i64>().ok().map(|x| x * 1000).or_else(|| parse_iso_millis(s)),
  _ => None,
 }
}
// Date.parse-compatible enough for the ISO timestamps emitted by Claude Code.
fn parse_iso_millis(s: &str) -> Option<i64> {
 let (date, time) = s.split_once('T')?;
 let mut ds=date.split('-').map(str::parse::<i64>); let (y,mo,d)=(ds.next()?.ok()?,ds.next()?.ok()?,ds.next()?.ok()?);
 if !(1..=12).contains(&mo) { return None; }
 let zone_at=time.find(|c| c == 'Z' || c == '+' || c == '-').unwrap_or(time.len());
 let clock=&time[..zone_at]; let zone=&time[zone_at..]; let mut ts=clock.split(':');
 let h=ts.next()?.parse::<i64>().ok()?; let mi=ts.next()?.parse::<i64>().ok()?;
 let sec_part=ts.next().unwrap_or("0"); let (sec, fraction)=sec_part.split_once('.').unwrap_or((sec_part,""));
 let se=sec.parse::<i64>().ok()?; let ms=fraction.chars().take(3).collect::<String>().parse::<i64>().unwrap_or(0) * match fraction.len() { 0 => 0, 1 => 100, 2 => 10, _ => 1 };
 let year_days=365*(y-1970)+(1970..y).filter(|year| *year%4==0 && (*year%100!=0 || *year%400==0)).count() as i64;
 let leap= y%4==0 && (y%100!=0 || y%400==0); let month_days=[31,28 + leap as i64,31,30,31,30,31,31,30,31,30,31];
 if d < 1 || d > month_days[(mo - 1) as usize] || !(0..=23).contains(&h) || !(0..=59).contains(&mi) || !(0..=59).contains(&se) { return None; }
 let days=year_days+month_days[..(mo-1) as usize].iter().sum::<i64>()+d-1;
 let offset=if zone.is_empty() || zone=="Z" {0} else { let sign=if zone.starts_with('+') {1} else {-1}; let z=&zone[1..]; let (zh,zm)=z.split_once(':').unwrap_or((z,"0")); sign*(zh.parse::<i64>().ok()?*3600+zm.parse::<i64>().ok()?*60) };
 Some((days*86400+h*3600+mi*60+se-offset)*1000+ms)
}
impl RenderContext {
 pub fn from_value(raw: &Value, config: Option<&Value>) -> Self {
  let get = |p: &[&str]| -> Option<&Value> { let mut x=raw; for k in p { x=x.get(*k)?; } Some(x) };
  let cfg = |p: &[&str]| -> Option<&Value> { let mut x=config?; for k in p { x=x.get(*k)?; } Some(x) };
  let win = |name| Window { used_percentage:num(get(&["rate_limits",name,"used_percentage"])), resets_at:reset(get(&["rate_limits",name,"resets_at"])) };
  Self { model_display_name:get(&["model","display_name"]).and_then(Value::as_str).map(str::to_owned), effort_level:get(&["effort","level"]).and_then(Value::as_str).map(str::to_owned), cwd:get(&["cwd"]).or_else(||get(&["workspace","current_dir"])).and_then(Value::as_str).map(str::to_owned), path_depth:num(cfg(&["segments","dir","pathDepth"])).map(|n|n as usize).unwrap_or(99), version:get(&["version"]).and_then(Value::as_str).map(str::to_owned), session_id:get(&["session_id"]).and_then(Value::as_str).map(str::to_owned), context_used_percentage:num(get(&["context_window","used_percentage"])), total_input_tokens:num(get(&["context_window","total_input_tokens"])), total_output_tokens:num(get(&["context_window","total_output_tokens"])), cache_read_input_tokens:num(get(&["context_window","current_usage","cache_read_input_tokens"])), cache_creation_input_tokens:num(get(&["context_window","current_usage","cache_creation_input_tokens"])), five_hour:win("five_hour"), seven_day:win("seven_day"), total_cost_usd:num(get(&["cost","total_cost_usd"])), total_duration_ms:num(get(&["cost","total_duration_ms"])), gauge_width:num(cfg(&["gauge","barWidth"])).map(|n|n as usize).unwrap_or(20), warn_pct:num(cfg(&["gauge","warnPct"])).unwrap_or(65.), hot_pct:num(cfg(&["gauge","hotPct"])).unwrap_or(85.) }
 }
}
pub fn read_stdin_json() -> Result<Value, String> { let mut input=Vec::new(); io::stdin().read_to_end(&mut input).map_err(|e|format!("cannot read stdin: {e}"))?; serde_json::from_slice(&input).map_err(|e|e.to_string()) }
