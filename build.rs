use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/sandbox_supervisor_main.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for (name, fixture) in [
        ("sandbox-supervisor", false),
        ("sandbox-supervisor-fixture", true),
    ] {
        let mut rustc = Command::new(env::var_os("RUSTC").unwrap());
        rustc
            .args(["--edition=2021", "-C", "opt-level=2", "--target"])
            .arg(env::var_os("TARGET").unwrap())
            .arg("src/sandbox_supervisor_main.rs")
            .arg("-o")
            .arg(out.join(name));
        let linker = format!(
            "CARGO_TARGET_{}_LINKER",
            env::var("TARGET").unwrap().replace('-', "_").to_uppercase()
        );
        if let Some(linker) = env::var_os(linker) {
            rustc
                .arg("-C")
                .arg(format!("linker={}", linker.to_string_lossy()));
        }
        if fixture {
            rustc.args(["--cfg", "supervisor_fixture"]);
        }
        assert!(rustc
            .status()
            .expect("compile sandbox supervisor")
            .success());
    }
}
