// OrientationSensor: accelerometer orientation from iio-sensor-proxy
// (net.hadess.SensorProxy, system bus). While `enabled`, the accelerometer is claimed
// (the proxy only reads the sensor for claimed clients; polkit allows the claim for the
// active local session). Values: "normal", "bottom-up", "left-up", "right-up",
// "undefined".
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

class DBusPropertyMirror;

class OrientationSensor : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available NOTIFY changed)
    Q_PROPERTY(bool enabled READ enabled WRITE setEnabled NOTIFY enabledChanged)
    Q_PROPERTY(bool claimed READ claimed NOTIFY changed)
    Q_PROPERTY(QString orientation READ orientation NOTIFY changed)
    Q_PROPERTY(QString lastError READ lastError NOTIFY changed)

public:
    explicit OrientationSensor(QObject *parent = nullptr);
    ~OrientationSensor() override;

    bool available() const;
    bool enabled() const { return m_enabled; }
    void setEnabled(bool e);
    bool claimed() const { return m_claimed; }
    QString orientation() const;
    QString lastError() const { return m_lastError; }

signals:
    void changed();
    void enabledChanged();

private:
    void sync();
    void call(const QString &method);

    DBusPropertyMirror *m_proxy;
    bool m_enabled = false;
    bool m_claimed = false;
    bool m_inFlight = false;
    QString m_lastError;
};
