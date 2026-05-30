pub mod builtins;
pub mod config;
pub mod env_setup;
pub mod frontmatter;
pub mod identity;
pub mod index;
pub mod scaffold;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_version() {
        assert_eq!(version(), "0.1.0");
    }
}
