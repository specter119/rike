//! Command-line parsing. Pure: the caller decides how to report errors and exits.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

/// Parsed command line. Callers pass `args_os().skip(1)` to [`parse`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cli {
    pub vault: Option<VaultArg>,
    pub trace: bool,
    pub fps: bool,
    pub dev_server: Option<String>,
    pub help: bool,
}

/// A vault directory named on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultArg {
    pub path: PathBuf,
    /// `--vault` creates a missing directory; a positional directory must exist.
    pub create: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// A second positional directory was given.
    TwoDirs,
    /// A positional directory and `--vault` were both given, or `--vault` twice.
    DirAndVault,
    /// A flag that takes a value was not followed by one.
    MissingValue(&'static str),
    /// A flag that takes a value got one it cannot use.
    InvalidValue(&'static str),
    UnknownArg(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TwoDirs => f.write_str("only one vault directory can be given"),
            Self::DirAndVault => {
                f.write_str("give the vault directory either as an argument or with --vault")
            }
            Self::MissingValue(flag) => write!(f, "{flag} needs a value"),
            Self::InvalidValue(flag) => write!(f, "{flag} has an invalid value"),
            Self::UnknownArg(arg) => write!(f, "unknown argument: {arg}"),
        }
    }
}

impl std::error::Error for CliError {}

pub fn usage() -> &'static str {
    "\
Usage: rike [OPTIONS] [DIR]

Open a Rike vault. Without DIR or --vault the last vault is opened, or the
welcome screen is shown.

Arguments:
  [DIR]                    Vault directory (must already exist)

Options:
  -v, --vault <DIR>        Vault directory, created when missing
      --trace              Write UI performance traces to the log
      --fps                Show the frame rate
      --dev-server <URL>   Load the UI from a local NSH dev server, e.g. ws://127.0.0.1:8790/ws (dev builds)
  -h, --help               Print this help
"
}

const KNOWN_FLAGS: [&str; 7] = [
    "-h",
    "--help",
    "-v",
    "--vault",
    "--trace",
    "--fps",
    "--dev-server",
];

/// Parses arguments without the program name. Flags may appear in any order.
///
/// `--help` stops parsing: nothing after it is validated. A value that looks like a
/// known flag is treated as missing rather than as a path, so typos fail loudly;
/// pass `./--trace` for a directory with a flag-like name.
pub fn parse<I, S>(args: I) -> Result<Cli, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter();
    let mut cli = Cli::default();
    let mut positional: Option<PathBuf> = None;
    let mut vault_flag: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg.to_str() {
            Some("-h" | "--help") => {
                cli.help = true;
                // Help short-circuits: whatever follows is not validated.
                return Ok(cli);
            }
            Some("--trace") => cli.trace = true,
            Some("--fps") => cli.fps = true,
            Some("-v" | "--vault") => {
                let value = value_for(&mut args, "--vault")?;
                if positional.is_some() || vault_flag.is_some() {
                    return Err(CliError::DirAndVault);
                }
                vault_flag = Some(PathBuf::from(value));
            }
            Some("--dev-server") => {
                let value = value_for(&mut args, "--dev-server")?;
                cli.dev_server = Some(
                    value
                        .to_str()
                        .ok_or(CliError::InvalidValue("--dev-server"))?
                        .to_owned(),
                );
            }
            Some(flag) if flag.starts_with('-') && flag.len() > 1 => {
                return Err(CliError::UnknownArg(flag.to_owned()));
            }
            _ => {
                if vault_flag.is_some() {
                    return Err(CliError::DirAndVault);
                }
                if positional.is_some() {
                    return Err(CliError::TwoDirs);
                }
                positional = Some(PathBuf::from(arg));
            }
        }
    }

    cli.vault = vault_flag.map_or_else(
        || {
            positional.map(|path| VaultArg {
                path,
                create: false,
            })
        },
        |path| Some(VaultArg { path, create: true }),
    );
    Ok(cli)
}

/// Whether `url` is a `ws://` URL for this machine only. The dev server pushes code the
/// app runs with the user's rights, so anything else is refused.
pub fn is_loopback_ws(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("ws://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host),
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn value_for<I, S>(args: &mut I, flag: &'static str) -> Result<OsString, CliError>
where
    I: Iterator<Item = S>,
    S: AsRef<OsStr>,
{
    let value = args.next().ok_or(CliError::MissingValue(flag))?;
    if is_known_flag(value.as_ref()) {
        return Err(CliError::MissingValue(flag));
    }
    Ok(value.as_ref().to_owned())
}

fn is_known_flag(arg: &OsStr) -> bool {
    arg.to_str().is_some_and(|arg| KNOWN_FLAGS.contains(&arg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn dev_server_must_be_loopback() {
        assert!(is_loopback_ws("ws://127.0.0.1:8790/ws"));
        assert!(is_loopback_ws("ws://localhost:8790/ws"));
        assert!(is_loopback_ws("ws://[::1]:8790/ws"));
        assert!(!is_loopback_ws("ws://192.168.1.5:8790/ws"));
        assert!(!is_loopback_ws("ws://127.0.0.1.evil.com:8790/ws"));
        assert!(!is_loopback_ws("wss://127.0.0.1:8790/ws"));
        assert!(!is_loopback_ws("http://127.0.0.1:8790"));
    }

    #[test]
    fn defaults_to_no_vault() {
        let cli = parse([] as [&str; 0]).unwrap();
        assert_eq!(cli.vault, None);
        assert!(!cli.trace);
        assert!(!cli.fps);
        assert_eq!(cli.dev_server, None);
        assert!(!cli.help);
    }

    #[test]
    fn positional_directory_must_not_create() {
        let cli = parse(["/tmp/vault"]).unwrap();
        assert_eq!(
            cli.vault,
            Some(VaultArg {
                path: path("/tmp/vault"),
                create: false
            })
        );
    }

    #[test]
    fn vault_flag_creates() {
        for flag in ["--vault", "-v"] {
            let cli = parse([flag, "/tmp/vault"]).unwrap();
            assert_eq!(
                cli.vault,
                Some(VaultArg {
                    path: path("/tmp/vault"),
                    create: true
                })
            );
        }
    }

    #[test]
    fn arguments_can_come_in_any_order() {
        let cli = parse([
            "--trace",
            "/tmp/vault",
            "--fps",
            "--dev-server",
            "http://127.0.0.1:8790",
        ])
        .unwrap();
        assert_eq!(
            cli.vault,
            Some(VaultArg {
                path: path("/tmp/vault"),
                create: false
            })
        );
        assert!(cli.trace);
        assert!(cli.fps);
        assert_eq!(cli.dev_server.as_deref(), Some("http://127.0.0.1:8790"));
    }

    #[test]
    fn help_wins() {
        for flag in ["-h", "--help"] {
            let cli = parse([flag, "--bogus"]).unwrap();
            assert!(cli.help);
            assert_eq!(cli.vault, None);
        }
    }

    #[test]
    fn two_positionals_are_an_error() {
        assert_eq!(parse(["/a", "/b"]), Err(CliError::TwoDirs));
    }

    #[test]
    fn positional_and_vault_conflict() {
        assert_eq!(parse(["/a", "--vault", "/b"]), Err(CliError::DirAndVault));
        assert_eq!(parse(["--vault", "/b", "/a"]), Err(CliError::DirAndVault));
        assert_eq!(
            parse(["--vault", "/a", "-v", "/b"]),
            Err(CliError::DirAndVault)
        );
    }

    #[test]
    fn missing_values_are_errors() {
        assert_eq!(parse(["--vault"]), Err(CliError::MissingValue("--vault")));
        assert_eq!(
            parse(["-v", "--trace"]),
            Err(CliError::MissingValue("--vault"))
        );
        assert_eq!(
            parse(["--dev-server"]),
            Err(CliError::MissingValue("--dev-server"))
        );
        assert_eq!(
            parse(["--dev-server", "--fps"]),
            Err(CliError::MissingValue("--dev-server"))
        );
    }

    #[test]
    fn unknown_arguments_are_errors() {
        assert_eq!(
            parse(["--bogus"]),
            Err(CliError::UnknownArg("--bogus".to_owned()))
        );
        assert_eq!(parse(["-x"]), Err(CliError::UnknownArg("-x".to_owned())));
    }

    #[test]
    fn usage_mentions_every_flag() {
        let usage = usage();
        for flag in KNOWN_FLAGS {
            assert!(usage.contains(flag), "usage misses {flag}");
        }
    }
}
