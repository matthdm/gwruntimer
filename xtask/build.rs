fn main() {
    // xtask is built with the workspace's default target (see .cargo/config.toml),
    // so its own target triple is also the triple whose `target/<triple>/release`
    // directory holds the artifacts we package.
    println!("cargo:rustc-env=BUILD_TARGET={}", std::env::var("TARGET").unwrap());
    println!("cargo:rerun-if-changed=build.rs");
}
