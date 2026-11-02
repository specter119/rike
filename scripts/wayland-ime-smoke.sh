#!/usr/bin/env bash
# Types pinyin into a target app inside a throwaway headless sway with its
# own fcitx5, so the Wayland text-input-v3 path (the one a sway + fcitx5
# session without GTK_IM_MODULE uses) can be exercised without X11 and
# without touching the real desktop.
#
#   scripts/wayland-ime-smoke.sh {gtk|gtk-late|search|block|math|quit} [outdir]
#
# `math` does not type anything: it opens a vault with formulas and takes one
# screenshot (math-0-before.png), to check how they are drawn.
#
# `block` starts with the first block in edit mode (RIKE_DEBUG_EDIT_FIRST, debug
# builds only). The binary must embed the UI bundle: scripts/build-release.sh's
# `nsh build` step, then touch crates/rike-app/build.rs.
#
# Needs: sway, fcitx5, wtype, grim, dbus-run-session, python3 with PyGObject.
# Screenshots and logs land in outdir (default target/ime-smoke).
set -euo pipefail

usage="usage: $0 gtk|gtk-late|search|block|math|quit [outdir]"
target=${1:?$usage}
repo=$(cd "$(dirname "$0")/.." && pwd)
out=${2:-$repo/target/ime-smoke}

if [[ -z ${SX_IME_INNER:-} ]]; then
  # A private session bus keeps fcitx5-remote and fcitx5 away from the user's.
  exec env SX_IME_INNER=1 dbus-run-session -- "$0" "$@"
fi

mkdir -p "$out"
root=$(mktemp -d "$HOME/.cache/sxime.XXXXXX")
pids=()
cleanup() {
  for p in "${pids[@]:-}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  case $root in "$HOME"/.cache/sxime.*) /usr/bin/rm -rf "$root" ;; esac
}
trap cleanup EXIT

run=$root/run
mkdir -m 700 "$run"
export XDG_RUNTIME_DIR=$run XDG_CONFIG_HOME=$root/cfg XDG_DATA_HOME=$root/data
export XDG_CACHE_HOME=$root/cache XDG_STATE_HOME=$root/state
# Nothing may reach the real compositor.
unset DISPLAY WAYLAND_DISPLAY I3SOCK
export SWAYSOCK=$root/sway.sock

cat >"$root/sway.conf" <<CONF
xwayland disable
default_border none
output HEADLESS-1 resolution 1280x800
CONF
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c "$root/sway.conf" >"$out/sway.log" 2>&1 &
pids+=($!)

for _ in $(seq 50); do
  sock=$(compgen -G "$run/wayland-[0-9]*" | grep -v '\.lock$' | head -1 || true)
  [[ -n $sock ]] && break
  sleep 0.2
done
[[ -n ${sock:-} ]] || { echo "sway did not start; see $out/sway.log" >&2; exit 1; }
export WAYLAND_DISPLAY=$(basename "$sock")

# Guard: every output must be a headless one.
outputs=$(swaymsg -s "$SWAYSOCK" -t get_outputs | python3 -c \
  'import json,sys; print(" ".join(o["name"] for o in json.load(sys.stdin)))')
for name in $outputs; do
  [[ $name == HEADLESS-* ]] || { echo "refusing: output $name" >&2; exit 1; }
done

# The seat needs a keyboard before any client can get keyboard focus.
wtype -s 3600000 &
pids+=($!)
sleep 0.5
swaymsg -s "$SWAYSOCK" -t get_seats >"$out/seats.json"

mkdir -p "$XDG_CONFIG_HOME/fcitx5"
cp "$HOME/.config/fcitx5/profile" "$XDG_CONFIG_HOME/fcitx5/profile"
printf '[Behavior]\nActiveByDefault=True\n' >"$XDG_CONFIG_HOME/fcitx5/config"
fcitx5 --verbose default=5 >"$out/fcitx5.log" 2>&1 &
pids+=($!)
sleep 2

export GDK_BACKEND=wayland GSK_RENDERER=cairo GDK_DEBUG=no-portals
export GTK_A11Y=none GSETTINGS_BACKEND=memory
vault=$root/vault
mkdir -p "$vault"
cp -r "${RIKE_SMOKE_VAULT:-$repo/fixtures/vaults/basic}/." "$vault/"
case $target in
  gtk) cmd=(python3 "$repo/scripts/gtk4_textview.py") ;;
  gtk-late) cmd=(env GTK4_TEXTVIEW_DELAY_MS=1500 python3 "$repo/scripts/gtk4_textview.py") ;;
  search | math | quit) cmd=("$repo/target/debug/rike" --vault "$vault") ;;
  block) cmd=(env RIKE_DEBUG_EDIT_FIRST=1 "$repo/target/debug/rike" --vault "$vault") ;;
  *) echo "unknown target $target" >&2; exit 1 ;;
esac
if [[ $target == math ]]; then
  mkdir -p "$vault/journals"
  cat >"$vault/journals/2026_10_30.md" <<'MD'
- inline \(x^2\) and \(\frac{a}{b}\) and $\sum_{i=1}^{n} x_i$ sit on the text line
- \[ \int_0^1 \frac{\partial f}{\partial x}\,dx = \left\{\begin{array}{ll} 1 & x>0 \\ 0 & \text{else} \end{array}\right. \]
- broken $\begin{$ keeps its TeX
MD
fi
if [[ -n ${RIKE_SMOKE_VIEW:-} ]]; then
  # Start on a saved view, as the next start of a real vault does.
  mkdir -p "$XDG_STATE_HOME/rike"
  printf '%s\t%s\n' "$vault" "$RIKE_SMOKE_VIEW" >"$XDG_STATE_HOME/rike/vaults"
  cmd=("$repo/target/debug/rike")
fi
WAYLAND_DEBUG=client "${cmd[@]}" >"$out/app.log" 2>&1 &
pids+=($!)
sleep 5

if [[ $target == quit ]]; then
  # Ctrl+W must save and close the window: the app process has to exit.
  app=${pids[-1]}
  wtype -M ctrl -k w -m ctrl
  for _ in $(seq 20); do
    kill -0 "$app" 2>/dev/null || { echo "quit: the app exited"; exit 0; }
    sleep 0.5
  done
  grim "$out/quit-still-open.png"
  echo "quit: the app is still running after Ctrl+W" >&2
  exit 1
fi

if [[ $target == math ]]; then
  grim "$out/math-0-before.png"
  echo "done: screenshot in $out/math-0-before.png"
  exit 0
fi

if [[ $target == search ]]; then
  wtype -M ctrl -k k -m ctrl
  sleep 1
fi

fcitx5-remote -s pinyin || true
fcitx5-remote -o || true
{
  echo "im: $(fcitx5-remote -n)"
  echo "state: $(fcitx5-remote)"
} >"$out/fcitx5-remote.txt"
sleep 0.5

grim "$out/$target-0-before.png"
wtype -d 80 nihao
sleep 1
grim "$out/$target-1-preedit.png"
wtype -k space
sleep 1
grim "$out/$target-2-committed.png"

grep -E 'text_input|zwp_input' "$out/app.log" | head -40 >"$out/$target-text-input.txt" || true
echo "done: screenshots and logs in $out"
