// CoreClient against the real pocketd on the current session bus (no mocks).
// Run: dbus-run-session -- env POCKETD_BIN=~/pocket-os/bin/pocketd ./tst_coreclient
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDir>
#include <QFile>
#include <QProcess>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include "appcatalog.h"
#include "coreclient.h"
#include "dockclient.h"

// Stand-in for the desktop's notification service, on the test's private bus.
class FakeNotifications : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.Notifications")
public:
    QStringList summaries;
public slots:
    uint Notify(const QString &, uint, const QString &, const QString &summary, const QString &,
                const QStringList &, const QVariantMap &, int)
    {
        summaries << summary;
        return 7;
    }
};

class TstCoreClient : public QObject {
    Q_OBJECT

    QProcess core;

    // Runs a CLI to completion and returns its stdout.
    static QString run(const QString &program, const QStringList &args, const QProcessEnvironment &env)
    {
        QProcess p;
        p.setProcessEnvironment(env);
        p.start(program, args);
        if (!p.waitForFinished(10000) || p.exitCode() != 0)
            qWarning().noquote() << program << args << "failed:" << p.readAllStandardError();
        return QString::fromUtf8(p.readAllStandardOutput());
    }
    static QString field(const QString &out, const QString &key)
    {
        for (const QString &l : out.split(QLatin1Char('\n')))
            if (l.startsWith(key + QLatin1Char('=')))
                return l.mid(key.size() + 1).trimmed();
        return {};
    }

    void startCore(const QProcessEnvironment &env = QProcessEnvironment::systemEnvironment())
    {
        core.setProcessEnvironment(env);
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
        QCOMPARE(client.protocol(), QStringLiteral("1.2"));
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

    // AppCatalog shares the CoreClient's connection, loads the catalog on connect,
    // matches window app_ids, reports launch results and exits, and keeps the last
    // catalog (marked stale) while the core is down.
    void appCatalog()
    {
        QTemporaryDir data;
        QVERIFY(data.isValid());
        QDir(data.path()).mkpath(QStringLiteral("apps/applications"));
        auto write = [&](const QString &name, const QByteArray &body) {
            QFile f(data.filePath(QStringLiteral("apps/applications/") + name));
            QVERIFY(f.open(QIODevice::WriteOnly));
            f.write(body);
        };
        write(QStringLiteral("t.ok.desktop"),
              "[Desktop Entry]\nType=Application\nName=Okay\nKeywords=calc;numbers;\n"
              "StartupWMClass=OkWindow\nExec=true\n");
        write(QStringLiteral("t.bad.desktop"),
              "[Desktop Entry]\nType=Application\nName=Broken\nExec=sh -c \"exit 7\"\n");
        auto env = QProcessEnvironment::systemEnvironment();
        env.insert(QStringLiteral("XDG_DATA_HOME"), data.filePath(QStringLiteral("home")));
        env.insert(QStringLiteral("XDG_DATA_DIRS"), data.filePath(QStringLiteral("apps")));

        CoreClient client;
        AppCatalog apps;
        apps.setCore(&client);
        startCore(env);
        QTRY_COMPARE(client.status(), CoreClient::Connected);
        QTRY_VERIFY(apps.loaded());
        QVERIFY(!apps.stale());
        QCOMPARE(apps.rowCount(), 2);
        QCOMPARE(apps.idAt(0), QStringLiteral("t.bad")); // sorted by name: Broken, Okay
        QCOMPARE(apps.get(QStringLiteral("t.ok")).value(QStringLiteral("name")).toString(), QStringLiteral("Okay"));
        QCOMPARE(apps.idForWindow(QStringLiteral("okwindow")), QStringLiteral("t.ok"));
        QCOMPARE(apps.idForWindow(QStringLiteral("T.OK")), QStringLiteral("t.ok"));
        QVERIFY(apps.idForWindow(QStringLiteral("other")).isEmpty());

        AppFilter filter;
        filter.setSourceModel(&apps);
        filter.setQuery(QStringLiteral("numb"));  // keyword match
        QCOMPARE(filter.count(), 1);
        filter.setQuery(QStringLiteral("BRO"));   // case-insensitive name match
        QCOMPARE(filter.count(), 1);
        filter.setQuery(QString());
        QCOMPARE(filter.count(), 2);

        QSignalSpy launched(&apps, &AppCatalog::launched);
        QSignalSpy failed(&apps, &AppCatalog::launchFailed);
        QSignalSpy exited(&apps, &AppCatalog::exited);
        apps.launch(QStringLiteral("t.bad"));
        QTRY_COMPARE(launched.count(), 1);
        QTRY_COMPARE(exited.count(), 1);
        auto ex = exited.takeFirst();
        QCOMPARE(ex.at(0).toString(), QStringLiteral("t.bad"));
        QCOMPARE(ex.at(2).toBool(), false);
        QCOMPARE(ex.at(3).toString(), QStringLiteral("status=7"));
        apps.launch(QStringLiteral("missing"));
        QTRY_COMPARE(failed.count(), 1);
        QVERIFY(failed.takeFirst().at(1).toString().contains(QStringLiteral("missing")));

        // Core gone: catalog kept but stale; launching fails visibly without a call.
        core.terminate();
        QVERIFY(core.waitForFinished());
        QTRY_COMPARE(client.status(), CoreClient::Disconnected);
        QVERIFY(apps.stale());
        QCOMPARE(apps.rowCount(), 2);
        apps.launch(QStringLiteral("t.ok"));
        QCOMPARE(failed.count(), 1);

        // Core back with a changed catalog: reloaded on reconnect.
        write(QStringLiteral("t.new.desktop"), "[Desktop Entry]\nType=Application\nName=New\nExec=true\n");
        startCore(env);
        QTRY_COMPARE(client.status(), CoreClient::Connected);
        QTRY_COMPARE(apps.rowCount(), 3);
        QVERIFY(!apps.stale());
        core.terminate();
        QVERIFY(core.waitForFinished());
    }

    // DockClient end to end: DockClient -> pocketd -> TCP/Noise -> the real desktop agent
    // (POCKET_AGENT_BIN, cross-built) -> a fake notification service on this bus.
    void dockClient()
    {
        const QString agentBin = qEnvironmentVariable("POCKET_AGENT_BIN");
        const QString coreBin = qEnvironmentVariable("POCKETD_BIN", QStringLiteral("pocketd"));
        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        auto env = QProcessEnvironment::systemEnvironment();
        env.insert(QStringLiteral("POCKET_DOCK_DIR"), dir.filePath(QStringLiteral("phone")));
        env.insert(QStringLiteral("POCKET_AGENT_DIR"), dir.filePath(QStringLiteral("agent")));

        // Unconfigured: Dock is served, disconnected, and requests fail visibly.
        CoreClient client;
        DockClient dock;
        dock.setCore(&client);
        startCore(env);
        QTRY_VERIFY(dock.supported());
        QCOMPARE(dock.state(), QStringLiteral("disconnected"));
        QVERIFY(!dock.configured());
        QSignalSpy failed(&dock, &DockClient::requestFailed);
        QSignalSpy finished(&dock, &DockClient::actionFinished);
        dock.connectHost();
        QTRY_COMPARE(failed.count(), 1);
        QVERIFY(failed.takeFirst().at(1).toString().startsWith(QStringLiteral("NotConfigured")));
        dock.execute(QStringLiteral("desktop.notify"));
        QTRY_COMPARE(finished.count(), 1);
        auto f = finished.takeFirst();
        QCOMPARE(f.at(0).toULongLong(), 0ull);
        QCOMPARE(f.at(2).toBool(), false);
        QCOMPARE(f.at(3).toString(), QStringLiteral("NotConnected"));
        core.terminate();
        QVERIFY(core.waitForFinished());
        QTRY_VERIFY(!dock.supported());

        if (agentBin.isEmpty())
            QSKIP("POCKET_AGENT_BIN not set: agent part skipped");

        // Pair through the real CLIs, exactly as scripts/dock-pair.sh does.
        const QString listen = QStringLiteral("127.0.0.1:47199");
        const QString phoneKey = field(run(coreBin, {"dock", "init"}, env), QStringLiteral("public_key"));
        const QString agentKey = field(run(agentBin, {"init", "--listen", listen, "--name", "test-desk"}, env),
                                       QStringLiteral("public_key"));
        QCOMPARE(phoneKey.size(), 64);
        QCOMPARE(agentKey.size(), 64);
        run(agentBin, {"trust", "test-phone", phoneKey}, env);
        run(coreBin, {"dock", "set-host", "desk", listen, agentKey}, env);

        FakeNotifications notifications;
        QVERIFY(QDBusConnection::sessionBus().registerObject(QStringLiteral("/org/freedesktop/Notifications"),
                                                             &notifications, QDBusConnection::ExportAllSlots));
        QVERIFY(QDBusConnection::sessionBus().registerService(QStringLiteral("org.freedesktop.Notifications")));

        QProcess agent;
        agent.setProcessEnvironment(env);
        agent.setProcessChannelMode(QProcess::ForwardedChannels);
        agent.start(agentBin, {"run"});
        QVERIFY(agent.waitForStarted());

        startCore(env);
        QTRY_VERIFY(dock.supported());
        QVERIFY(dock.configured());
        QCOMPARE(dock.hostName(), QStringLiteral("desk"));
        dock.connectHost();
        QTRY_COMPARE_WITH_TIMEOUT(dock.state(), QStringLiteral("connected"), 10000);
        QCOMPARE(dock.hostName(), QStringLiteral("test-desk"));
        QTRY_VERIFY(!dock.actions().isEmpty());
        QCOMPARE(dock.actions().first().toMap().value(QStringLiteral("id")).toString(), QStringLiteral("desktop.notify"));

        // Accepted (pending) first, then confirmed by the computer.
        dock.execute(QStringLiteral("desktop.notify"), {{QStringLiteral("title"), QStringLiteral("bridge test")}});
        QTRY_COMPARE(finished.count(), 1);
        f = finished.takeFirst();
        QVERIFY(f.at(0).toULongLong() > 0);
        QCOMPARE(f.at(2).toBool(), true);
        QVERIFY(f.at(5).toString().contains(QStringLiteral("\"notification_id\":7")));
        QCOMPARE(notifications.summaries, QStringList{QStringLiteral("bridge test")});
        QVERIFY(dock.pendingActions().isEmpty());

        // Unknown action: rejected by Core, never reaches the computer.
        dock.execute(QStringLiteral("no.such.action"));
        QTRY_COMPARE(finished.count(), 1);
        QCOMPARE(finished.takeFirst().at(3).toString(), QStringLiteral("UnknownAction"));

        // Agent dies: the state follows; it comes back by itself.
        agent.kill();
        QVERIFY(agent.waitForFinished());
        QTRY_COMPARE_WITH_TIMEOUT(dock.state(), QStringLiteral("error"), 5000);
        QCOMPARE(dock.errorCode(), QStringLiteral("connection_closed"));
        QVERIFY(dock.actions().isEmpty());
        agent.start(agentBin, {"run"});
        QVERIFY(agent.waitForStarted());
        QTRY_COMPARE_WITH_TIMEOUT(dock.state(), QStringLiteral("connected"), 10000);

        // Core restart: the client resynchronises (the new core starts disconnected).
        core.terminate();
        QVERIFY(core.waitForFinished());
        QTRY_VERIFY(!dock.supported());
        startCore(env);
        QTRY_VERIFY(dock.supported());
        QCOMPARE(dock.state(), QStringLiteral("disconnected"));
        dock.connectHost();
        QTRY_COMPARE_WITH_TIMEOUT(dock.state(), QStringLiteral("connected"), 10000);
        dock.disconnectHost();
        QTRY_COMPARE(dock.state(), QStringLiteral("disconnected"));

        core.terminate();
        QVERIFY(core.waitForFinished());
        agent.terminate();
        QVERIFY(agent.waitForFinished());
        QCOMPARE(agent.exitCode(), 0);
    }
};

QTEST_GUILESS_MAIN(TstCoreClient)
#include "tst_coreclient.moc"
