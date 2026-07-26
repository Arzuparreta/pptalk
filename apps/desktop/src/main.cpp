#include "AppController.hpp"

#include <QApplication>
#include <QCryptographicHash>
#include <QLocalServer>
#include <QLocalSocket>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
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
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [] { QCoreApplication::exit(-1); }, Qt::QueuedConnection);
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
        }
    }
    if (QCoreApplication::arguments().contains(QStringLiteral("--minimized"))) {
        QTimer::singleShot(0, [] {
            for (auto *window : QGuiApplication::topLevelWindows()) window->hide();
        });
    }
    return app.exec();
}
