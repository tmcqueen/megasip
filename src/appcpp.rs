//! The C++ entry point that owns the QApplication.

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("app.h");
        fn run_qt_app(app_id: &str, start_hidden: bool) -> i32;
    }
}
