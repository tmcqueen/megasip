//! The C++ entry point that owns the QApplication.

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("app.h");
        fn run_qt_app(app_id: &str, icon_path: &str, start_hidden: bool) -> i32;
        /// Switch this process's app_id and window icon ("" for MegaSIP's own icon).
        fn set_app_identity(app_id: &str, icon_path: &str);
    }
}
