// CoreClient against the real pocketd on the current session bus (no mocks).
// Run: dbus-run-session -- env POCKETD_BIN=~/pocket-os/bin/pocketd ./tst_coreclient
#include <QDBusConnection>
#include <QDBusMessage>
#include <QProcess>
#include <QSignalSpy>
#include <QTest>

#include "coreclient.h"

class TstCoreClient : public QObject {
    Q_OBJECT

    QProcess core;

    void startCore()
    {
        core.setProgram(qEnvironmentVariable("POCKETD_BIN", QStringLiteral("pocketd")));
        core.setProcessChannelMode(QProcess::ForwardedChannels);
        core.start();
        QVERIFY(core.waitForStarted());
    }

private slots:
    void lifecycle()
    {
        // UI first, core not running yet.
        CoreClient client;
        QTRY_COMPARE(client.status(), CoreClient::Disconnected);
        QCOMPARE(client.connections(), 0);

        QSignalSpy finished(&client, &CoreClient::requestFinished);
        client.requestMode(QStringLiteral("dock"));
        QCOMPARE(finished.count(), 1);
        QCOMPARE(finished.takeFirst().at(1).toBool(), false);

        // Core starts after the UI: noticed through the name-owner watch.
        startCore();
        QTRY_COMPARE(client.status(), CoreClient::Connected);
        QCOMPARE(client.mode(), QStringLiteral("handheld"));
        QCOMPARE(client.protocol(), QStringLiteral("1.0"));
        QVERIFY(client.ready());
        QCOMPARE(client.connections(), 1);
        const QString firstOwner = client.owner();

        // UI -> core -> signal -> UI.
        client.requestMode(QStringLiteral("dock"));
        QTRY_COMPARE(finished.count(), 1);
        QCOMPARE(finished.takeFirst().at(1).toBool(), true);
        QTRY_COMPARE(client.mode(), QStringLiteral("dock"));

        // Invalid command: typed error, state unchanged.
        client.requestMode(QStringLiteral("desktop"));
        QTRY_COMPARE(finished.count(), 1);
        const auto args = finished.takeFirst();
        QCOMPARE(args.at(1).toBool(), false);
        QVERIFY(args.at(2).toString().startsWith(QStringLiteral("org.pocketos.Core1.Error.InvalidMode")));
        QCOMPARE(client.mode(), QStringLiteral("dock"));

        // A change made by another client reaches this one through StateChanged.
        auto call = QDBusMessage::createMethodCall(QStringLiteral("org.pocketos.Core1"),
                                                   QStringLiteral("/org/pocketos/Core1"),
                                                   QStringLiteral("org.pocketos.Core1"),
                                                   QStringLiteral("RequestMode"));
        call << QStringLiteral("handheld");
        QCOMPARE(QDBusConnection::sessionBus().call(call).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE(client.mode(), QStringLiteral("handheld"));
        client.requestMode(QStringLiteral("dock"));
        QTRY_COMPARE(client.mode(), QStringLiteral("dock"));

        // Core crashes (SIGKILL: no clean name release; the bus drops the name).
        core.kill();
        QVERIFY(core.waitForFinished());
        QTRY_COMPARE(client.status(), CoreClient::Disconnected);
        QVERIFY(!client.ready());

        // Core restarts: reconnect and resynchronise to the fresh core state.
        startCore();
        QTRY_COMPARE(client.status(), CoreClient::Connected);
        QCOMPARE(client.connections(), 2);
        QVERIFY(client.owner() != firstOwner);
        QCOMPARE(client.mode(), QStringLiteral("handheld"));
        client.requestMode(QStringLiteral("dock"));
        QTRY_COMPARE(client.mode(), QStringLiteral("dock"));

        // Clean stop.
        core.terminate();
        QVERIFY(core.waitForFinished());
        QCOMPARE(core.exitCode(), 0);
        QTRY_COMPARE(client.status(), CoreClient::Disconnected);
    }
};

QTEST_GUILESS_MAIN(TstCoreClient)
#include "tst_coreclient.moc"
