#include "dbusservice.h"

#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>

DBusPropertyMirror::DBusPropertyMirror(const QDBusConnection &bus, const QString &service,
                                       const QString &path, const QString &interface, QObject *parent)
    : QObject(parent), m_bus(bus), m_service(service), m_path(path), m_interface(interface),
      m_watcher(new QDBusServiceWatcher(service, bus, QDBusServiceWatcher::WatchForOwnerChange, this))
{
    connect(m_watcher, &QDBusServiceWatcher::serviceOwnerChanged, this,
            [this](const QString &, const QString &, const QString &newOwner) {
                if (newOwner.isEmpty()) {
                    m_props.clear();
                    if (m_present) {
                        m_present = false;
                        emit presentChanged();
                    }
                } else {
                    fetch();
                }
            });
    m_bus.connect(service, path, QStringLiteral("org.freedesktop.DBus.Properties"),
                  QStringLiteral("PropertiesChanged"), this,
                  SLOT(onPropertiesChanged(QString, QVariantMap, QStringList)));
    // Only read if it is already running: activatable services are not started by us.
    if (m_bus.interface() && m_bus.interface()->isServiceRegistered(service))
        fetch();
}

void DBusPropertyMirror::fetch()
{
    auto msg = QDBusMessage::createMethodCall(m_service, m_path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                              QStringLiteral("GetAll"));
    msg << m_interface;
    auto *call = new QDBusPendingCallWatcher(m_bus.asyncCall(msg, 5000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<QVariantMap> reply = *c;
        if (reply.isError())
            return;
        m_props = reply.value();
        const bool was = m_present;
        m_present = true;
        if (!was)
            emit presentChanged();
        emit propertiesChanged(m_props.keys());
    });
}

void DBusPropertyMirror::onPropertiesChanged(const QString &iface, const QVariantMap &changed,
                                             const QStringList &invalidated)
{
    if (iface != m_interface)
        return;
    for (auto it = changed.begin(); it != changed.end(); ++it)
        m_props.insert(it.key(), it.value());
    if (!invalidated.isEmpty()) {
        fetch();
        return;
    }
    emit propertiesChanged(changed.keys());
}
