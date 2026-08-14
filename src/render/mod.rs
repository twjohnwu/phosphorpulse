pub mod row_builder;
pub mod themes;

pub fn render() {
    if let Err(reason) = crate::config::load() {
        println!("⚠ warning: phosphorpulse config invalid — {reason}");
    }
}

pub fn render_subagent() {
    if crate::config::load().is_err() {
        let _defaults = crate::config::default_config();
        // The full renderer lands in a later task; this preserves the required
        // subagent fallback contract without suppressing the statusline.
        println!("phosphorpulse subagent");
    }
}
