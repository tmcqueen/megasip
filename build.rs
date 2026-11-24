use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let qml_files = std::fs::read_dir("qml")
        .expect("qml dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "qml"))
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for f in &qml_files {
        println!("cargo::rerun-if-changed={f}");
    }
    println!("cargo::rerun-if-changed=cpp/app.cpp");
    println!("cargo::rerun-if-changed=cpp/app.h");
    let builder = CxxQtBuilder::new_qml_module(QmlModule::new("org.megasip").qml_files(qml_files))
        .qt_module("Quick")
        .qt_module("Widgets")
        .qt_module("QuickControls2")
        .files(["src/bridge.rs", "src/appcpp.rs"])
        .cpp_file("cpp/app.cpp");
    // SAFETY: we only add include paths; nothing that conflicts with cxx-qt's own flags.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            cc.include("/usr/include/KF6/KI18n");
            cc.include("/usr/include/x86_64-linux-gnu/qt6/QtQmlIntegration");
        })
    };
    builder.build();
    println!("cargo::rustc-link-lib=KF6I18n");
    println!("cargo::rustc-link-lib=KF6I18nQml");
}
