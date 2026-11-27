use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlFile, QmlModule};

/// QML files that are singletons (`pragma Singleton`).
const SINGLETONS: [&str; 2] = ["Theme.qml", "Units.qml"];

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
    let qml_files = qml_files.into_iter().map(|f| {
        let singleton = SINGLETONS.iter().any(|s| f.ends_with(s));
        QmlFile::from(f).singleton(singleton)
    });
    let builder = CxxQtBuilder::new_qml_module(QmlModule::new("org.megasip").qml_files(qml_files))
        .qt_module("Quick")
        .qt_module("Widgets")
        .qt_module("QuickControls2")
        .files(["src/bridge.rs", "src/appcpp.rs"])
        .cpp_file("cpp/app.cpp")
        .qrc_resources(icon_resources());
    // SAFETY: we only add include paths; nothing that conflicts with cxx-qt's own flags.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
        })
    };
    builder.build();
    // Link these after the generated archive that uses them. cxx-qt lists Qt Widgets/
    // Quick/QuickControls2 before it, and GNU ld with --as-needed (the Debian/Ubuntu
    // default) drops a library that nothing *earlier* on the line needs, which leaves
    // QApplication, QQuickStyle and QQuickWindow undefined. Other linkers don't care.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        for lib in [
            "Qt6Widgets",
            "Qt6QuickControls2",
            "Qt6Quick",
            "Qt6Gui",
            "Qt6Qml",
            "Qt6Core",
        ] {
            println!("cargo::rustc-link-lib=dylib={lib}");
        }
    }
}

/// The bundled icon themes (icons/megasip-light and -dark), as :/icons/<theme>/... resources.
/// A resource is named by the path given here, so build it with forward slashes.
fn icon_resources() -> QResources {
    println!("cargo::rerun-if-changed=icons");
    fn walk(dir: &str, files: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("icons dir").flatten() {
            let path = format!("{dir}/{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                walk(&path, files);
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk("icons", &mut files);
    files.sort();
    QResources::new().resource(QResource::new().prefix("/").files(files))
}
