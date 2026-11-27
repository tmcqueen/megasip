// Creates a QApplication (widgets, for the tray menu and Plasma's desktop QtQuick style)
// and loads the QML UI. Nothing here needs KDE Frameworks: on Plasma the org.kde.desktop
// style is used when it's installed, Fusion elsewhere on Linux, and Fluent on Windows.
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
#include <QPalette>

#include <QFileInfo>
#include <QFontDatabase>
#include <QPixmap>
#include <QQuickImageProvider>

#include <algorithm>
#include <utility>

static QString fromRust(rust::Str s)
{
    return QString::fromUtf8(s.data(), s.size());
}

// image://icon/<name>: a themed icon as an image, for the QML Icon item.
class IconProvider : public QQuickImageProvider
{
public:
    IconProvider()
        : QQuickImageProvider(QQuickImageProvider::Pixmap)
    {
    }
    QPixmap requestPixmap(const QString &id, QSize *size, const QSize &requested) override
    {
        const QSize want(requested.width() > 0 ? requested.width() : 32, requested.height() > 0 ? requested.height() : 32);
        QPixmap px = QIcon::fromTheme(id).pixmap(want);
        if (px.isNull()) {
            px = QPixmap(want);
            px.fill(Qt::transparent);
        }
        if (size) {
            *size = px.size();
        }
        return px;
    }
};

// The style to use when QT_QUICK_CONTROLS_STYLE doesn't name one.
static QString defaultStyle(const QQmlApplicationEngine &engine)
{
#ifdef Q_OS_WIN
    Q_UNUSED(engine);
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
    return QStringLiteral("FluentWinUI3");
#else
    return QStringLiteral("Windows");
#endif
#else
    // Plasma's own style (qqc2-desktop-style) follows the desktop theme; it's optional.
    for (const QString &dir : engine.importPathList()) {
        if (QFileInfo::exists(dir + QStringLiteral("/org/kde/desktop/qmldir"))) {
            return QStringLiteral("org.kde.desktop");
        }
    }
    return QStringLiteral("Fusion");
#endif
}

// MegaSIP bundles the Breeze icons it uses (as :/icons/megasip-light and -dark). Where the
// desktop has no icon theme (Windows), they're the theme; elsewhere they fill in names the
// desktop's theme lacks.
static void setUpIcons()
{
    QIcon::setThemeSearchPaths(QIcon::themeSearchPaths() << QStringLiteral(":/icons"));
    const QColor window = QGuiApplication::palette().color(QPalette::Window);
    const QString ours = window.lightness() < 128 ? QStringLiteral("megasip-dark") : QStringLiteral("megasip-light");
    const QString current = QIcon::themeName();
    if (current.isEmpty() || current == QStringLiteral("hicolor")) {
        QIcon::setThemeName(ours);
    } else {
        QIcon::setFallbackThemeName(ours);
    }
}

static QIcon iconFor(const QString &path)
{
    if (!path.isEmpty()) {
        QIcon icon(path);
        if (!icon.isNull()) {
            return icon;
        }
    }
    return QIcon::fromTheme(QStringLiteral("org.megasip.MegaSIP"), QIcon::fromTheme(QStringLiteral("call-start")));
}

void set_app_identity(rust::Str app_id, rust::Str icon_path)
{
    QApplication::setWindowIcon(iconFor(fromRust(icon_path)));
    const QString id = fromRust(app_id);
    if (id == QGuiApplication::desktopFileName()) {
        return;
    }
    QGuiApplication::setDesktopFileName(id);
    // A Wayland window's app_id goes to the compositor when the window is mapped, and the
    // taskbar takes the icon from the .desktop file of that app_id. So re-map the windows
    // that are showing: hide dialogs before the windows they belong to, show them after.
    QWindow *focused = QGuiApplication::focusWindow();
    QList<QWindow *> shown;
    for (QWindow *w : QGuiApplication::topLevelWindows()) {
        if (w->isVisible() && (w->type() == Qt::Window || w->type() == Qt::Dialog)) {
            shown << w;
        }
    }
    std::stable_sort(shown.begin(), shown.end(), [](QWindow *a, QWindow *b) {
        return a->transientParent() != nullptr && b->transientParent() == nullptr;
    });
    for (QWindow *w : std::as_const(shown)) {
        w->hide();
    }
    for (auto it = shown.crbegin(); it != shown.crend(); ++it) {
        (*it)->show();
    }
    if (focused && shown.contains(focused)) {
        focused->requestActivate();
    }
}

int32_t run_qt_app(rust::Str app_id, rust::Str icon_path, bool start_hidden)
{
    static int argc = 1;
    static char arg0[] = "megasip";
    static char *argv[] = {arg0, nullptr};

    QApplication::setApplicationName(QStringLiteral("megasip"));
    QApplication::setApplicationDisplayName(QStringLiteral("MegaSIP"));
    QApplication::setOrganizationDomain(QStringLiteral("megasip.org"));
    QApplication::setDesktopFileName(fromRust(app_id));
    QApplication app(argc, argv);
    setUpIcons();
    QApplication::setWindowIcon(iconFor(fromRust(icon_path)));
    QApplication::setQuitOnLastWindowClosed(false);

    QQmlApplicationEngine engine;
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE")) {
        QQuickStyle::setStyle(defaultStyle(engine));
    }
    engine.addImageProvider(QStringLiteral("icon"), new IconProvider);
    engine.rootContext()->setContextProperty(QStringLiteral("fixedFontFamily"),
                                             QFontDatabase::systemFont(QFontDatabase::FixedFont).family());
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
            const int steps = 20;
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
