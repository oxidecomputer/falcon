use cargo_toml::Manifest;
use quote::quote;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    get_propolis_version();
}

fn get_propolis_version() {
    let manifest = Manifest::from_path("../Cargo.toml")
        .expect("read workspace Cargo.toml");

    let workspace = manifest.workspace.expect("get workspace");

    let propolis = workspace
        .dependencies
        .get("propolis-client")
        .expect("build.rs: get propolis client dependency");

    let Some(rev) = propolis.git_rev() else {
        panic!("build.rs: expected git rev for propolis client");
    };

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("propolis_version.rs");

    let tokens = quote! { const PROPOLIS_REV: &str = #rev; };

    let file: syn::File =
        syn::parse2(tokens).expect("build.rs: parse generated code");
    let code = prettyplease::unparse(&file);

    fs::write(&dest_path, code).unwrap();

    println!("cargo::rerun-if-changed=../Cargo.toml");
}
