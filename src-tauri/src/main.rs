//! Rilldown entry point.
//!
//! The binary doubles as the browser native-messaging host. Browsers cannot pass
//! arguments in a Chrome manifest, so the shell installs this executable's
//! sibling `rilldown-native-host`; `--native-host` remains available for
//! development and Firefox manifests that do support arguments.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--native-host")
    {
        return rilldown_native_host::run();
    }
    rilldown_app_lib::run();
    std::process::ExitCode::SUCCESS
}
