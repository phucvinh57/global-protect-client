fn main() {
	let library = pkg_config::Config::new()
		.probe("openconnect")
		.expect("libopenconnect development files are required");

	let bindings = bindgen::Builder::default()
		.header("/usr/include/openconnect.h")
		.allowlist_function("openconnect_.*")
		.allowlist_type("openconnect_.*")
		.allowlist_type("oc_.*")
		.allowlist_var("OC_.*")
		.allowlist_var("PRG_.*")
		.generate_comments(false)
		.generate()
		.expect("could not generate libopenconnect bindings");
	bindings
		.write_to_file(std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bindings.rs"))
		.expect("could not write libopenconnect bindings");

	let mut cc = cc::Build::new();
	cc.file("src/shim.c");
	for include in library.include_paths {
		cc.include(include);
	}
	cc.compile("gp_openconnect_shim");
	println!("cargo:rerun-if-changed=src/shim.c");
	println!("cargo:rerun-if-changed=/usr/include/openconnect.h");
}
