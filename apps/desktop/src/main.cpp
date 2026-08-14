#include "AppController.hpp"

#include <cstdio>

#include <QApplication>
#include <QCryptographicHash>
#include <QLocalServer>
#include <QLocalSocket>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQmlError>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QStandardPaths>
#include <QSettings>
#include <QTimer>
#include <QWindow>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("pptalk"));
    QGuiApplication::setOrganizationName(QStringLiteral("pptalk"));
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    const auto serverName = QStringLiteral("pptalk-") +
        QString::fromLatin1(QCryptographicHash::hash(
            QStandardPaths::writableLocation(QStandardPaths::AppDataLocation).toUtf8(),
            QCryptographicHash::Sha256).toHex().left(16));
    QString externalLink;
    for (const auto &argument : QCoreApplication::arguments()) {
        if (argument.startsWith(QStringLiteral("pptalk://"))) {
            externalLink = argument;
            break;
        }
    }
    QLocalSocket existing;
    existing.connectToServer(serverName);
    if (existing.waitForConnected(300)) {
        existing.write((externalLink + QLatin1Char('\n')).toUtf8());
        existing.waitForBytesWritten(300);
        return 0;
    }

    QLocalServer instanceServer;
    if (!instanceServer.listen(serverName)) {
        QLocalServer::removeServer(serverName);
        if (!instanceServer.listen(serverName)) return 2;
    }

    AppController controller;
    QObject::connect(&instanceServer, &QLocalServer::newConnection, &app,
                     [&instanceServer, &controller]() {
        while (auto *socket = instanceServer.nextPendingConnection()) {
            if (socket->bytesAvailable() == 0) socket->waitForReadyRead(100);
            const auto link = QString::fromUtf8(socket->readAll()).trimmed();
            if (!link.isEmpty()) controller.handleExternalLink(link);
            for (auto *window : QGuiApplication::topLevelWindows()) {
                window->show();
                window->raise();
                window->requestActivate();
            }
            socket->disconnectFromServer();
            socket->deleteLater();
        }
    });
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("App"), &controller);
    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const auto &warning : warnings) {
            std::fprintf(stderr, "QML warning: %s\n", qPrintable(warning.toString()));
        }
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [](const QUrl &url) {
        std::fprintf(stderr, "QML object creation failed for %s\n",
                     qPrintable(url.toString()));
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);
    engine.loadFromModule(QStringLiteral("Pptalk"), QStringLiteral("Main"));
    if (!engine.rootObjects().isEmpty()) {
        if (auto *window = qobject_cast<QWindow *>(engine.rootObjects().constFirst())) {
            QSettings settings;
            const auto width = settings.value(QStringLiteral("window/width"), 1240).toInt();
            const auto height = settings.value(QStringLiteral("window/height"), 780).toInt();
            window->resize(width, height);
            QObject::connect(&app, &QCoreApplication::aboutToQuit, window, [window]() {
                QSettings settings;
                settings.setValue(QStringLiteral("window/width"), window->width());
                settings.setValue(QStringLiteral("window/height"), window->height());
            });
            const auto screenshotPath = qEnvironmentVariable("PPTALK_SCREENSHOT_PATH");
            if (!screenshotPath.isEmpty()) {
                const auto panelName = qEnvironmentVariable("PPTALK_SCREENSHOT_PANEL");
                if (!panelName.isEmpty()) {
                    QTimer::singleShot(100, window, [window, panelName]() {
                        if (auto *panel = window->findChild<QObject *>(panelName)) {
                            QMetaObject::invokeMethod(panel, "open");
                        } else {
                            qWarning().noquote() << "Could not find UI panel" << panelName;
                        }
                    });
                }
                const auto delay = qEnvironmentVariableIntValue("PPTALK_SCREENSHOT_DELAY_MS");
                QTimer::singleShot(delay > 0 ? delay : 800, window,
                                   [window, screenshotPath]() {
                    if (auto *quickWindow = qobject_cast<QQuickWindow *>(window)) {
                        if (!quickWindow->grabWindow().save(screenshotPath)) {
                            qWarning().noquote() << "Could not save UI screenshot to"
                                                 << screenshotPath;
                        }
                    }
                    QCoreApplication::quit();
                });
            }
        }
    }
    if (QCoreApplication::arguments().contains(QStringLiteral("--minimized"))) {
        QTimer::singleShot(0, [] {
            for (auto *window : QGuiApplication::topLevelWindows()) window->hide();
        });
    }
    return app.exec();
}
