// Creates a QApplication (so Plasma's desktop QtQuick style applies) and loads the QML UI.
#include "app.h"

#include <QApplication>
#include <QIcon>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QString>
#include <QQuickWindow>
#include <QGuiApplication>
#include <QWindow>
#include <QTimer>
#include <QImage>

#include <KLocalizedQmlContext>
#include <KLocalizedString>

int32_t run_qt_app(rust::Str app_id, bool start_hidden)
{
    static int argc = 1;
    static char arg0[] = "megasip";
    static char *argv[] = {arg0, nullptr};

    QApplication::setApplicationName(QStringLiteral("megasip"));
    QApplication::setApplicationDisplayName(QStringLiteral("MegaSIP"));
    QApplication::setOrganizationDomain(QStringLiteral("megasip.org"));
    QApplication::setDesktopFileName(QString::fromUtf8(app_id.data(), app_id.size()));
    QApplication app(argc, argv);
    QApplication::setWindowIcon(QIcon::fromTheme(QStringLiteral("org.megasip.MegaSIP"), QIcon::fromTheme(QStringLiteral("call-start"))));
    QApplication::setQuitOnLastWindowClosed(false);

    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE")) {
        QQuickStyle::setStyle(QStringLiteral("org.kde.desktop"));
    }

    KLocalizedString::setApplicationDomain("megasip");
    QQmlApplicationEngine engine;
    KLocalization::setupLocalizedContext(&engine);
    engine.rootContext()->setContextProperty(QStringLiteral("startHidden"), start_hidden);
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, &app,
        []() { QCoreApplication::exit(1); }, Qt::QueuedConnection);
    engine.loadFromModule("org.megasip", "Main");

    // Debug aid: MEGASIP_SCREENSHOT=/path/prefix renders each tab and preferences page to
    // PNG (prefix-<step>-<window>.png) and exits.
    const QString shot = qEnvironmentVariable("MEGASIP_SCREENSHOT");
    if (!shot.isEmpty() && !engine.rootObjects().isEmpty()) {
        auto *win = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        if (win) {
            win->show();
            const int steps = 10;
            for (int step = 0; step < steps; ++step) {
                QTimer::singleShot(2500 + step * 1200, win, [win, step]() { win->setProperty("shotStep", step); });
                QTimer::singleShot(3200 + step * 1200, win, [step, shot]() {
                    int n = 0;
                    for (QWindow *w : QGuiApplication::topLevelWindows()) {
                        auto *qw = qobject_cast<QQuickWindow *>(w);
                        if (qw && qw->isVisible()) {
                            qw->grabWindow().save(shot + QStringLiteral("-%1-%2.png").arg(step).arg(n++));
                        }
                    }
                });
            }
            QTimer::singleShot(3200 + steps * 1200, &app, []() { QCoreApplication::exit(0); });
        }
    }
    return app.exec();
}
