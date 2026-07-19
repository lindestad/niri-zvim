# Terminal launchers used by the optional portability smoke tests.
# shellcheck shell=bash

terminal_command() {
  case "$1" in
    alacritty) printf 'alacritty\n' ;;
    kitty) printf 'kitty\n' ;;
    foot) printf 'foot\n' ;;
    wezterm) printf 'wezterm\n' ;;
    *) return 1 ;;
  esac
}

terminal_app_id() {
  case "$1" in
    alacritty) printf 'Alacritty\n' ;;
    kitty) printf 'kitty\n' ;;
    foot) printf 'foot\n' ;;
    wezterm) printf 'org.wezfurlong.wezterm\n' ;;
    *) return 1 ;;
  esac
}

launch_test_terminal() {
  local terminal="$1"
  shift
  case "$terminal" in
    alacritty)
      alacritty --config-file /dev/null \
        -o 'window.dynamic_title=true' \
        --class Alacritty \
        -e "$@" >/dev/null 2>&1 &
      ;;
    kitty)
      kitty --config NONE --app-id kitty "$@" >/dev/null 2>&1 &
      ;;
    foot)
      foot --config /dev/null --app-id=foot "$@" >/dev/null 2>&1 &
      ;;
    wezterm)
      wezterm --skip-config start --always-new-process \
        --class org.wezfurlong.wezterm -- "$@" >/dev/null 2>&1 &
      ;;
    *)
      echo "unknown terminal fixture: $terminal" >&2
      return 2
      ;;
  esac
}
