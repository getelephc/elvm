# elvm

Version manager for [elephc](https://github.com/illegalstudio/elephc).

## Install

    curl -fsSL https://get.elephc.dev | sh
    elvm install latest

The installer prompts to add `~/.elvm/bin` to your `PATH` and installs elvm itself. It
does not download a compiler — you choose the version. After installation, start a
new shell for the `PATH` to update.

## Use

    elvm install 0.26.4        # install a specific version
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
