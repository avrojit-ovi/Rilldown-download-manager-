//! Native messaging entry point.
//!
//! stdout is reserved for protocol frames. All diagnostics go to stderr, which
//! browsers capture in their own logs.

fn main() -> std::process::ExitCode {
    rilldown_native_host::run()
}
