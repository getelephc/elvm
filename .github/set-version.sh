#!/bin/sh
# Rewrite Cargo.toml and Cargo.lock to a given version, then verify the
# rewrite took effect.
#
# Used by .github/workflows/release.yml in two places: the build job's
# per-leg local rewrite (never committed, just used to build the right
# binary) and the release job's rewrite that gets committed back to main.
# One copy means a fix to the rewrite, or to the guard below, applies to
# both call sites instead of risking one getting fixed and the other missed.
#
# Usage: set-version.sh VERSION
#   VERSION - the crate version to set, without a leading "v" (e.g. 1.2.3)
#
# Must run from the repository root: it rewrites ./Cargo.toml and
# ./Cargo.lock in place.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 VERSION" >&2
    exit 1
fi
VERSION="$1"

# Cargo.toml: rewrite only the first `version = "..."` line, so a
# dependency's inline `{ version = "...", ... }` table entry (which never
# starts the line) is never touched.
awk -v ver="$VERSION" '
  !done && /^version = / { sub(/version = ".*"/, "version = \"" ver "\""); done = 1 }
  { print }
' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

# Cargo.lock: rewrite only the elvm package's own version line, not the
# version line of any dependency package.
awk -v ver="$VERSION" '
  /^name = "elvm"$/ { inpkg = 1 }
  inpkg && /^version = / { sub(/version = ".*"/, "version = \"" ver "\""); inpkg = 0 }
  { print }
' Cargo.lock > Cargo.lock.tmp
mv Cargo.lock.tmp Cargo.lock

# Guard against the failure that motivated pulling this into its own
# script: a rewrite that silently does nothing (Cargo.toml reformatted,
# the pattern stops matching) must not produce a binary mislabelled with
# the old version, and must not let a commit-back step push an empty
# "bump" commit while reporting success. awk exits 0 whether or not its
# pattern matched, so this has to be checked explicitly.
ACTUAL="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
if [ "$ACTUAL" != "$VERSION" ]; then
    echo "Cargo.toml version is '${ACTUAL}' after the rewrite, but expected '${VERSION}'" >&2
    exit 1
fi
