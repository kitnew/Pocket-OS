#include "coreclient.h"

#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>
#include <QLoggingCategory>

Q_LOGGING_CATEGORY(lcCore, "pocket.core.client", QtInfoMsg)

namespace {
// Contract: crates/pocket-ipc (Rust) and docs/decisions/0002-pocket-core-and-ipc.md.
const QString kService = QStringLiteral("org.pocketos.Core1");
const QString kPath = QStringLiteral("/org/pocketos/Core1");
const QString kInterface = QStringLiteral("org.pocketos.Core1");
const int kProtocolMajor = 1;
const int kCallTimeoutMs = 5000;
const int kMaxRetryDelayMs = 30000;
} // namespace

CoreClient::CoreClient(QObject *parent)
    : QObject(parent), m_bus(QString())
{
    m_retry.setSingleShot(true);
    connect(&m_retry, &QTimer::timeout, this, [this] {
        if (!m_bus.isConnected())
            connectBus();
        else
            fetchState();
    });
    connectBus();
}

CoreClient::~CoreClient()
{
    if (!m_busName.isEmpty())
        QDBusConnection::disconnectFromBus(m_busName);
}

QString CoreClient::statusText() const
{
    switch (m_status) {
    case Connecting: return QStringLiteral("Connecting");
    case Connected: return QStringLiteral("Connected");
    case Disconnected: return QStringLiteral("Disconnected");
    }
    return {};
}

void CoreClient::connectBus()
{
    // A private named connection (not the shared default) so a failed connection can be
    // dropped and retried.
    if (!m_busName.isEmpty())
        QDBusConnection::disconnectFromBus(m_busName);
    m_busName = QStringLiteral("pocket-core-client-%1").arg(++m_busAttempt);
    m_bus = QDBusConnection::connectToBus(QDBusConnection::SessionBus, m_busName);
    emit busChanged();
    if (!m_bus.isConnected()) {
        setError(QStringLiteral("session bus unavailable: ") + m_bus.lastError().message());
        setStatus(Disconnected);
        scheduleRetry();
        return;
    }

    delete m_watcher;
    m_watcher = new QDBusServiceWatcher(kService, m_bus,
                                        QDBusServiceWatcher::WatchForOwnerChange, this);
    connect(m_watcher, &QDBusServiceWatcher::serviceOwnerChanged, this,
            &CoreClient::onOwnerChanged);

    // Subscribe before reading the state so no change can be missed. QtDBus follows the
    // owner of the well-known name, so this survives core restarts.
    m_bus.connect(kService, kPath, kInterface, QStringLiteral("StateChanged"), this,
                  SLOT(onStateChanged(QVariantMap)));

    setStatus(Connecting);
    fetchState();
}

void CoreClient::fetchState()
{
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface,
                                              QStringLiteral("GetState"));
    auto *call = new QDBusPendingCallWatcher(m_bus.asyncCall(msg, kCallTimeoutMs), this);
    call->setProperty("generation", m_generation);
    connect(call, &QDBusPendingCallWatcher::finished, this, &CoreClient::onGetStateFinished);
}

void CoreClient::onGetStateFinished(QDBusPendingCallWatcher *call)
{
    call->deleteLater();
    if (call->property("generation").toInt() != m_generation) {
        qCInfo(lcCore, "ignoring GetState reply from a previous core owner");
        return; // the owner changed meanwhile; the new owner's GetState is in flight
    }
    QDBusPendingReply<QVariantMap> reply = *call;
    if (reply.isError()) {
        const QDBusError err = reply.error();
        setError(err.name() + QStringLiteral(": ") + err.message());
        setStatus(Disconnected);
        // Core not running: wait for the owner-change notification instead of polling.
        const bool absent = err.type() == QDBusError::ServiceUnknown
            || err.name() == QLatin1String("org.freedesktop.DBus.Error.NameHasNoOwner");
        if (!absent)
            scheduleRetry();
        else
            qCInfo(lcCore, "core not on the bus; waiting for it to appear");
        return;
    }
    if (m_owner.isEmpty()) // first connection: the watcher only reports later changes
        m_owner = m_bus.interface()->serviceOwner(kService).value();
    if (applyState(reply.value(), true)) {
        m_retryDelayMs = 1000;
        if (m_status != Connected)
            ++m_connections;
        setError(QString());
        setStatus(Connected);
    }
}

void CoreClient::onOwnerChanged(const QString &, const QString &oldOwner, const QString &newOwner)
{
    qCInfo(lcCore) << "core owner changed" << (oldOwner.isEmpty() ? "-" : oldOwner) << "->"
                   << (newOwner.isEmpty() ? "-" : newOwner);
    m_retry.stop();
    ++m_generation;
    m_revision = 0; // revisions restart with a new core process
    m_owner = newOwner;
    if (newOwner.isEmpty()) {
        m_ready = false;
        emit stateUpdated();
        setStatus(Disconnected);
        return;
    }
    setStatus(Connecting);
    fetchState();
}

void CoreClient::onStateChanged(const QVariantMap &state)
{
    if (m_status == Connected)
        applyState(state, false);
    // While connecting, the pending GetState delivers the same or newer state.
}

bool CoreClient::applyState(const QVariantMap &state, bool initial)
{
    const QString protocol = state.value(QStringLiteral("protocol")).toString();
    if (protocol.section(QLatin1Char('.'), 0, 0).toInt() != kProtocolMajor) {
        setError(QStringLiteral("incompatible core protocol ") + protocol);
        setStatus(Disconnected);
        return false;
    }
    const quint64 rev = state.value(QStringLiteral("revision")).toULongLong();
    if (initial ? rev < m_revision : rev <= m_revision) {
        qCInfo(lcCore) << "dropping stale state revision" << rev << "<=" << m_revision;
        return false;
    }
    m_revision = rev;
    m_protocol = protocol;
    m_version = state.value(QStringLiteral("version")).toString();
    m_ready = state.value(QStringLiteral("ready")).toBool();
    const QString mode = state.value(QStringLiteral("mode")).toString();
    const bool modeDiffers = mode != m_mode;
    m_mode = mode;
    qCInfo(lcCore).noquote() << "state" << (initial ? "read" : "changed") << "mode=" + m_mode
                             << "revision=" + QString::number(m_revision);
    emit stateUpdated();
    if (modeDiffers)
        emit modeChanged();
    return true;
}

void CoreClient::requestMode(const QString &mode)
{
    if (m_status != Connected) {
        const QString e = QStringLiteral("core not connected");
        setError(e);
        emit requestFinished(mode, false, e);
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface,
                                              QStringLiteral("RequestMode"));
    msg << mode;
    ++m_pendingRequests;
    emit requestPendingChanged();
    auto *call = new QDBusPendingCallWatcher(m_bus.asyncCall(msg, kCallTimeoutMs), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, mode](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        --m_pendingRequests;
        emit requestPendingChanged();
        QDBusPendingReply<> reply = *c;
        if (reply.isError()) {
            const QString e = reply.error().name() + QStringLiteral(": ") + reply.error().message();
            qCWarning(lcCore).noquote() << "RequestMode" << mode << "failed:" << e;
            setError(e);
            emit requestFinished(mode, false, e);
        } else {
            emit requestFinished(mode, true, QString());
        }
    });
}

void CoreClient::scheduleRetry()
{
    qCInfo(lcCore) << "retrying in" << m_retryDelayMs << "ms";
    m_retry.start(m_retryDelayMs);
    m_retryDelayMs = qMin(m_retryDelayMs * 2, kMaxRetryDelayMs);
}

void CoreClient::setStatus(Status s)
{
    if (s == m_status)
        return;
    m_status = s;
    qCInfo(lcCore).noquote() << "status" << statusText() << "owner=" + m_owner;
    emit statusChanged();
}

void CoreClient::setError(const QString &e)
{
    if (e == m_lastError)
        return;
    m_lastError = e;
    emit lastErrorChanged();
}
