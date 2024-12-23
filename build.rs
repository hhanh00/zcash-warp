fn main() {
    tonic_build();
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
