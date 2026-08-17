#!/bin/sh
# elvm installer — installs the version manager only.
# Pick a compiler afterwards with: elvm install latest
set -eu

main() {
    repo="illegalstudio/elvm"
    modify_path=1
    assume_yes=0
    for arg in "$@"; do
        case "$arg" in
            --no-modify-path) modify_path=0 ;;
            --yes|-y) assume_yes=1 ;;
            --help|-h) usage; return 0 ;;
            *) err "unknown option: $arg" ;;
        esac
    done

    need curl
    need tar
    need_digest_tool

    elvm_dir="${ELVM_DIR:-$HOME/.elvm}"
    target="$(detect_target)"
    version="${ELVM_VERSION:-$(latest_version "$repo")}"
    tarball="elvm-v${version}-${target}.tar.gz"
    base="https://github.com/${repo}/releases/download/v${version}"

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    say "downloading elvm ${version} for ${target}"
    curl -fsSL "${base}/${tarball}" -o "${tmp}/${tarball}" \
        || err "could not download ${base}/${tarball}"
    curl -fsSL "${base}/${tarball}.sha256" -o "${tmp}/${tarball}.sha256" \
        || err "could not download ${base}/${tarball}.sha256"

    verify "$tmp" "$tarball"

    mkdir -p "${elvm_dir}/bin"
    tar -xzf "${tmp}/${tarball}" -C "$tmp"
    mv "${tmp}/elvm" "${elvm_dir}/bin/elvm"
    chmod 755 "${elvm_dir}/bin/elvm"

    # Relative symlink: it survives an atomic replacement of the binary during
    # self-update, and keeps the directory relocatable.
    ln -sf elvm "${elvm_dir}/bin/elephc"

    if [ "$modify_path" -eq 1 ]; then
        configure_path "$elvm_dir" "$assume_yes"
    fi

    say ""
    say "elvm ${version} installed to ${elvm_dir}/bin"
    say ""
    say "next:  elvm install latest"
    say ""
}

usage() {
    cat <<'EOF'
Usage: install.sh [--yes] [--no-modify-path]

  --yes              do not prompt before editing a shell profile
  --no-modify-path   never edit a shell profile

Environment:
  ELVM_DIR       install location (default: $HOME/.elvm)
  ELVM_VERSION   version to install (default: latest release)
EOF
}

say() { printf '%s\n' "$1"; }
err() { printf 'error: %s\n' "$1" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || err "$1 is required but was not found"
}

get_digest_tool() {
    command -v shasum >/dev/null 2>&1 && printf 'shasum' && return 0
    command -v sha256sum >/dev/null 2>&1 && printf 'sha256sum' && return 0
    return 1
}

need_digest_tool() {
    get_digest_tool >/dev/null || err "neither shasum nor sha256sum is available to verify the download"
}

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "${os} ${arch}" in
        "Darwin arm64") printf 'aarch64-apple-darwin' ;;
        "Linux x86_64") printf 'x86_64-unknown-linux-gnu' ;;
        "Linux aarch64") printf 'aarch64-unknown-linux-gnu' ;;
        *) err "unsupported platform: ${os} ${arch}" ;;
    esac
}

latest_version() {
    repo="$1"
    json="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest")" \
        || err "could not reach the GitHub API to find the latest elvm version"
    version="$(printf '%s' "$json" | sed -n 's/.*"tag_name" *: *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$version" ] || err "could not determine the latest elvm version"
    printf '%s' "$version"
}

verify() {
    dir="$1"
    file="$2"
    expected="$(awk '{print $1}' "${dir}/${file}.sha256")"
    digest_tool="$(get_digest_tool)" || err "neither shasum nor sha256sum is available to verify the download"
    if [ "$digest_tool" = "shasum" ]; then
        actual="$(shasum -a 256 "${dir}/${file}" | awk '{print $1}')"
    else
        actual="$(sha256sum "${dir}/${file}" | awk '{print $1}')"
    fi
    [ "$expected" = "$actual" ] || err "checksum mismatch for ${file}"
}

configure_path() {
    dir="$1"
    yes="$2"
    line="export PATH=\"${dir}/bin:\$PATH\""

    case "${SHELL:-}" in
        */zsh) profile="$HOME/.zshrc" ;;
        */bash) profile="$HOME/.bashrc" ;;
        *) profile="$HOME/.profile" ;;
    esac

    if [ -f "$profile" ] && grep -qF "${dir}/bin" "$profile"; then
        return 0
    fi

    if [ "$yes" -eq 0 ]; then
        if [ -t 0 ]; then
            printf 'Add elvm to PATH in %s? [y/N] ' "$profile"
            read -r answer || answer=""
        elif { printf '' > /dev/tty; } 2>/dev/null; then
            # `[ -r /dev/tty ]` would pass here too: the device node is
            # world-readable even with no controlling terminal, so the
            # earlier write below fails with ENXIO under `set -eu` and
            # aborts the whole script after the binary is already
            # installed. Opening it is the only way to tell.
            printf 'Add elvm to PATH in %s? [y/N] ' "$profile" > /dev/tty
            read -r answer < /dev/tty || answer=""
        else
            say ""
            say "add this line to ${profile} yourself:"
            say "  ${line}"
            return 0
        fi
        case "$answer" in
            [Yy]|[Yy][Ee][Ss]) ;;
            *) say "skipped; add it yourself:  ${line}"; return 0 ;;
        esac
    fi

    printf '\n# elvm\n%s\n' "$line" >> "$profile"
    say "added elvm to ${profile}; restart your shell or run: ${line}"
}

main "$@"
