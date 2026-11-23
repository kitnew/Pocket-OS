#include "dockclient.h"

#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLoggingCategory>

Q_LOGGING_CATEGORY(lcDock, "pocket.core.dock", QtInfoMsg)

namespace {
// Contract: pocket_ipc::dbus::dock (Rust) / docs/decisions/0007-dock-architecture.md.
const QString kService = QStringLiteral("org.pocketos.Core1");
const QString kPath = QStringLiteral("/org/pocketos/Core1/Dock");
const QString kInterface = QStringLiteral("org.pocketos.Core1.Dock");
const int kMinProtocolMinor = 2;
const int kCallTimeoutMs = 5000;

QString dbusErrorText(const QDBusError &e)
{
    return e.name().section(QLatin1Char('.'), -1) + QStringLiteral(": ") + e.message();
}
} // namespace

DockClient::DockClient(QObject *parent) : QObject(parent)
{
    qDBusRegisterMetaType<QList<QVariantMap>>();
    m_tick.setSingleShot(true);
    m_tick.setTimerType(Qt::PreciseTimer);
    connect(&m_tick, &QTimer::timeout, this, &DockClient::updateCountdown);
}

int DockClient::retryRemainingMs() const
{
    if (!retryScheduled())
        return 0;
    return int(qMax<qint64>(0, m_retryDeadline.remainingTime()));
}

void DockClient::setCountdownActive(bool active)
{
    if (active == m_countdownActive)
        return;
    m_countdownActive = active;
    emit countdownActiveChanged();
    updateCountdown();
}

// Recomputes the whole seconds left and, while someone is watching, wakes up again at the
// next whole-second boundary of the deadline (one wakeup per second, none otherwise).
void DockClient::updateCountdown()
{
    const int ms = retryRemainingMs();
    const int secs = (ms + 999) / 1000;
    if (secs != m_retrySeconds) {
        m_retrySeconds = secs;
        emit retrySecondsChanged();
    }
    if (m_countdownActive && ms > 0)
        m_tick.start(ms % 1000 == 0 ? 1000 : ms % 1000 + 1);
    else
        m_tick.stop();
}

void DockClient::setCore(CoreClient *core)
{
    if (core == m_core)
        return;
    if (m_core)
        disconnect(m_core, nullptr, this, nullptr);
    m_core = core;
    m_subscribedBus.clear();
    if (m_core) {
        connect(m_core, &CoreClient::statusChanged, this, &DockClient::coreStatusChanged);
        connect(m_core, &CoreClient::stateUpdated, this, &DockClient::coreStatusChanged);
        connect(m_core, &CoreClient::busChanged, this, [this] { m_subscribedBus.clear(); });
        coreStatusChanged();
    }
    emit coreChanged();
}

void DockClient::coreStatusChanged()
{
    const bool connected = m_core && m_core->status() == CoreClient::Connected;
    const int minor = connected ? m_core->protocol().section(QLatin1Char('.'), 1, 1).toInt() : 0;
    if (connected && minor >= kMinProtocolMinor) {
        // A new core owner restarts revisions; the core client resets on owner change and
        // reports Connected again, so re-read whenever we were not supported yet.
        if (!m_supported) {
            subscribe();
            fetch();
        }
        return;
    }
    if (connected)
        qCWarning(lcDock).noquote() << "core protocol" << m_core->protocol() << "has no Dock object";
    if (m_supported) {
        m_supported = false;
        ++m_generation;
        m_revision = 0;
        // The core is gone: whatever it had in flight will never be reported.
        clearPending(QStringLiteral("core_lost"), QStringLiteral("Pocket Core stopped before the computer answered"));
        if (!m_actions.isEmpty()) {
            m_actions.clear();
            emit actionsChanged();
        }
        m_retryDeadline = QDeadlineTimer(QDeadlineTimer::Forever);
        emit changed();
        updateCountdown();
    }
}

void DockClient::subscribe()
{
    QDBusConnection bus = m_core->bus();
    if (m_subscribedBus == bus.name())
        return;
    m_subscribedBus = bus.name();
    // Signals follow the well-known name, so subscriptions survive core restarts.
    bus.connect(kService, kPath, kInterface, QStringLiteral("StateChanged"), this,
                SLOT(onStateChanged(QVariantMap)));
    bus.connect(kService, kPath, kInterface, QStringLiteral("ActionsChanged"), this,
                SLOT(onActionsChanged()));
    bus.connect(kService, kPath, kInterface, QStringLiteral("ActionFinished"), this,
                SLOT(onActionFinished(qulonglong, QString, bool, QString, QString, QString)));
}

void DockClient::fetch()
{
    const int gen = ++m_generation;
    m_revision = 0;
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("GetState"));
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, gen](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        if (gen != m_generation)
            return;
        QDBusPendingReply<QVariantMap> reply = *c;
        if (reply.isError()) {
            qCWarning(lcDock).noquote() << "GetState failed:" << dbusErrorText(reply.error());
            return; // retried on the next core status change
        }
        if (applyState(reply.value(), true)) {
            m_supported = true;
            emit changed();
            updateCountdown();
            fetchActions();
        }
    });
}

void DockClient::fetchActions()
{
    if (!m_core)
        return;
    const int gen = m_generation;
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("ListActions"));
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, gen](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        if (gen != m_generation)
            return;
        QDBusPendingReply<QList<QVariantMap>> reply = *c;
        if (reply.isError()) {
            qCWarning(lcDock).noquote() << "ListActions failed:" << dbusErrorText(reply.error());
            return;
        }
        QVariantList list;
        for (const QVariantMap &a : reply.value())
            list.append(a);
        m_actions = list;
        qCInfo(lcDock) << "actions:" << m_actions.size();
        emit actionsChanged();
    });
}

bool DockClient::applyState(const QVariantMap &state, bool initial)
{
    const quint64 rev = state.value(QStringLiteral("revision")).toULongLong();
    if (initial ? rev < m_revision : rev <= m_revision)
        return false;
    m_revision = rev;
    const QString before = this->state();
    m_state = state;
    // The time left was computed by Core when it produced this state; count from now.
    // (Cores older than 1.3 only send whole seconds.)
    qint64 ms = state.contains(QStringLiteral("retry_in_ms"))
        ? state.value(QStringLiteral("retry_in_ms")).toLongLong()
        : state.value(QStringLiteral("retry_in")).toLongLong() * 1000;
    if (this->state() != QLatin1String("error"))
        ms = 0;
    m_retryDeadline = ms > 0 ? QDeadlineTimer(ms) : QDeadlineTimer(QDeadlineTimer::Forever);
    if (before != this->state())
        qCInfo(lcDock).noquote() << "state" << this->state() << "host=" + hostName()
                                 << (errorCode().isEmpty() ? QString() : "error=" + errorCode());
    return true;
}

void DockClient::onStateChanged(const QVariantMap &state)
{
    if (m_supported && applyState(state, false)) {
        emit changed();
        updateCountdown();
    }
}

void DockClient::onActionsChanged()
{
    if (m_supported)
        fetchActions();
}

void DockClient::onActionFinished(qulonglong request, const QString &action, bool ok,
                                  const QString &code, const QString &message, const QString &data)
{
    if (m_pending.value(action).toULongLong() == request) {
        m_pending.remove(action);
        emit pendingChanged();
    } else {
        // Finished before the Execute reply reached us (or another client's request).
        m_finishedEarly.insert(request);
        if (m_finishedEarly.size() > 64)
            m_finishedEarly.clear();
    }
    qCInfo(lcDock).noquote() << "finished" << request << action << (ok ? "ok" : "failed " + code);
    emit actionFinished(request, action, ok, code, message, data);
}

void DockClient::call(const QString &method, const QString &op)
{
    if (!m_core || !m_supported) {
        emit requestFailed(op, QStringLiteral("Pocket Core is not connected"));
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, method);
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, op](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<> reply = *c;
        if (reply.isError()) {
            qCWarning(lcDock).noquote() << op << "failed:" << dbusErrorText(reply.error());
            emit requestFailed(op, dbusErrorText(reply.error()));
        }
    });
}

void DockClient::connectHost() { call(QStringLiteral("Connect"), QStringLiteral("connect")); }
void DockClient::disconnectHost() { call(QStringLiteral("Disconnect"), QStringLiteral("disconnect")); }

void DockClient::execute(const QString &actionId, const QVariantMap &params)
{
    if (!m_core || !m_supported) {
        emit actionFinished(0, actionId, false, QStringLiteral("NotConnected"),
                            QStringLiteral("Pocket Core is not connected"), QString());
        return;
    }
    if (m_pending.contains(actionId)) {
        emit actionFinished(0, actionId, false, QStringLiteral("Busy"),
                            QStringLiteral("already waiting for the computer"), QString());
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("Execute"));
    msg << actionId << params;
    const int gen = m_generation;
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, actionId, gen](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<qulonglong> reply = *c;
        if (reply.isError()) {
            const QDBusError e = reply.error();
            qCWarning(lcDock).noquote() << "Execute" << actionId << "rejected:" << dbusErrorText(e);
            emit actionFinished(0, actionId, false, e.name().section(QLatin1Char('.'), -1), e.message(), QString());
            return;
        }
        const qulonglong request = reply.value();
        if (m_finishedEarly.remove(request) || gen != m_generation)
            return;
        qCInfo(lcDock).noquote() << "accepted" << request << actionId;
        m_pending.insert(actionId, request);
        emit pendingChanged();
    });
}

void DockClient::clearPending(const QString &code, const QString &message)
{
    if (m_pending.isEmpty())
        return;
    const QVariantMap pending = m_pending;
    m_pending.clear();
    emit pendingChanged();
    for (auto it = pending.cbegin(); it != pending.cend(); ++it)
        emit actionFinished(it.value().toULongLong(), it.key(), false, code, message, QString());
}
