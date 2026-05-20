fn main() {
    let flecs_c = "third_party/flecs/distr/flecs.c";
    let flecs_h = "third_party/flecs/distr/flecs.h";

    println!("cargo:rerun-if-changed={flecs_c}");
    println!("cargo:rerun-if-changed={flecs_h}");
    println!("cargo:rerun-if-changed=build.rs");

    let mut build = cc::Build::new();
    build
        .file(flecs_c)
        .include("third_party/flecs/distr")
        .define("flecs_STATIC", None)
        .define("FLECS_NO_CPP", None)
        .warnings(false);

    if build.get_compiler().is_like_msvc() {
        build.flag_if_supported("/std:c11");
    } else {
        build.flag_if_supported("-std=c99");
        build.flag_if_supported("-Wno-unused-parameter");
        build.flag_if_supported("-Wno-missing-field-initializers");
    }

    build.compile("flecs");
}
