//! Probe the *specific* API we need, not the compiler release channel.
//! cfg_rust_features 0.1.2 does not support proc_macro_span; a targeted
//! compile probe also keeps working if these APIs stabilize in the future.

use std::{env, io::Write, process::{Command, Stdio}};

fn probe(rustc: &std::ffi::OsStr, out_dir: &std::path::Path, gated: bool) -> bool {
    let mut cmd = Command::new(rustc);
    cmd.args(["--crate-name", "tracing_span_probe", "--crate-type", "lib",
              "--edition", "2018", "--emit", "metadata", "-o"])
        .arg(out_dir.join("tracing_span_probe.rmeta"))
        .arg("-");
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else { return false };
    let code = if gated {
        "#![feature(proc_macro_span)]\n"
    } else {
        ""
    };
    let input = format!(
        "{}extern crate proc_macro;\n         pub fn check() {{\n           let span = proc_macro::Span::call_site();\n           let _ = span.parent();\n         }}\n",
        code
    );
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(input.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
    }
    child.wait().map(|status| status.success()).unwrap_or(false)
}

fn main() {
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rustc-check-cfg=cfg(tracing_proc_macro_span)");
    println!("cargo:rustc-check-cfg=cfg(tracing_proc_macro_span_unstable)");

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let out_dir = std::path::PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));

    if probe(&rustc, &out_dir, false) {
        // Feature already stable: don't emit an unnecessary #![feature] gate.
        println!("cargo:rustc-cfg=tracing_proc_macro_span");
    } else if probe(&rustc, &out_dir, true) {
        // Nightly (or RUSTC_BOOTSTRAP) where the unstable API is available.
        println!("cargo:rustc-cfg=tracing_proc_macro_span");
        println!("cargo:rustc-cfg=tracing_proc_macro_span_unstable");
    }
}
