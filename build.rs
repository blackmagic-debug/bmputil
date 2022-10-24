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
    match std::env::var_os("CARGO_FEATURE_DETECT_BACKTRACE") {
        Some(_val) => {
            match version_meta() {
                Ok(version_meta) => {
                    if version_meta.channel == Channel::Nightly {
                        println!("cargo:rustc-cfg=feature=\"backtrace\"");
                    }
                },
                Err(e) => {
                    println!("cargo:warning=error detecting rustc version: {}", e);
                }
            }
        },
        None => (),
    }
}
