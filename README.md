<p align="center">
  <img src="assets/logo-mark.png" alt="elvm logo" width="130">
</p>

<h1 align="center">elvm</h1>

<p align="center">
  <em>Pin a compiler. Ship the same one everywhere.</em>
</p>

<p align="center">
  <a href="https://github.com/getelephc/elvm/stargazers"><img src="https://img.shields.io/github/stars/getelephc/elvm?style=flat-square&logo=github&logoColor=white&label=stars&color=FF7A1A" alt="Stars"></a>
  <a href="https://github.com/getelephc/elvm/releases"><img src="https://img.shields.io/github/downloads/getelephc/elvm/total?style=flat-square&logo=github&logoColor=white&label=downloads&color=FF7A1A" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/getelephc/elvm?style=flat-square&color=FF7A1A" alt="License: MIT"></a>
  <a href="https://x.com/nahime0"><img src="https://img.shields.io/badge/Follow-%40nahime0-FF7A1A?style=flat-square&logo=x&logoColor=white" alt="Follow @nahime0 on X"></a>
</p>

<p align="center">
  <strong>single binary &middot; no shell hooks &middot; per-project pinning &middot; builds from source</strong>
</p>

<p align="center">
  Version manager for <a href="https://github.com/illegalstudio/elephc">elephc</a>, the PHP-to-native compiler. <code>~/.elvm/bin</code> goes on your <code>PATH</code> once; from then on a committed <code>.elephc-version</code> decides which compiler runs, so every contributor and every CI job builds with the same one. No shell function to source and no <code>cd</code> hook &mdash; a shim resolves the version and <code>exec</code>s the real binary, so exit codes, signals and TTY behaviour are indistinguishable from calling <code>elephc</code> directly.
</p>

<p align="center">
  <a href="https://elephc.dev"><strong>Official Website</strong></a>
</p>

---

## Install

    curl -fsSL https://get.elephc.dev | sh
    elvm install latest

The installer prompts to add `~/.elvm/bin` to your `PATH` and installs elvm itself. It
does not download a compiler — you choose the version. After installation, start a
new shell for the `PATH` to update.

## Use

    elvm install 0.26.4        # install a specific version
    elvm install nightly       # install the newest build of main
    elvm use 0.26.4            # pin it for this project (.elephc-version)
    elvm use 0.26.4 --global   # set the default
    elvm ls                    # what is installed
    elvm ls-remote             # what is published
    elvm doctor                # diagnose PATH and installation problems

Commit `.elephc-version` and every contributor gets the same compiler:

    elvm install              # reads .elephc-version

## How version selection works

The first of these that applies wins:

1. `$ELEPHC_VERSION`
2. the nearest `.elephc-version`, searching upward from the current directory
3. the global default set by `elvm use --global`

`latest` in a version file means the highest **installed** version; `elvm
install latest` means the newest **published** one. Running `elephc` never
downloads anything — if the selected version is missing, elvm tells you which
command installs it.

## Nightly builds

elephc publishes unattended builds of `main` as pre-releases. They are not
supported: no compatibility, stability, or upgrade guarantees.

    elvm install nightly       # the newest build of main
    elvm install nightly-20260901   # one specific build

`nightly` is a channel, not a version. Installing it again is an update, not
an error, and it never takes part in version selection: `latest` and prefixes
like `0.26` only ever match released versions, so a nightly on disk cannot
change what an existing `.elephc-version` resolves to. `elvm ls-remote` lists
the dated builds; `elvm ls` and `elvm doctor` show which one is installed,
since every rolling install is called `nightly` no matter which build is in
it.

**`nightly` is not a pin.** It names whichever build was newest when each
person last installed it, so a committed `.elephc-version` saying `nightly`
gives different compilers to different machines — the one thing a version
file exists to prevent. A dated tag like `nightly-20260901` *is* a pin,
because upstream never republishes one.

Dated builds are kept upstream for 14 nightlies and then deleted, tag
included. So a dated pin is reproducible for about two weeks and no longer:
after that, only someone who already has it installed — or still has the
tarball in `~/.elvm/cache/downloads` — can reinstall it. Pin a release for
anything that has to build a year from now.

## Platform support

Which platforms have a downloadable binary is a property of each elephc
release, not a fixed list — check `elvm ls-remote` for the version and
platform you need. As of elephc v0.25.2, releases publish macOS ARM64
(`aarch64-apple-darwin`), Linux x86_64 (`x86_64-unknown-linux-gnu`), and Linux
ARM64 (`aarch64-unknown-linux-gnu`); older releases (through v0.24.x) are
macOS ARM64 only. Wherever a binary isn't published, build from source:

    elvm install --build v0.26.4    # requires Rust and git

## License

MIT
