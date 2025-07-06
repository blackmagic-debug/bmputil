# `bmputil` companion utility to Black Magic Debug

[![Discord](https://img.shields.io/discord/613131135903596547?logo=discord)](https://discord.gg/P7FYThy)

A probe management utility for debuggers running the [Black Magic Debug firmware](https://black-magic.org/).

This tool is designed as a companion to be used along side probes running the Black Magic Debug firmware.
The idea behind this tool is to quickly and easily switch the firmware between multiple different releases
and variants for a given probe, and manage the probes - eg, forcing them into their bootloaders, or discovering
which you have connected and what their serial numbers are.

## Installation

Binary releases for Linux, mac OS (amd64/AArch64) and Windows (amd64/AArch64) are now available with every
[release](https://github.com/blackmagic-debug/bmputil/releases). These should work out-of-the-box with no
extra dependencies or software needing to be installed.

Alternatively `cargo binstall` can be used instead. Which allows for easy updates.
We recommend the following order of operations:
* [Install rustup](https://rustup.rs/)
* [Install cargo-binstall](https://github.com/cargo-bins/cargo-binstall?tab=readme-ov-file#installation)
* Install bmputil by invoking
  * `cargo binstall bmputil` for the stable release
  * `cargo binstall bmputil@1.0.0-rc.2` for a release candidate

The tool will be available as `bmputil-cli` starting with v1.0.0 and `bmputil` for older releases.

Another alternative is to use `cargo install` instead of `cargo binstall` which will install the tool from source. In
such case `cargo-binstall` can be skipped in the instructions above. The `binstall` path will fall back to source
compilation if a binary build is not available for the specific os/architecture combination.

`bmputil` on Windows will automatically setup driver installation on first run for a probe if appropriate.
This will require administrator access when it occurs, and uses the Windows Driver Installer framework.

## Building from source

Alternatively, you can build and install the tool from source if you want something newer than the latest
crates.io release. This assumes that you have Rust (and git, etc) installed already.

```sh
git clone https://github.com/blackmagic-debug/bmputil
cd bmputil
cargo b -r
```

You can then copy the resulting binary from `target/release/bmputil-cli` to some place on `$PATH`.

Alternatively, `cargo install --path .` can be used in place of `cargo b -r`, or
`cargo install https://github.com/blackmagic-debug/bmputil` in place of the manual clone and build to automate this.

If you are working on patches or contributions to the tool, then you can use `cargo build` (`cargo b`) and
`cargo run [params]` as needed to build test and run the tool. The `-r` (`--release`) option does a release
build.

### Windows

For building the tool on Windows, please see the
[Black Magic Debug website guide](https://black-magic.org/knowledge/bmputil-on-windows.html) on the process.

## Current Status

The first goal of this tool is to serve as a more ergonomic, dedicated to BMD, DFU programmer. This utility is meant
to replace the need for dfu-util and the old stm32_mem.py script. We take advantage of the fact that we only have to
support a specific target and a small number of DFU implementations to make for a nicer user experience. Additionally
we provide an automatic firmware switching command as we know the location where to look for BMD firmware builds. It
is planned to eventually provide BMD-specific configuration functions and automated build customisability, allowing
the tool to bake a firmware image for you that pulls together the combination of targets you care about.

Currently implemented:

* Find and detect Black Magic Probe (BMP) debuggers connected to the system.
* Check firmware type and version on the attached BMPs.
* Flash Firmware using the DFU protocol onto the BMPs connected to the system.
* Automated download of metadata as-needed to pick up new releases.
* Guided switching of the running firmware w/ automated firmware download.

Planned:

* Configure BMP firmware defaults. (will require firmware support for permanent settings)
* And many more... :)

## Getting Help

Discuss this project in the #blackmagic channel on the [1BitSquared discord server](https://discord.gg/P7FYThy).
