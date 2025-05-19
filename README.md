# `bmputil` companion utility to Black Magic Debug

[![Discord](https://img.shields.io/discord/613131135903596547?logo=discord)](https://discord.gg/P7FYThy)

A probe management utility for debuggers running the [Black Magic Debug firmware](https://black-magic.org/).

This tool is designed as a companion to be used along side probes running the Black Magic Debug firmware.
The idea behind this tool is to quickly and easily switch the firmware between multiple different releases
and variants for a given probe, and manage the probes - eg, forcing them into their bootloaders, or discovering
which you have connected and what their serial numbers are.

## Installation

Binary releases for Linux, Mac (arm64/AArch64) and Windows (amr64/AArch64) are now available with every
[release](https://github.com/blackmagic-debug/bmputil/releases). These should work out-of-the-box with no
extra dependencies or software needing to be installed.

Alternately, you can install directly from [crates.io](https://crates.io/crates/bmputil) with cargo.

First install Rust on your computer using `rustup`. Follow the instructions on the
[Rust Lang website](https://www.rust-lang.org/tools/install).

Then, install bmputil using `cargo install bmputil`. The tool will be available as `bmputil-cli`.

bmputil on Windows will automatically setup driver installation on first run for a probe if appropriate.
This will require administrator access when it occurs, and uses the Windows Driver Installer framework.

## Building from source

Alternatively, you can build and install the tool from source. This assumes that you have Rust (and
git, etc) installed already.

```sh
git clone https://github.com/blackmagic-debug/bmputil.git
cd bmputil
cargo b -r
```

If you are working on patches or contributions to the tool, you can obviously use `cargo build` and
`cargo run [params]` as needed.

### Windows

For building the tool on Windows, please see the
[Black Magic Debug website guide](https://black-magic.org/knowledge/bmputil-on-windows.html) on the process.

## Features

The first goal of this tool is to serve as a more ergonomic, dedicated to BMP DFU programmer. This utility is meant to replace the need for dfu-util and stm32_mem.py script. We can take advantage of the fact that we only have to support a specific target and DFU implementation to make for a nicer user experience. Additionally we can eventually provide automatic firmware update/upgrade commands as we know the location where to look for BMP firmwares. And even further, eventually, provide BMP specific configuration functions.

Currently implemented:

* Find and detect Black Magic Probe (BMP) debuggers connected to the system.
* Check firmware type and version on the attached BMPs.
* Flash Firmware using the DFU protocol onto the BMPs connected to the system.
* Search for new firmware releases.

Planned:

* Provide automated upgrade to newest command.
* Configure BMP firmware defaults. (will require firmware support for permanent settings)
* And many more... :)

## Getting Help

Discuss this project in the #blackmagic channel on the [1BitSquared discord server](https://discord.gg/P7FYThy).
