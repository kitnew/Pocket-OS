// OnScreenKeyboard: visibility of the system on-screen keyboard over its D-Bus control
// interface sm.puri.OSK0 (squeekboard; also implemented by phosh-osk-stevia). The
// keyboard itself is a separate Wayland client using input-method-v2; it shows itself
// when a text field gets focus. This object only reflects and overrides visibility.
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

class DBusPropertyMirror;

class OnScreenKeyboard : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available NOTIFY changed)
    Q_PROPERTY(bool visible READ visible NOTIFY changed)

public:
    explicit OnScreenKeyboard(QObject *parent = nullptr);

    bool available() const;
    bool visible() const;
    Q_INVOKABLE void setVisible(bool visible);
    Q_INVOKABLE void toggle() { setVisible(!visible()); }

signals:
    void changed();

private:
    DBusPropertyMirror *m_osk;
};
