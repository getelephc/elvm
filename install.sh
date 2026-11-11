#!/bin/sh
# elvm installer — installs the version manager only.
# Pick a compiler afterwards with: elvm install latest
set -eu

main() {
    repo="getelephc/elvm"
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
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 1' HUP INT TERM
    version="${ELVM_VERSION:-$(latest_version "$repo" "$tmp/latest.json")}"
    tarball="elvm-v${version}-${target}.tar.gz"
    base="https://github.com/${repo}/releases/download/v${version}"

    say "downloading elvm ${version} for ${target}"
    fetch_file "${base}/${tarball}" "${tmp}/${tarball}" 1 \
        || err "could not download ${base}/${tarball}"
    fetch_file "${base}/${tarball}.sha256" "${tmp}/${tarball}.sha256" 0 \
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

say() { printf '%s\n' "$1" >&2; }
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

# Keep retries on files: stdout cannot be rewound after a partial response.
# A subshell keeps these variables separate from the installer's state.
fetch_file() {
    (
        url="$1"
        output="$2"
        resume="$3"
        attempt=1
        backoff=1
        while :; do
            set -- -fsSL --connect-timeout 10 --speed-limit 1 --speed-time 30 \
                -D "$output.headers" -w '%{http_code}' -o "$output"
            if [ "$resume" -eq 1 ]; then
                set -- "$@" -C -
            else
                set -- "$@" --max-time 60
            fi
            if status="$(curl "$@" "$url")"; then
                # curl can report success for 416 while leaving an incomplete file.
                [ "$status" = 416 ] || return 0
                code=22
            else
                code="$?"
            fi
            case "$code" in
                22)
                    case "$status" in
                        408|429|5??) ;;
                        416)
                            [ "$resume" -eq 1 ] || return "$code"
                            rm -f "$output" || return 1
                            ;;
                        *) return "$code" ;;
                    esac
                    ;;
                33|36)
                    [ "$resume" -eq 1 ] && [ -s "$output" ] || return "$code"
                    rm -f "$output" || return 1
                    ;;
                6|7|16|18|28|35|52|55|56|92) ;;
                *) return "$code" ;;
            esac
            [ "$attempt" -lt 6 ] || return "$code"
            delay="$backoff"
            if [ -f "$output.headers" ]; then
                retry_after="$(tr -d '\r' < "$output.headers" | awk '
                    /^HTTP\// { value="" }
                    tolower($1) == "retry-after:" { $1=""; sub(/^ /, ""); value=$0 }
                    END { print value }')"
                case "$retry_after" in
                    '') ;;
                    *[!0-9]*)
                        retry_at="$(LC_ALL=C date -d "$retry_after" +%s 2>/dev/null \
                            || LC_ALL=C date -j -f '%a, %d %b %Y %H:%M:%S GMT' "$retry_after" +%s 2>/dev/null \
                            || true)"
                        if [ -n "$retry_at" ]; then
                            delay=$((retry_at - $(date +%s)))
                            [ "$delay" -ge 0 ] || delay=0
                        fi
                        ;;
                    *) delay="$retry_after" ;;
                esac
            fi
            # Do not retry before the server permits it or wait indefinitely.
            if [ "${#delay}" -gt 2 ] || [ "$delay" -gt 60 ]; then
                say "server requested a longer wait; please try again later"
                return "$code"
            fi
            say "download failed; retrying in ${delay}s (attempt $((attempt + 1))/6)"
            sleep "$delay"
            attempt=$((attempt + 1))
            backoff=$((backoff * 2))
        done
    )
}

latest_version() {
    repo="$1"
    fetch_file "https://api.github.com/repos/${repo}/releases/latest" "$2" 0 \
        || err "could not reach the GitHub API to find the latest elvm version"
    json="$(cat "$2")"
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
