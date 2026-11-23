// PowerKey: receives the power key (keysym XF86PowerOff) from Phoc through the
// phosh_private keyboard-event accelerator grab (protocol v5+, Phoc 0.59 serves v8).
// While grabbed, Phoc delivers the key only to this client. The grab ends with the
// shell process; elogind's HandlePowerKey policy (packaging/elogind) applies to the
// logind side independently.
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

class PhoshPrivateExt;
class KeyboardEvent;

class PowerKey : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool active READ active NOTIFY activeChanged)
    Q_PROPERTY(QString error READ error NOTIFY activeChanged)

public:
    explicit PowerKey(QObject *parent = nullptr);
    ~PowerKey() override;

    bool active() const { return m_active; }
    QString error() const { return m_error; }

signals:
    void activeChanged();
    void pressed();
    void released();

private:
    friend class KeyboardEvent;
    void setActive(bool active, const QString &error);

    PhoshPrivateExt *m_ext = nullptr;
    KeyboardEvent *m_events = nullptr;
    bool m_active = false;
    QString m_error;
};
