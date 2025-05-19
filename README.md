# `bmputil` companion utility to Black Magic Debug

[![Discord](https://img.shields.io/discord/613131135903596547?logo=discord)](https://discord.gg/P7FYThy)

A management utility for debuggers running the [Black Magic Debug firmware](https://black-magic.org/).

This project is currently still in early stages and under heavy development.

This tool can currently be used to update the Black Magic Debug firmware on your Black Magic Probe.

## Installation

Binary releases for Linux, Mac (arm64/AArch64) and Windows (amr64/AArch64) are now available with every
[release](https://github.com/blackmagic-debug/bmputil/releases). These should work out-of-the-box,
and do not require manual installation of Windows Driver Kit 8.0 or Rust.

Alternately, you can install directly from [crates.io](https://crates.io/crates/bmputil) with cargo.

First install Rust on your computer. Follow the instructions on the [Rust Lang website](https://www.rust-lang.org/tools/install).

Then, install bmputil using `cargo install bmputil`

bmputil on Windows will attempt to automatically setup driver installation on first run.
This is extra experimental, and will require administrator access on the first run.

## Building from source

Alternatively, you can build and install the tool from source. This assumes that you have Rust (and
git, etc) installed already.

```sh
git clone https://github.com/blackmagic-debug/bmputil.git
cd bmputil
cargo install --path .
```

If you are working on patches or contributions to the tool, you can obviously use `cargo build` and
`cargo run [params]` as needed.

### Windows

For building bmputil locally for a Windows platform (either on Windows or cross-compiling), you will
need to install the [Windows Driver Kit 8.0 redistributable components](https://go.microsoft.com/fwlink/p/?LinkID=253170)
(link from [this](https://learn.microsoft.com/en-us/windows-hardware/drivers/other-wdk-downloads) page).
If you are cross compiling to Windows, you will need to set the `WDK_DIR` environment variable to
the path of the extracted WDK redistributable components.

## Features

The first goal of this tool is to serve as a more ergonomic, dedicated to BMP DFU programmer. This utility is meant to replace the need for dfu-util and stm32_mem.py script. We can take advantage of the fact that we only have to support a specific target and DFU implementation to make for a nicer user experience. Additionally we can eventually provide automatic firmware update/upgrade commands as we know the location where to look for BMP firmwares. And even further, eventually, provide BMP specific configuration functions.

Currently implemented:

* Find and detect Black Magic Probe (BMP) debuggers connected to the system.
* Check firmware type and version on the attached BMPs.
* Flash Firmware using the DFU protocol onto the BMPs connected to the system.

Planned:

* Search for new firmware releases.
* Provide automated upgrade to newest command.
* Configure BMP firmware defaults. (will require firmware support for permanent settings)
* And many more... :)

## Getting Help

Discuss this project in the #blackmagic channel on the [1BitSquared discord server](https://discord.gg/P7FYThy).
