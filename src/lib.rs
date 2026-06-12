pub mod builtins;
pub mod command_meta;
pub mod config;
pub mod env_setup;
pub mod identity;
pub mod index;
pub mod scaffold;

pub fn version() -> &'static str {
    // Builds the version into the binary
    env!("CARGO_PKG_VERSION")
}

/// Process exit codes, centralized so they stay consistent across the binary
/// and the built-ins. Values follow common shell conventions where they apply.
pub mod exit_codes {
    /// A general runtime failure (an I/O error, a failed config load, …).
    pub const FAILURE: i32 = 1;
    /// Bad invocation: an unrecognized/missing flag or a missing required argument.
    pub const USAGE: i32 = 2;
    /// A command was found but could not be executed.
    pub const EXEC_FAILED: i32 = 126;
    /// No such command (mirrors the shell's "command not found").
    pub const NOT_FOUND: i32 = 127;
    /// Completion produced nothing; the shell should use its default completion.
    pub const COMPLETION_FALLBACK: i32 = 42;
}
