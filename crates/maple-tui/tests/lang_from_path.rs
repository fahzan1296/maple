//! Port of `l/test/lang-from-path.test.ts`.

use maple_tui::lang_from_path::{detect_language_id, get_language_from_path};

#[test]
fn detects_dockerfile_by_basename_case_insensitively() {
	assert_eq!(get_language_from_path("Dockerfile"), Some("dockerfile"));
	assert_eq!(get_language_from_path("dockerfile"), Some("dockerfile"));
	assert_eq!(get_language_from_path("Dockerfile.dev"), Some("dockerfile"));
	assert_eq!(get_language_from_path("DOCKERFILE"), Some("dockerfile"));
}

#[test]
fn detects_containerfile() {
	assert_eq!(get_language_from_path("Containerfile"), Some("dockerfile"));
}

#[test]
fn detects_env_files_by_prefix() {
	assert_eq!(get_language_from_path(".env.local"), Some("env"));
	assert_eq!(get_language_from_path(".env.production"), Some("env"));
	assert_eq!(get_language_from_path(".env"), Some("env"));
}

#[test]
fn detects_emacs() {
	assert_eq!(get_language_from_path(".emacs"), Some("emacs-lisp"));
}

#[test]
fn detects_justfile() {
	assert_eq!(get_language_from_path("justfile"), Some("just"));
}

#[test]
fn detects_cmake_lists_as_cmake() {
	assert_eq!(get_language_from_path("CMakeLists.txt"), Some("cmake"));
	assert_eq!(get_language_from_path("cmakelists.txt"), Some("cmake"));
}

#[test]
fn is_case_insensitive_on_extensions() {
	assert_eq!(get_language_from_path("Main.TS"), Some("typescript"));
	assert_eq!(get_language_from_path("App.TSX"), Some("tsx"));
}

#[test]
fn returns_none_for_unknown_extensions() {
	assert_eq!(get_language_from_path("file.unknownext"), None);
	assert_eq!(get_language_from_path("file.xyz123"), None);
}

#[test]
fn returns_none_for_files_with_no_extension() {
	assert_eq!(get_language_from_path("README"), None);
}

#[test]
fn returns_the_last_extension_when_multiple_dots_are_present() {
	assert_eq!(get_language_from_path("config.test.ts"), Some("typescript"));
}

#[test]
fn handles_full_paths_with_directories() {
	assert_eq!(
		get_language_from_path("/home/user/project/src/index.ts"),
		Some("typescript")
	);
	assert_eq!(
		get_language_from_path("C:\\Users\\dev\\app\\main.rs"),
		Some("rust")
	);
}

#[test]
fn resolves_cuda_sources_and_headers_as_cpp() {
	assert_eq!(get_language_from_path("kernel.cu"), Some("cpp"));
	assert_eq!(get_language_from_path("kernel.cuh"), Some("cpp"));
}

#[test]
fn detect_language_id_dockerfile() {
	assert_eq!(detect_language_id("Dockerfile"), "dockerfile");
	assert_eq!(detect_language_id("dockerfile.dev"), "dockerfile");
}

#[test]
fn detect_language_id_containerfile() {
	assert_eq!(detect_language_id("Containerfile"), "dockerfile");
}

#[test]
fn detect_language_id_emacs() {
	assert_eq!(detect_language_id(".emacs"), "emacs-lisp");
}

#[test]
fn detect_language_id_makefile() {
	assert_eq!(detect_language_id("Makefile"), "makefile");
	assert_eq!(detect_language_id("makefile"), "makefile");
	assert_eq!(detect_language_id("gnumakefile"), "makefile");
}

#[test]
fn detect_language_id_justfile() {
	assert_eq!(detect_language_id("justfile"), "just");
}

#[test]
fn detect_language_id_cmake() {
	assert_eq!(detect_language_id("CMakeLists.txt"), "cmake");
	assert_eq!(detect_language_id("FindPackage.cmake"), "cmake");
}

#[test]
fn detect_language_id_falls_back_to_plaintext() {
	assert_eq!(detect_language_id("file.unknownext"), "plaintext");
	assert_eq!(detect_language_id("README"), "plaintext");
}

#[test]
fn detect_language_id_cuda_is_cpp() {
	assert_eq!(detect_language_id("kernel.cu"), "cpp");
	assert_eq!(detect_language_id("kernel.cuh"), "cpp");
}
