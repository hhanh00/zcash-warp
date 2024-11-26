fn main() {
    tonic_build();
    // create_c_bindings();
}

fn tonic_build() {
    tonic_build::configure()
        .out_dir("src/generated")
        .compile_protos(
            &["proto/service.proto", "proto/compact_formats.proto"],
            &["proto"],
        )
        .unwrap();
}

// Requires nightly rustc
#[allow(dead_code)]
fn create_c_bindings() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config::from_file("cbindgen.toml").unwrap();

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("./binding.h");
}
