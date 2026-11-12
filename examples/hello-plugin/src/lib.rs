//! A library with no PWRS in it, as a plugin another program loads is. The
//! hello example's `libraries` table ships it in `runtimes/<rid>/plugins`.

/// What a caller reads to show the library loaded and ran.
#[unsafe(no_mangle)]
pub extern "C" fn hello_plugin_answer() -> i32 {
    42
}
