//! Logging: `warn` by default, `nsh::perf` at info with `--trace` or `--fps`, `RUST_LOG` last.

use env_logger::Builder;
use log::LevelFilter;

/// Target of the performance logs enabled by the command line.
pub const PERF_TARGET: &str = "nsh::perf";

/// Filter directives implied by the command line, before `RUST_LOG` is applied.
pub fn directives(trace: bool, fps: bool) -> Vec<(&'static str, LevelFilter)> {
    let mut directives = vec![("", LevelFilter::Warn)];
    if trace || fps {
        directives.push((PERF_TARGET, LevelFilter::Info));
    }
    directives
}

/// Builds the logger: command-line filters first, then `RUST_LOG` on top.
pub fn builder(trace: bool, fps: bool) -> Builder {
    let mut builder = filtered(trace, fps);
    builder.parse_default_env();
    builder
}

/// Builds the logger without reading `RUST_LOG`; kept separate so tests avoid the environment.
fn filtered(trace: bool, fps: bool) -> Builder {
    let mut builder = Builder::new();
    for (module, level) in directives(trace, fps) {
        if module.is_empty() {
            builder.filter_level(level);
        } else {
            builder.filter_module(module, level);
        }
    }
    builder
}

/// Installs the global logger. A second call keeps the first logger.
pub fn init(trace: bool, fps: bool) {
    let mut builder = builder(trace, fps);
    let _ = builder.try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{Level, Log, Metadata};

    fn enabled(trace: bool, fps: bool, level: Level, target: &str) -> bool {
        let logger = filtered(trace, fps).build();
        logger.enabled(&Metadata::builder().level(level).target(target).build())
    }

    #[test]
    fn warns_by_default() {
        assert!(enabled(false, false, Level::Warn, "rike_vault"));
        assert!(!enabled(false, false, Level::Info, "rike_vault"));
        assert!(!enabled(false, false, Level::Debug, "nsh::perf"));
    }

    #[test]
    fn perf_logs_are_enabled_by_trace_or_fps() {
        for (trace, fps) in [(true, false), (false, true), (true, true)] {
            assert!(enabled(trace, fps, Level::Info, "nsh::perf"));
            assert!(enabled(trace, fps, Level::Info, "nsh::perf::frame"));
            assert!(!enabled(trace, fps, Level::Debug, "nsh::perf"));
            // Everything else stays at the default level.
            assert!(!enabled(trace, fps, Level::Info, "rike_vault"));
        }
    }

    #[test]
    fn directives_are_default_then_perf() {
        assert_eq!(directives(false, false), vec![("", LevelFilter::Warn)]);
        assert_eq!(
            directives(true, false),
            vec![("", LevelFilter::Warn), (PERF_TARGET, LevelFilter::Info)]
        );
        assert_eq!(
            directives(false, true),
            vec![("", LevelFilter::Warn), (PERF_TARGET, LevelFilter::Info)]
        );
    }
}
