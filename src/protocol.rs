use std::io::{self, Read};
use serde_json::Value;

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
fn reset(v: Option<&Value>) -> Option<i64> { let value=v?; match value { Value::Number(_) => num(Some(value)).map(|x| (x * 1000.0) as i64), Value::String(s) if !s.is_empty() => s.parse::<i64>().ok(), _ => None } }
impl RenderContext {
 pub fn from_value(raw: &Value, config: Option<&Value>) -> Self {
  let get = |p: &[&str]| -> Option<&Value> { let mut x=raw; for k in p { x=x.get(*k)?; } Some(x) };
  let cfg = |p: &[&str]| -> Option<&Value> { let mut x=config?; for k in p { x=x.get(*k)?; } Some(x) };
  let win = |name| Window { used_percentage:num(get(&["rate_limits",name,"used_percentage"])), resets_at:reset(get(&["rate_limits",name,"resets_at"])) };
  Self { model_display_name:get(&["model","display_name"]).and_then(Value::as_str).map(str::to_owned), effort_level:get(&["effort","level"]).and_then(Value::as_str).map(str::to_owned), cwd:get(&["cwd"]).or_else(||get(&["workspace","current_dir"])).and_then(Value::as_str).map(str::to_owned), path_depth:num(cfg(&["segments","dir","pathDepth"])).map(|n|n as usize).unwrap_or(99), version:get(&["version"]).and_then(Value::as_str).map(str::to_owned), session_id:get(&["session_id"]).and_then(Value::as_str).map(str::to_owned), context_used_percentage:num(get(&["context_window","used_percentage"])), total_input_tokens:num(get(&["context_window","total_input_tokens"])), total_output_tokens:num(get(&["context_window","total_output_tokens"])), cache_read_input_tokens:num(get(&["context_window","current_usage","cache_read_input_tokens"])), cache_creation_input_tokens:num(get(&["context_window","current_usage","cache_creation_input_tokens"])), five_hour:win("five_hour"), seven_day:win("seven_day"), total_cost_usd:num(get(&["cost","total_cost_usd"])), total_duration_ms:num(get(&["cost","total_duration_ms"])), gauge_width:num(cfg(&["gauge","barWidth"])).map(|n|n as usize).unwrap_or(20), warn_pct:num(cfg(&["gauge","warnPct"])).unwrap_or(65.), hot_pct:num(cfg(&["gauge","hotPct"])).unwrap_or(85.) }
 }
}
pub fn read_stdin_json() -> Result<Value, String> { let mut input=Vec::new(); io::stdin().read_to_end(&mut input).map_err(|e|format!("cannot read stdin: {e}"))?; serde_json::from_slice(&input).map_err(|e|e.to_string()) }
