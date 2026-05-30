pub mod builtins;
pub mod config;
pub mod env_setup;
pub mod frontmatter;
pub mod identity;
pub mod index;
pub mod scaffold;

pub fn version() -> &'static str {
    // Builds the version into the binary
    env!("CARGO_PKG_VERSION")
}
