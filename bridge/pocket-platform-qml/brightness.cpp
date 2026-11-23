#include "brightness.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDir>
#include <QFile>
#include <QLoggingCategory>

Q_LOGGING_CATEGORY(lcBrightness, "pocket.platform.brightness", QtInfoMsg)

namespace {
const double kMinimum = 0.02;
int readInt(const QString &path)
{
    QFile f(path);
    if (!f.open(QIODevice::ReadOnly))
        return -1;
    bool ok = false;
    const int v = f.readAll().trimmed().toInt(&ok);
    return ok ? v : -1;
}
} // namespace

Brightness::Brightness(QObject *parent) : QObject(parent)
{
    const QDir dir(QStringLiteral("/sys/class/backlight"));
    const auto devices = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);
    if (devices.isEmpty()) {
        m_lastError = QStringLiteral("no backlight device");
        return;
    }
    m_device = devices.first();
    m_path = dir.filePath(m_device);
    m_max = qMax(0, readInt(m_path + QStringLiteral("/max_brightness")));
    refresh();
    qCInfo(lcBrightness).noquote() << "backlight" << m_device << "max" << m_max << "now" << m_raw;
}

void Brightness::refresh()
{
    if (m_path.isEmpty())
        return;
    const int raw = readInt(m_path + QStringLiteral("/brightness"));
    if (raw >= 0 && raw != m_raw) {
        m_raw = raw;
        emit valueChanged();
    }
}

void Brightness::setValue(double v)
{
    if (!available())
        return;
    v = qBound(kMinimum, v, 1.0);
    m_pending = qMax(1, int(v * m_max + 0.5));
    if (!m_inFlight)
        send();
}

void Brightness::send()
{
    const int target = m_pending;
    m_pending = -1;
    auto msg = QDBusMessage::createMethodCall(
        QStringLiteral("org.freedesktop.login1"), QStringLiteral("/org/freedesktop/login1/session/auto"),
        QStringLiteral("org.freedesktop.login1.Session"), QStringLiteral("SetBrightness"));
    msg << QStringLiteral("backlight") << m_device << uint(target);
    m_inFlight = true;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(msg, 3000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, target](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        m_inFlight = false;
        QDBusPendingReply<> reply = *c;
        if (reply.isError()) {
            qCWarning(lcBrightness).noquote() << "SetBrightness" << target << "failed:" << reply.error().message();
            setError(reply.error().message());
        } else {
            setError(QString());
        }
        refresh();
        if (m_pending >= 0)
            send();
    });
}

void Brightness::setError(const QString &e)
{
    if (e != m_lastError) {
        m_lastError = e;
        emit lastErrorChanged();
    }
}
