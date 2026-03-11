/// Build script for crous-python.
///
/// When built by maturin (`MATURIN_BUILD_TRIPLE` is set), maturin handles all
/// Python linking itself, so we do nothing.
///
/// When built with plain `cargo build` / `cargo test` the `extension-module`
/// feature intentionally suppresses PyO3's own Python link flags (correct for
/// a redistributable `.so` that must work with any Python). Without extra help
/// the linker therefore cannot resolve Python C-API symbols.  This script
/// detects that case and emits the framework search-path + library name so the
/// local macOS Python framework is found.
///
/// On non-Apple platforms the dylib lives in LIBDIR, not a framework bundle,
/// so we fall back to `-lpython3.X` there.
fn main() {
    // Tell cargo to re-run this script only when relevant env vars change.
    println!("cargo:rerun-if-env-changed=MATURIN_BUILD_TRIPLE");
    println!("cargo:rerun-if-env-changed=PYO3_BUILD_EXTENSION_MODULE");
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=PYTHON_SYS_EXECUTABLE");

    // If maturin is driving this build it handles Python linking on its own.
    // PYO3_BUILD_EXTENSION_MODULE is always set by maturin (including inside Docker).
    // MATURIN_BUILD_TRIPLE is a secondary fallback for non-Docker maturin builds.
    if std::env::var("PYO3_BUILD_EXTENSION_MODULE").is_ok()
        || std::env::var("MATURIN_BUILD_TRIPLE").is_ok()
    {
        return;
    }

    // --- Resolve Python interpreter -----------------------------------------------------------
    // Respect an explicit override, otherwise fall back to `python3`.
    let python = std::env::var("PYO3_PYTHON")
        .or_else(|_| std::env::var("PYTHON_SYS_EXECUTABLE"))
        .unwrap_or_else(|_| "python3".into());

    // Ask Python where its framework / library lives.
    let query = r#"
import sysconfig, sys
cfg = sysconfig.get_config_vars()
framework_prefix = cfg.get('PYTHONFRAMEWORKPREFIX', '')
framework_dir    = cfg.get('PYTHONFRAMEWORKDIR', '')
libdir           = cfg.get('LIBDIR', '')
ldlibrary        = cfg.get('LDLIBRARY', '')
ver              = sysconfig.get_python_version()
print(framework_prefix)
print(framework_dir)
print(libdir)
print(ldlibrary)
print(ver)
"#;

    let out = std::process::Command::new(&python)
        .args(["-c", query])
        .output()
        .expect("failed to run Python interpreter – set PYO3_PYTHON to an accessible Python 3");

    if !out.status.success() {
        panic!(
            "Python sysconfig query failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    let framework_prefix = lines.next().unwrap_or("").trim().to_owned();
    let framework_dir    = lines.next().unwrap_or("").trim().to_owned();
    let libdir           = lines.next().unwrap_or("").trim().to_owned();
    let _ldlibrary       = lines.next().unwrap_or("").trim().to_owned();
    let _version         = lines.next().unwrap_or("").trim().to_owned();

    // --- Emit link directives -----------------------------------------------------------------
    if !framework_prefix.is_empty() && !framework_dir.is_empty() {
        // macOS framework build – link the whole Python framework bundle.
        // `framework_dir` is typically "Python.framework"; strip any ".framework" suffix to get
        // the name passed to `-framework <Name>`.
        let framework_name = framework_dir
            .strip_suffix(".framework")
            .unwrap_or(&framework_dir)
            .to_owned();

        // cargo:rustc-link-search=framework=<path>  →  -F <path>
        println!("cargo:rustc-link-search=framework={framework_prefix}");
        // cargo:rustc-link-lib=framework=<name>     →  -framework <name>
        println!("cargo:rustc-link-lib=framework={framework_name}");
    } else if !libdir.is_empty() {
        // Non-framework build (Linux, Windows, non-framework macOS).
        // Link against the shared / static Python library in LIBDIR.
        println!("cargo:rustc-link-search=native={libdir}");
        // Derive `pythonX.Y` from the library filename or fall back to `python3`.
        let lib_name = _ldlibrary
            .trim_start_matches("lib")
            .trim_end_matches(".so")
            .trim_end_matches(".dylib")
            .trim_end_matches(".a")
            // Strip version suffix like ".3.14" that isn't part of the -l flag
            .split('.').next()
            .unwrap_or("python3")
            .to_owned();
        println!("cargo:rustc-link-lib={lib_name}");
    }
}
