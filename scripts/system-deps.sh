#!/usr/bin/env bash
# Make sure everything AirMail needs from the system is installed, and install
# whatever is missing with rvn. Every imlazy command that builds or runs
# AirMail depends on this, so a fresh checkout needs nothing done by hand.
#
# Checked by what the build actually looks for -- a pkg-config module or a
# command on PATH -- rather than by package name, so it costs a few
# milliseconds when all is well and also counts things installed some other
# way.
#
#   scripts/system-deps.sh           install what is missing
#   scripts/system-deps.sh --check   only report; exit 1 if anything is missing
set -euo pipefail

check_only=false
[[ ${1:-} == --check ]] && check_only=true

# package | how to tell it is there
requirements=(
    "rust|cmd:cargo"
    "gcc|cmd:cc"
    "pkgconf|cmd:pkg-config"
    "git|cmd:git"
    "gtk4|pc:gtk4 >= 4.12"
    "libadwaita|pc:libadwaita-1 >= 1.5"
    "webkitgtk-6.0|pc:webkitgtk-6.0"
    "desktop-file-utils|cmd:desktop-file-validate"
)

present() {
    local probe=$1
    case $probe in
        cmd:*) command -v "${probe#cmd:}" >/dev/null 2>&1 ;;
        # Without pkg-config nothing can be checked; pkgconf is listed first
        # among the libraries, so it gets installed in the same run.
        pc:*) command -v pkg-config >/dev/null 2>&1 && pkg-config --exists "${probe#pc:}" ;;
    esac
}

missing=()
for requirement in "${requirements[@]}"; do
    package=${requirement%%|*}
    probe=${requirement#*|}
    present "$probe" || missing+=("$package")
done

if ((${#missing[@]} == 0)); then
    exit 0
fi

echo "AirMail needs: ${missing[*]}"
if $check_only; then
    exit 1
fi

if ! command -v rvn >/dev/null 2>&1; then
    echo "rvn is not available; install these packages with your package manager." >&2
    exit 1
fi

# System packages need root. Official repositories only: these are all there,
# and nothing here should quietly build something from the AUR.
if ((EUID == 0)); then
    rvn install --repo-only --yes "${missing[@]}"
else
    sudo rvn install --repo-only --yes "${missing[@]}"
fi

# Say so if the install did not actually provide what was missing, rather than
# letting the build fail with a pkg-config error further down.
still=()
for requirement in "${requirements[@]}"; do
    present "${requirement#*|}" || still+=("${requirement%%|*}")
done
if ((${#still[@]} > 0)); then
    echo "Still missing after install: ${still[*]}" >&2
    exit 1
fi
echo "System dependencies installed."
