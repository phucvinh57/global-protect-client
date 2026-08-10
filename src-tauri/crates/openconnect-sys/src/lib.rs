#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

unsafe extern "C" {
	pub fn gp_progress_trampoline(
		privdata: *mut ::std::os::raw::c_void,
		level: ::std::os::raw::c_int,
		fmt: *const ::std::os::raw::c_char,
		...,
	);
}
