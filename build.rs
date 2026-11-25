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
            // System headers: their warnings (e.g. KI18n's -Wattributes) aren't ours.
            for dir in ["/usr/include/KF6/KI18n", "/usr/include/x86_64-linux-gnu/qt6/QtQmlIntegration"] {
                cc.flag("-isystem");
                cc.flag(dir);
            }
        })
    };
    builder.build();
    // Link these after the generated archive that uses them. cxx-qt lists Qt Widgets/
    // Quick/QuickControls2 before it, and GNU ld with --as-needed (the Debian/Ubuntu
    // default) drops a library that nothing *earlier* on the line needs, which leaves
    // QApplication, QQuickStyle and QQuickWindow undefined.
    for lib in ["Qt6Widgets", "Qt6QuickControls2", "Qt6Quick", "Qt6Gui", "Qt6Qml", "Qt6Core", "KF6I18n", "KF6I18nQml"] {
        println!("cargo::rustc-link-lib=dylib={lib}");
    }
}
