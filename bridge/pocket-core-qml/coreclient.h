// CoreClient: QML-facing client of Pocket Core (org.pocketos.Core1 on the session bus).
//
// It mirrors the core state (mode, revision, ...) and tracks the core's lifetime:
//  - the bus-name owner is watched (QDBusServiceWatcher), so a core that starts after the
//    UI, crashes, or restarts is noticed without polling;
//  - on every new owner the full state is re-read (GetState) and the revision baseline is
//    reset, so the UI resynchronises after a core restart;
//  - state changes arrive as StateChanged signals; stale data is dropped by revision.
// QML never assumes a request succeeded: `mode` only changes when the core says so.
#pragma once

#include <QDBusConnection>
#include <QDBusPendingCallWatcher>
#include <QObject>
#include <QString>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

class QDBusServiceWatcher;

class CoreClient : public QObject {
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(Status status READ status NOTIFY statusChanged)
    Q_PROPERTY(QString statusText READ statusText NOTIFY statusChanged)
    Q_PROPERTY(QString mode READ mode NOTIFY modeChanged)
    Q_PROPERTY(QString coreVersion READ coreVersion NOTIFY stateUpdated)
    Q_PROPERTY(QString protocol READ protocol NOTIFY stateUpdated)
    Q_PROPERTY(bool ready READ ready NOTIFY stateUpdated)
    Q_PROPERTY(double revision READ revision NOTIFY stateUpdated)
    Q_PROPERTY(QString owner READ owner NOTIFY statusChanged)
    Q_PROPERTY(int connections READ connections NOTIFY statusChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)
    Q_PROPERTY(bool requestPending READ requestPending NOTIFY requestPendingChanged)

public:
    enum Status { Connecting, Connected, Disconnected };
    Q_ENUM(Status)

    explicit CoreClient(QObject *parent = nullptr);
    ~CoreClient() override;

    Status status() const { return m_status; }
    QString statusText() const;
    QString mode() const { return m_mode; }
    QString coreVersion() const { return m_version; }
    QString protocol() const { return m_protocol; }
    bool ready() const { return m_ready; }
    double revision() const { return double(m_revision); }
    QString owner() const { return m_owner; }
    int connections() const { return m_connections; }
    QString lastError() const { return m_lastError; }
    bool requestPending() const { return m_pendingRequests > 0; }

    // Asks the core to switch mode. Result: requestFinished(); state: modeChanged().
    Q_INVOKABLE void requestMode(const QString &mode);

    // For other clients of the same core (AppCatalog): the connection this client uses.
    // It changes only after a transport error (busChanged()).
    QDBusConnection bus() const { return m_bus; }

signals:
    void statusChanged();
    void modeChanged();
    void stateUpdated();
    void lastErrorChanged();
    void requestPendingChanged();
    void requestFinished(const QString &mode, bool ok, const QString &error);
    void busChanged();

private slots:
    void onOwnerChanged(const QString &name, const QString &oldOwner, const QString &newOwner);
    void onStateChanged(const QVariantMap &state);
    void onGetStateFinished(QDBusPendingCallWatcher *call);

private:
    void connectBus();
    void fetchState();
    void scheduleRetry();
    bool applyState(const QVariantMap &state, bool initial);
    void setStatus(Status s);
    void setError(const QString &e);

    QString m_busName;
    QDBusConnection m_bus;
    QDBusServiceWatcher *m_watcher = nullptr;
    QTimer m_retry;
    int m_retryDelayMs = 1000;
    int m_busAttempt = 0;
    int m_generation = 0; // bumped on every core owner change

    Status m_status = Connecting;
    QString m_owner;
    int m_connections = 0;
    int m_pendingRequests = 0;
    QString m_mode;
    QString m_version;
    QString m_protocol;
    bool m_ready = false;
    quint64 m_revision = 0;
    QString m_lastError;
};
