//! Shared visual chrome for the TUI.

const PRODUCT_VERSION: &str = concat!("phosphorpulse v", env!("CARGO_PKG_VERSION"));

/// Builds the Main Menu title line.
pub fn main_menu_title() -> String {
    PRODUCT_VERSION.to_owned()
}

/// Builds the Wizard title line.
pub fn wizard_title() -> String {
    PRODUCT_VERSION.to_owned()
}
