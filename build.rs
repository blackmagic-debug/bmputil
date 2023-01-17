// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2022-2023 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Mikaela Szekely <mikaela.szekely@qyriad.me>
//! To whomever might be reading this, understandably skeptical of build scripts:
//! This build script is *optional*, and exists only to set *default* options under
//! circumstances under which they are supported. Actually, all this build script does
//! is detect if we are running nightly Rust, and enable backtrace support for errors
//! if we are.

use rustc_version::{version_meta, Channel};

fn main()
{
    // Statically link the Visual C runtime on Windows.
    static_vcruntime::metabuild();

    // If detect-backtrace is enabled (default), detect if we're on nightly or not.
    // If we're on nightly, enable backtraces automatically.
    if let Some(_val) = std::env::var_os("CARGO_FEATURE_DETECT_BACKTRACE") {
        match version_meta() {
            Ok(version_meta) => {
                if version_meta.channel == Channel::Nightly {
                    // Tell Cargo to enable backtraces.
                    println!("cargo:rustc-cfg=feature=\"backtrace\"");
                }
            },
            Err(e) => {
                println!("cargo:warning=error detecting rustc version: {}", e);
            }
        }
    }
}
