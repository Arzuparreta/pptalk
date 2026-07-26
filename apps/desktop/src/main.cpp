#include "AppController.hpp"

#include <QApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("pptalk"));
    QGuiApplication::setOrganizationName(QStringLiteral("pptalk"));
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    AppController controller;
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("App"), &controller);
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [] { QCoreApplication::exit(-1); }, Qt::QueuedConnection);
    engine.loadFromModule(QStringLiteral("Pptalk"), QStringLiteral("Main"));
    return app.exec();
}
