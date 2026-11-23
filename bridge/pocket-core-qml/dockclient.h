// DockClient: the Dock session served by Pocket Core (org.pocketos.Core1.Dock on
// /org/pocketos/Core1/Dock, protocol >= 1.2): link state to the trusted computer, the
// actions it offers, and action dispatch.
//
// Like AppCatalog it has no D-Bus connection of its own: it uses the `core` CoreClient's
// connection and follows its status, re-reading everything when the core (re)connects.
// Nothing is guessed locally:
//  - `state`, `hostName`, ... change only through GetState/StateChanged (revision-checked);
//  - execute() reports "accepted" (pendingActions) when Core accepted the request, and
//    actionFinished(ok=true) only when Core relays the computer's confirmation;
//  - the retry countdown follows Core's deadline: Core reports the time left when it
//    produces the state (retry_in_ms), and this client counts down from the moment it
//    received it with a monotonic QDeadlineTimer. It never computes a backoff itself.
#pragma once

#include <QDeadlineTimer>
#include <QObject>
#include <QPointer>
#include <QSet>
#include <QString>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include "coreclient.h"

class DockClient : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(CoreClient *core READ core WRITE setCore NOTIFY coreChanged)
    // The connected core serves the Dock object (and its state has been read).
    Q_PROPERTY(bool supported READ supported NOTIFY changed)
    // "disconnected" | "connecting" | "connected" | "error" ("" until first read)
    Q_PROPERTY(QString state READ state NOTIFY changed)
    Q_PROPERTY(bool configured READ configured NOTIFY changed)
    Q_PROPERTY(QString hostName READ hostName NOTIFY changed)
    Q_PROPERTY(QString hostId READ hostId NOTIFY changed)
    Q_PROPERTY(QString address READ address NOTIFY changed)
    Q_PROPERTY(QString agent READ agent NOTIFY changed)
    Q_PROPERTY(QString errorCode READ errorCode NOTIFY changed)
    Q_PROPERTY(QString errorText READ errorText NOTIFY changed)
    // Seconds until the next automatic attempt as reported in the last state (static).
    Q_PROPERTY(int retryIn READ retryIn NOTIFY changed)
    // An automatic attempt is scheduled (state "error" with a deadline).
    Q_PROPERTY(bool retryScheduled READ retryScheduled NOTIFY changed)
    // Live countdown to that attempt, in whole seconds (rounded up, never negative). It
    // updates once per second only while countdownActive is true (the UI showing it).
    Q_PROPERTY(int retrySeconds READ retrySeconds NOTIFY retrySecondsChanged)
    Q_PROPERTY(bool countdownActive READ countdownActive WRITE setCountdownActive NOTIFY countdownActiveChanged)
    // The user wants the link (Core's connection intent, protocol >= 1.3).
    Q_PROPERTY(bool connectionWanted READ connectionWanted NOTIFY changed)
    // [{id, label, category, available, reason}], in the agent's order.
    Q_PROPERTY(QVariantList actions READ actions NOTIFY actionsChanged)
    // action id -> request id, while Core has accepted a request and the computer has
    // not answered yet.
    Q_PROPERTY(QVariantMap pendingActions READ pendingActions NOTIFY pendingChanged)

public:
    explicit DockClient(QObject *parent = nullptr);

    CoreClient *core() const { return m_core; }
    void setCore(CoreClient *core);
    bool supported() const { return m_supported; }
    QString state() const { return m_state.value(QStringLiteral("state")).toString(); }
    bool configured() const { return m_state.value(QStringLiteral("configured")).toBool(); }
    QString hostName() const { return m_state.value(QStringLiteral("host_name")).toString(); }
    QString hostId() const { return m_state.value(QStringLiteral("host_id")).toString(); }
    QString address() const { return m_state.value(QStringLiteral("address")).toString(); }
    QString agent() const { return m_state.value(QStringLiteral("agent")).toString(); }
    QString errorCode() const { return m_state.value(QStringLiteral("error_code")).toString(); }
    QString errorText() const { return m_state.value(QStringLiteral("error")).toString(); }
    int retryIn() const { return m_state.value(QStringLiteral("retry_in")).toInt(); }
    bool retryScheduled() const { return m_supported && !m_retryDeadline.isForever(); }
    int retrySeconds() const { return m_retrySeconds; }
    bool countdownActive() const { return m_countdownActive; }
    void setCountdownActive(bool active);
    bool connectionWanted() const { return m_state.value(QStringLiteral("connection_wanted")).toBool(); }
    // Milliseconds left until the scheduled attempt (0 when none or due).
    Q_INVOKABLE int retryRemainingMs() const;
    QVariantList actions() const { return m_actions; }
    QVariantMap pendingActions() const { return m_pending; }

    // Results: requestFailed(op, error) when Core rejects the call; state via changed().
    Q_INVOKABLE void connectHost();
    Q_INVOKABLE void disconnectHost();
    // Result: actionFinished(...) exactly once per call (request 0 if Core rejected it).
    Q_INVOKABLE void execute(const QString &actionId, const QVariantMap &params = {});

signals:
    void coreChanged();
    void changed();
    void actionsChanged();
    void pendingChanged();
    void retrySecondsChanged();
    void countdownActiveChanged();
    void requestFailed(const QString &op, const QString &error);
    void actionFinished(qulonglong request, const QString &action, bool ok, const QString &code,
                        const QString &message, const QString &data);

private slots:
    void onStateChanged(const QVariantMap &state);
    void onActionsChanged();
    void onActionFinished(qulonglong request, const QString &action, bool ok, const QString &code,
                          const QString &message, const QString &data);

private:
    void coreStatusChanged();
    void subscribe();
    void fetch();
    void fetchActions();
    bool applyState(const QVariantMap &state, bool initial);
    void call(const QString &method, const QString &op);
    void clearPending(const QString &code, const QString &message);
    void updateCountdown();

    QPointer<CoreClient> m_core;
    QString m_subscribedBus;
    bool m_supported = false;
    int m_generation = 0;
    quint64 m_revision = 0;
    QVariantMap m_state;
    QVariantList m_actions;
    QVariantMap m_pending;             // action -> request
    QSet<qulonglong> m_finishedEarly;  // ActionFinished seen before the Execute reply
    QDeadlineTimer m_retryDeadline{QDeadlineTimer::Forever}; // forever = none scheduled
    int m_retrySeconds = 0;
    bool m_countdownActive = false;
    QTimer m_tick; // single shot, armed for the next whole second while counting down
};
