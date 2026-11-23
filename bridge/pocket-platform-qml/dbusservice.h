// Small helper shared by the D-Bus adapters: follows the owner of a well-known name and
// mirrors the properties of one interface (GetAll + PropertiesChanged).
#pragma once

#include <QDBusConnection>
#include <QObject>
#include <QVariantMap>

class QDBusServiceWatcher;

class DBusPropertyMirror : public QObject {
    Q_OBJECT
public:
    DBusPropertyMirror(const QDBusConnection &bus, const QString &service, const QString &path,
                       const QString &interface, QObject *parent);

    bool present() const { return m_present; }
    QVariant value(const QString &key) const { return m_props.value(key); }
    QDBusConnection bus() const { return m_bus; }
    QString service() const { return m_service; }
    QString path() const { return m_path; }
    QString interface() const { return m_interface; }

signals:
    // Service appeared (after its properties were read) or disappeared.
    void presentChanged();
    void propertiesChanged(const QStringList &keys);

private slots:
    void onPropertiesChanged(const QString &iface, const QVariantMap &changed, const QStringList &invalidated);

private:
    void fetch();

    QDBusConnection m_bus;
    QString m_service, m_path, m_interface;
    QDBusServiceWatcher *m_watcher;
    QVariantMap m_props;
    bool m_present = false;
};
