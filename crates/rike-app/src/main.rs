//! `rike [DIR]`: opens the window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Termination};
use std::sync::Arc;
use std::time::Duration;

use nsh_platform_linux::{AppOptions, AppSource, Bundle, Limits};
use rike_app::{cli, logging};
use rike_native::session::{self, Boot, RelaunchHook, View};
use rike_native::{startup, state};
use rike_vault::Vault;

const APP_ID: &str = "dev.rike.app";
const BUNDLE: &str = include_str!(env!("RIKE_UI_BUNDLE"));
const BUNDLE_PRESENT: bool = matches!(env!("RIKE_UI_BUNDLE_PRESENT").as_bytes(), b"1");

fn die(code: i32, message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(code);
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn read_recent(file: Option<&Path>) -> Vec<state::Entry> {
    file.and_then(|f| std::fs::read_to_string(f).ok())
        .map(|text| state::parse(&text))
        .unwrap_or_default()
}

/// Restarts the process into another vault, passing the debug flags on.
fn relaunch_hook(cli: &cli::Cli) -> RelaunchHook {
    let mut flags: Vec<OsString> = Vec::new();
    if cli.trace {
        flags.push("--trace".into());
    }
    if cli.fps {
        flags.push("--fps".into());
    }
    if let Some(url) = &cli.dev_server {
        flags.push("--dev-server".into());
        flags.push(url.into());
    }
    Arc::new(move |path| {
        Command::new(std::env::current_exe()?)
            .args(&flags)
            .arg("--vault")
            .arg(path)
            .spawn()
            .map(drop)
    })
}

fn main() -> impl Termination {
    rike_app::im::select_gtk_module();
    let cli = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli) => cli,
        Err(e) => die(2, format!("rike: {e}\n\n{}", cli::usage())),
    };
    if cli.help {
        print!("{}", cli::usage());
        std::process::exit(0);
    }
    logging::init(cli.trace, cli.fps);
    // Loads the fonts on another thread while the vault and GTK start up.
    rike_native::math::prewarm();

    // The runtime must outlive the window: the vault actor and the session run on it.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap_or_else(|e| die(1, format!("cannot start the async runtime: {e}")));

    let home = env_path("HOME");
    let state_file = state::state_file_path(env_path("XDG_STATE_HOME").as_deref(), home.as_deref());
    let recent = read_recent(state_file.as_deref());

    // A relative path must be judged by where it really points.
    let cli_vault = cli.vault.as_ref().map(|v| {
        let path = std::fs::canonicalize(&v.path).unwrap_or_else(|_| v.path.clone());
        (path, v.create)
    });
    let decision = startup::decide_with(
        cli_vault.as_ref().map(|(p, c)| (p.as_path(), *c)),
        &recent,
        |p| p.is_dir(),
        home.as_deref(),
    );

    let mut notice = None;
    let mut opened = None;
    match decision {
        startup::Decision::Open { path, view, .. } => match runtime.block_on(Vault::open(&path)) {
            Ok((vault, events)) => {
                let root = vault.root().to_path_buf();
                let saved = view.or_else(|| {
                    recent
                        .iter()
                        .find(|e| e.path == root)
                        .and_then(|e| e.view.clone())
                });
                opened = Some((
                    vault,
                    events,
                    startup::view_from_saved(saved.as_deref(), &root),
                ));
            }
            Err(e) => notice = Some(format!("无法打开 {}：{e}", path.display())),
        },
        startup::Decision::Welcome { notice: n } => notice = n,
    }

    // Remember the vault before the UI starts so the welcome page and the
    // next start see it; a failure only costs the history.
    let mut recent_paths: Vec<String> = recent
        .iter()
        .map(|e| e.path.to_string_lossy().into_owned())
        .collect();
    if let (Some((vault, ..)), Some(file)) = (&opened, &state_file) {
        let root = vault.root().to_path_buf();
        match state::update(file, |entries| state::push_front(entries, &root)) {
            Ok(_) => {
                recent_paths = read_recent(Some(file))
                    .iter()
                    .map(|e| e.path.to_string_lossy().into_owned())
                    .collect()
            }
            Err(e) => log::warn!("cannot update the recent-vaults file: {e}"),
        }
    }

    let title = match &opened {
        Some((vault, ..)) => format!("日课 · {}", vault.root().display()),
        None => "日课".to_owned(),
    };
    let options = AppOptions {
        title,
        size: (1100, 800),
        app_id: APP_ID.to_owned(),
        trace: cli.trace,
        fps: cli.fps,
        limits: Limits::standalone(),
        ..Default::default()
    };

    let source = ui_source(&cli);
    let (vault, events, view) = match opened {
        Some((vault, events, view)) => (
            Some(vault),
            Some(Arc::new(tokio::sync::Mutex::new(events))),
            view,
        ),
        None => (None, None, View::Journals),
    };
    session::set_boot(Boot {
        vault,
        events,
        today: rike_native::today::local_today(),
        handle: runtime.handle().clone(),
        view,
        notice,
        recent: recent_paths,
        relaunch: relaunch_hook(&cli),
        state_file,
    });
    nsh_platform_linux::set_ui_sets(vec![nsh_base::registry(), rike_native::registry()]);
    let raw = nsh_platform_linux::run_app(source, options);
    // The window is closed: let the vault that is open now (it may have
    // changed in place) finish its queued writes.
    if let Some(vault) = session::boot().and_then(|b| b.vault) {
        let _ = runtime.block_on(vault.flush());
    }
    runtime.shutdown_timeout(Duration::from_secs(2));
    raw
}

fn ui_source(cli: &cli::Cli) -> AppSource {
    if let Some(url) = &cli.dev_server {
        if !cli::is_loopback_ws(url) {
            die(
                2,
                format!("rike: --dev-server must be a ws:// URL on this machine, got {url}"),
            );
        }
        #[cfg(feature = "dev")]
        return AppSource::DevServer(url.clone());
        #[cfg(not(feature = "dev"))]
        die(
            2,
            format!("rike: --dev-server {url} needs a build with `--features dev`"),
        );
    }
    if !BUNDLE_PRESENT {
        die(
            1,
            "rike: the UI bundle is missing; run scripts/build-release.sh, or use --dev-server",
        );
    }
    AppSource::Bundle(Bundle::source(BUNDLE.to_owned()).with_app_id(Some(APP_ID)))
}
