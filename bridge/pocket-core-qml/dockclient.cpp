#include "dockclient.h"

#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QJsonDocument>
#include <QJsonArray>
#include <QJsonObject>
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

QVariantMap parseObject(const QString &json)
{
    const QJsonDocument doc = QJsonDocument::fromJson(json.toUtf8());
    return doc.isObject() ? doc.object().toVariantMap() : QVariantMap();
}
} // namespace

DockClient::DockClient(QObject *parent) : QObject(parent)
{
    qDBusRegisterMetaType<QList<QVariantMap>>();
    m_tick.setSingleShot(true);
    m_tick.setTimerType(Qt::PreciseTimer);
    connect(&m_tick, &QTimer::timeout, this, &DockClient::updateCountdown);
    m_staleTimer.setSingleShot(true);
    connect(&m_staleTimer, &QTimer::timeout, this, &DockClient::checkStale);
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
        m_minor = minor;
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
        // Remote state is never shown once its source is gone.
        if (!m_live.isEmpty()) {
            m_live.clear();
            m_staleAt.clear();
            m_staleTimer.stop();
            emit liveChanged();
        }
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
    // 1.4 (connecting to signals an older core lacks is harmless: they never arrive).
    bus.connect(kService, kPath, kInterface, QStringLiteral("DeckChanged"), this,
                SLOT(onDeckChanged()));
    bus.connect(kService, kPath, kInterface, QStringLiteral("LiveChanged"), this,
                SLOT(onLiveChanged(QString, QString)));
    bus.connect(kService, kPath, kInterface, QStringLiteral("ActionProgress"), this,
                SLOT(onActionProgress(qulonglong, uint, uint, QString)));
    bus.connect(kService, kPath, kInterface, QStringLiteral("Notification"), this,
                SLOT(onNotification(QString)));
    bus.connect(kService, kPath, kInterface, QStringLiteral("NotificationClosed"), this,
                SLOT(onNotificationClosed(QString)));
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
            fetchPlatform();
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
        for (QVariantMap a : reply.value()) {
            // Core 1.5: parameters (with the laptop's closed `choices` lists) as JSON.
            const QJsonDocument doc = QJsonDocument::fromJson(a.value(QStringLiteral("params")).toString().toUtf8());
            a.insert(QStringLiteral("params"), doc.isArray() ? doc.array().toVariantList() : QVariantList());
            list.append(a);
        }
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
    sendExecute(QStringLiteral("Execute"), {actionId, params}, actionId);
}

// Execute / Retry: the reply is the accepted request id; the outcome is ActionFinished.
void DockClient::sendExecute(const QString &method, const QList<QVariant> &args, const QString &actionId)
{
    if (m_pending.contains(actionId)) {
        emit actionFinished(0, actionId, false, QStringLiteral("Busy"),
                            QStringLiteral("already waiting for the computer"), QString());
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, method);
    msg.setArguments(args);
    const int gen = m_generation;
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, actionId, gen, method](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<qulonglong> reply = *c;
        if (reply.isError()) {
            const QDBusError e = reply.error();
            qCWarning(lcDock).noquote() << method << actionId << "rejected:" << dbusErrorText(e);
            emit actionFinished(0, actionId, false, e.name().section(QLatin1Char('.'), -1), e.message(), QString());
            return;
        }
        const qulonglong request = reply.value();
        if (m_finishedEarly.remove(request) || gen != m_generation)
            return;
        qCInfo(lcDock).noquote() << "accepted" << request << actionId << (method == QLatin1String("Retry") ? "(retry)" : "");
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

// ── Core Dock 1.4 ──────────────────────────────────────────────────────────────────────

// Reads deck, live state and preferences (after every (re)connection of the core).
void DockClient::fetchPlatform()
{
    if (!m_core || m_minor < 4)
        return;
    const int gen = m_generation;
    auto get = [this, gen](const QString &method, auto apply) {
        auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, method);
        auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
        connect(w, &QDBusPendingCallWatcher::finished, this, [this, gen, method, apply](QDBusPendingCallWatcher *c) {
            c->deleteLater();
            if (gen != m_generation)
                return;
            QDBusMessage reply = c->reply();
            if (reply.type() == QDBusMessage::ErrorMessage) {
                qCWarning(lcDock).noquote() << method << "failed:" << reply.errorMessage();
                return;
            }
            apply(reply.arguments().value(0));
        });
    };
    get(QStringLiteral("GetDeck"), [this](const QVariant &v) {
        m_deck = parseObject(v.toString());
        qCInfo(lcDock).noquote() << "deck:" << m_deck.value(QStringLiteral("source")).toString()
                                 << m_deck.value(QStringLiteral("pages")).toList().size() << "pages";
        emit deckChanged();
    });
    get(QStringLiteral("GetLive"), [this](const QVariant &v) {
        const QVariantMap all = parseObject(v.toString());
        m_live.clear();
        m_staleAt.clear();
        for (auto it = all.cbegin(); it != all.cend(); ++it)
            setTopic(it.key(), it.value().toMap());
        emit liveChanged();
        checkStale();
    });
    get(QStringLiteral("GetPreferences"), [this](const QVariant &v) {
        m_prefs = qdbus_cast<QVariantMap>(v);
        emit preferencesChanged();
    });
}

void DockClient::onDeckChanged()
{
    if (m_supported)
        fetchPlatform();
}

// Records one topic value; a periodic topic (interval_ms > 0) becomes stale after 3 x its
// interval without an update (age_ms: how old Core says it already is).
void DockClient::setTopic(const QString &topic, const QVariantMap &value)
{
    QVariantMap v = value;
    v.insert(QStringLiteral("stale"), false);
    m_live.insert(topic, v);
    const qint64 interval = v.value(QStringLiteral("interval_ms")).toLongLong();
    if (interval > 0) {
        const qint64 age = v.value(QStringLiteral("age_ms")).toLongLong();
        m_staleAt.insert(topic, QDeadlineTimer(qMax<qint64>(0, 3 * interval - age)));
    } else {
        m_staleAt.remove(topic);
    }
}

void DockClient::checkStale()
{
    qint64 next = -1;
    bool changedAny = false;
    for (auto it = m_staleAt.begin(); it != m_staleAt.end();) {
        if (it.value().hasExpired()) {
            QVariantMap v = m_live.value(it.key()).toMap();
            if (!v.isEmpty() && !v.value(QStringLiteral("stale")).toBool()) {
                v.insert(QStringLiteral("stale"), true);
                m_live.insert(it.key(), v);
                changedAny = true;
                qCInfo(lcDock).noquote() << "live topic stale:" << it.key();
            }
            it = m_staleAt.erase(it);
        } else {
            const qint64 left = it.value().remainingTime();
            next = next < 0 ? left : qMin(next, left);
            ++it;
        }
    }
    if (changedAny)
        emit liveChanged();
    if (next >= 0)
        m_staleTimer.start(int(qMin<qint64>(next + 1, 24 * 3600 * 1000)));
    else
        m_staleTimer.stop();
}

void DockClient::onLiveChanged(const QString &topic, const QString &json)
{
    if (!m_supported)
        return;
    if (json.isEmpty()) {
        m_live.remove(topic);
        m_staleAt.remove(topic);
    } else {
        setTopic(topic, parseObject(json));
    }
    emit liveChanged();
    checkStale();
}

void DockClient::onActionProgress(qulonglong request, uint step, uint total, const QString &label)
{
    for (auto it = m_pending.cbegin(); it != m_pending.cend(); ++it) {
        if (it.value().toULongLong() == request) {
            emit actionProgress(request, it.key(), step, total, label);
            return;
        }
    }
}

void DockClient::onNotification(const QString &json)
{
    const QVariantMap n = parseObject(json);
    if (!n.isEmpty())
        emit notification(n);
}

void DockClient::onNotificationClosed(const QString &nid)
{
    emit notificationClosed(nid);
}

void DockClient::reloadDeck() { call(QStringLiteral("ReloadDeck"), QStringLiteral("reload deck")); }

void DockClient::setDeck(const QString &json)
{
    if (!deckEditable()) {
        emit deckSaved(false, QStringLiteral("Pocket Core cannot save the deck (update pocketd)"));
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("SetDeck"));
    msg << json;
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<> reply = *c;
        if (reply.isError()) {
            qCWarning(lcDock).noquote() << "save deck failed:" << dbusErrorText(reply.error());
            emit deckSaved(false, reply.error().message());
            return;
        }
        emit deckSaved(true, QString());
    });
}

void DockClient::resetDeck()
{
    if (!deckEditable()) {
        emit deckSaved(false, QStringLiteral("Pocket Core cannot reset the deck (update pocketd)"));
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("ResetDeck"));
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<> reply = *c;
        emit deckSaved(!reply.isError(), reply.isError() ? reply.error().message() : QString());
    });
}

void DockClient::setPreference(const QString &key, const QVariant &value)
{
    if (!m_core || !m_supported || m_minor < 4) {
        emit requestFailed(QStringLiteral("preference"), QStringLiteral("Pocket Core does not support preferences"));
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("SetPreference"));
    msg << key << QVariant::fromValue(QDBusVariant(value));
    auto *w = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<> reply = *c;
        if (reply.isError()) {
            emit requestFailed(QStringLiteral("preference"), dbusErrorText(reply.error()));
            return;
        }
        fetchPlatform(); // re-read the stored preferences (the source of truth)
    });
}

void DockClient::retry(qulonglong request, const QString &actionId)
{
    if (!m_core || !m_supported || m_minor < 4) {
        emit actionFinished(0, actionId, false, QStringLiteral("NotSupported"),
                            QStringLiteral("Pocket Core cannot retry requests"), QString());
        return;
    }
    sendExecute(QStringLiteral("Retry"), {QVariant::fromValue(request)}, actionId);
}
