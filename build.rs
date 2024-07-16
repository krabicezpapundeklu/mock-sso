use std::io::Result;

use static_files::NpmBuild;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=package-lock.json");
    println!("cargo:rerun-if-changed=client");
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=webpack.common.js");
    println!("cargo:rerun-if-changed=webpack.dev.js");
    println!("cargo:rerun-if-changed=webpack.prod.js");

    NpmBuild::new(".")
        .install()?
        .run("build")?
        .target("./dist")
        .to_resource_dir()
        .build()
}
