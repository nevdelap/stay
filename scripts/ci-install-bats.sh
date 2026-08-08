#!/usr/bin/env bash
set -euo pipefail

bats_version="1.14.0"
bats_sha256="bb537b70b15b732f6d8827dd6578e3d8ce166636ce1f18ea9a074184fcce9177"
bats_base="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/stay-bats-${bats_version}"
archive="${bats_base}.tar.gz"
source_root="${bats_base}-source"
install_root="${bats_base}-install"
download_url="https://github.com/bats-core/bats-core/archive/refs/tags/v${bats_version}.tar.gz"

case "$(uname -s)" in
    Linux)
        checksum_command=(sha256sum --check --status)
        ;;
    Darwin)
        checksum_command=(shasum -a 256 -c -)
        ;;
    *)
        echo "unsupported CI host: $(uname -s)" >&2
        exit 1
        ;;
esac

curl --fail --silent --show-error --location \
    --output "$archive" \
    "$download_url"
echo "${bats_sha256}  ${archive}" | "${checksum_command[@]}"

mkdir -p "$source_root"
tar -xzf "$archive" --strip-components=1 --directory "$source_root"
bash "$source_root/install.sh" "$install_root"

export PATH="${install_root}/bin:${PATH}"
echo "${install_root}/bin" >>"${GITHUB_PATH:?GITHUB_PATH must be set by GitHub Actions}"

installed_version="$(bats --version)"
if [[ "$installed_version" != "Bats ${bats_version}" ]]; then
    echo "Bats ${bats_version} required; found ${installed_version}" >&2
    exit 1
fi
bats --version
