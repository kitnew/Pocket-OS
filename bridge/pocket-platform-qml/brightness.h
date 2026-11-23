// Brightness: the panel backlight. Read from sysfs (/sys/class/backlight/<dev>), set
// through logind (org.freedesktop.login1.Session.SetBrightness on the caller's own
// session), which lets the active seat session change it without root. Writes are
// coalesced: at most one call in flight, the latest requested value wins.
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

class Brightness : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available CONSTANT)
    Q_PROPERTY(QString device READ device CONSTANT)
    // 0..1 of max_brightness, as last read back from sysfs.
    Q_PROPERTY(double value READ value NOTIFY valueChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)

public:
    explicit Brightness(QObject *parent = nullptr);

    bool available() const { return m_max > 0; }
    QString device() const { return m_device; }
    double value() const { return m_max > 0 ? double(m_raw) / m_max : 0.0; }
    QString lastError() const { return m_lastError; }

    // Clamped to [minimum, 1]; the panel is never turned fully dark by this control.
    Q_INVOKABLE void setValue(double v);
    Q_INVOKABLE void refresh();

signals:
    void valueChanged();
    void lastErrorChanged();

private:
    void send();
    void setError(const QString &e);

    QString m_device;
    QString m_path;
    int m_max = 0;
    int m_raw = 0;
    int m_pending = -1;
    bool m_inFlight = false;
    QString m_lastError;
};
