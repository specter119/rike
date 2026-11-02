//! Input method selection.
//!
//! On a Wayland compositor without `GTK_IM_MODULE`, GTK4 talks to the input
//! method through `zwp_text_input_v3`. The compositor (sway, seen at 1.12)
//! only sends that object its `enter` when the window gets the keyboard
//! focus, but NSH creates a text field's GTK widget, and with it the
//! text-input object, after the window is already focused. The input method
//! then never activates and every key arrives as a plain Latin letter, which
//! makes Chinese, Japanese and Korean input impossible. A plain GTK4
//! `TextView` created late behaves the same way
//! (`scripts/wayland-ime-smoke.sh gtk-late`).
//!
//! fcitx's own GTK module does not depend on that handshake, so it is
//! selected when the session says fcitx is in use and the module is there.

use std::path::Path;

const MODULE_DIRS: [&str; 4] = [
    "/usr/lib/gtk-4.0/4.0.0/immodules",
    "/usr/lib64/gtk-4.0/4.0.0/immodules",
    "/usr/lib/x86_64-linux-gnu/gtk-4.0/4.0.0/immodules",
    "/usr/lib/aarch64-linux-gnu/gtk-4.0/4.0.0/immodules",
];

/// The value `GTK_IM_MODULE` should get, if it should be set at all. An
/// explicit `GTK_IM_MODULE` always wins, so `GTK_IM_MODULE=wayland` opts out.
pub fn gtk_module(env: impl Fn(&str) -> Option<String>, installed: bool) -> Option<&'static str> {
    if env("GTK_IM_MODULE").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let says_fcitx = env("XMODIFIERS").is_some_and(|v| v.contains("@im=fcitx"))
        || env("QT_IM_MODULE").is_some_and(|v| v.starts_with("fcitx"));
    (says_fcitx && installed).then_some("fcitx")
}

fn fcitx_module_installed() -> bool {
    MODULE_DIRS.iter().any(|dir| {
        std::fs::read_dir(Path::new(dir)).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("libim-fcitx"))
        })
    })
}

/// Applies [`gtk_module`] to the process environment. Call it first thing in
/// `main`, before any thread exists and before GTK starts.
pub fn select_gtk_module() {
    let module = gtk_module(|k| std::env::var(k).ok(), fcitx_module_installed());
    if let Some(module) = module {
        // SAFETY: called at the top of `main`, while the process is still
        // single-threaded.
        unsafe { std::env::set_var("GTK_IM_MODULE", module) };
    }
}

#[cfg(test)]
mod tests {
    use super::gtk_module;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn fcitx_sessions_get_the_fcitx_module() {
        assert_eq!(
            gtk_module(env(&[("XMODIFIERS", "@im=fcitx")]), true),
            Some("fcitx")
        );
        assert_eq!(
            gtk_module(env(&[("QT_IM_MODULE", "fcitx5")]), true),
            Some("fcitx")
        );
    }

    #[test]
    fn an_explicit_choice_a_missing_module_or_another_im_is_left_alone() {
        let fcitx = ("XMODIFIERS", "@im=fcitx");
        assert_eq!(
            gtk_module(env(&[fcitx, ("GTK_IM_MODULE", "wayland")]), true),
            None
        );
        assert_eq!(gtk_module(env(&[fcitx]), false), None);
        assert_eq!(gtk_module(env(&[("XMODIFIERS", "@im=ibus")]), true), None);
        assert_eq!(gtk_module(env(&[]), true), None);
    }
}
