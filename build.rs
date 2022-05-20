
use rustc_version::{version_meta, Channel};

fn main()
{
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
