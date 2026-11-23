#include "orientationsensor.h"

#include "dbusservice.h"

#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLoggingCategory>

Q_LOGGING_CATEGORY(lcSensor, "pocket.platform.sensor", QtInfoMsg)

OrientationSensor::OrientationSensor(QObject *parent)
    : QObject(parent),
      m_proxy(new DBusPropertyMirror(QDBusConnection::systemBus(), QStringLiteral("net.hadess.SensorProxy"),
                                     QStringLiteral("/net/hadess/SensorProxy"),
                                     QStringLiteral("net.hadess.SensorProxy"), this))
{
    connect(m_proxy, &DBusPropertyMirror::presentChanged, this, [this] {
        m_claimed = false; // a restarted proxy has no claims
        sync();
        emit changed();
    });
    connect(m_proxy, &DBusPropertyMirror::propertiesChanged, this, [this](const QStringList &keys) {
        if (keys.contains(QStringLiteral("AccelerometerOrientation")))
            qCInfo(lcSensor).noquote() << "orientation" << orientation();
        emit changed();
    });
}

OrientationSensor::~OrientationSensor()
{
    if (m_claimed)
        QDBusConnection::systemBus().call(
            QDBusMessage::createMethodCall(m_proxy->service(), m_proxy->path(), m_proxy->interface(),
                                           QStringLiteral("ReleaseAccelerometer")),
            QDBus::NoBlock);
}

bool OrientationSensor::available() const
{
    return m_proxy->present() && m_proxy->value(QStringLiteral("HasAccelerometer")).toBool();
}

QString OrientationSensor::orientation() const
{
    const QString o = m_proxy->value(QStringLiteral("AccelerometerOrientation")).toString();
    return m_claimed && !o.isEmpty() ? o : QStringLiteral("undefined");
}

void OrientationSensor::setEnabled(bool e)
{
    if (e == m_enabled)
        return;
    m_enabled = e;
    emit enabledChanged();
    sync();
}

void OrientationSensor::sync()
{
    if (!available() || m_inFlight)
        return;
    if (m_enabled && !m_claimed)
        call(QStringLiteral("ClaimAccelerometer"));
    else if (!m_enabled && m_claimed)
        call(QStringLiteral("ReleaseAccelerometer"));
}

void OrientationSensor::call(const QString &method)
{
    auto msg = QDBusMessage::createMethodCall(m_proxy->service(), m_proxy->path(), m_proxy->interface(), method);
    m_inFlight = true;
    auto *w = new QDBusPendingCallWatcher(m_proxy->bus().asyncCall(msg, 5000), this);
    const bool claim = method.startsWith(QLatin1String("Claim"));
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, claim, method](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<> r = *c;
        m_inFlight = false;
        if (r.isError()) {
            m_lastError = r.error().message();
            qCWarning(lcSensor).noquote() << method << "failed:" << m_lastError;
            emit changed();
            return; // no automatic retry: the next enable or proxy restart tries again
        }
        m_lastError.clear();
        m_claimed = claim;
        qCInfo(lcSensor).noquote() << method << "ok";
        emit changed();
        sync(); // enabled may have changed while the call was in flight
    });
}
