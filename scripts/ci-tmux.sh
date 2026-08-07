#!/usr/bin/env bash
set -euo pipefail

# Pin the release and checksum together so a runner image or environment cannot
# silently select an older package or a different source archive.
tmux_version="3.6"
tmux_sha256="136db80cfbfba617a103401f52874e7c64927986b65b1b700350b6058ad69607"
tmux_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/stay-tmux-${tmux_version}"
archive="${tmux_root}.tar.gz"
source_root="${tmux_root}-source"

verify_tmux() {
    tmux -V
    installed_version="$(tmux -V | awk '{print $2}')"
    major="${installed_version%%.*}"
    minor="${installed_version#*.}"
    minor="${minor%%[!0-9]*}"
    if ((major < 3 || (major == 3 && minor < 6))); then
        echo "tmux 3.6 or newer is required; found ${installed_version}" >&2
        exit 1
    fi
}

if [[ "${1:-}" == "--verify" ]]; then
    verify_tmux
    exit 0
fi
if (($# != 0)); then
    echo "usage: $0 [--verify]" >&2
    exit 2
fi

case "$(uname -s)" in
    Linux)
        if ((EUID == 0)); then
            apt_command=(apt-get)
        else
            apt_command=(sudo apt-get)
        fi
        "${apt_command[@]}" update
        "${apt_command[@]}" install --yes bison build-essential libevent-dev libncurses-dev pkg-config ripgrep zsh
        jobs="$(nproc)"
        checksum_command=(sha256sum --check --status)
        ;;
    Darwin)
        brew untap aws/tap >/dev/null 2>&1 || true
        brew install zsh libevent ncurses pkg-config
        libevent_prefix="$(brew --prefix libevent)"
        ncurses_prefix="$(brew --prefix ncurses)"
        export PKG_CONFIG_PATH="${libevent_prefix}/lib/pkgconfig:${ncurses_prefix}/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
        jobs="$(sysctl -n hw.ncpu)"
        checksum_command=(shasum -a 256 -c -)
        ;;
    *)
        echo "unsupported CI host: $(uname -s)" >&2
        exit 1
        ;;
esac

curl --fail --silent --show-error --location \
    --output "$archive" \
    "https://github.com/tmux/tmux/releases/download/${tmux_version}/tmux-${tmux_version}.tar.gz"
echo "${tmux_sha256}  ${archive}" | "${checksum_command[@]}"

mkdir -p "$source_root"
tar -xzf "$archive" --strip-components=1 --directory "$source_root"
(
    cd "$source_root"
    # CI's tmux fixtures use ASCII; disable the optional macOS Unicode
    # dependency so the pinned source build has the same explicit choice on
    # both platforms.
    ./configure --prefix="$tmux_root" --disable-utf8proc
    make -j"$jobs"
    make install
)

echo "${tmux_root}/bin" >>"${GITHUB_PATH:?GITHUB_PATH must be set by GitHub Actions}"
export PATH="${tmux_root}/bin:${PATH}"

verify_tmux
