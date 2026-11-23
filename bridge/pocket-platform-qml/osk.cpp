#include "osk.h"

#include "dbusservice.h"

#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QLoggingCategory>

Q_LOGGING_CATEGORY(lcOsk, "pocket.platform.osk", QtInfoMsg)

OnScreenKeyboard::OnScreenKeyboard(QObject *parent)
    : QObject(parent),
      m_osk(new DBusPropertyMirror(QDBusConnection::sessionBus(), QStringLiteral("sm.puri.OSK0"),
                                   QStringLiteral("/sm/puri/OSK0"), QStringLiteral("sm.puri.OSK0"), this))
{
    connect(m_osk, &DBusPropertyMirror::presentChanged, this, [this] {
        qCInfo(lcOsk) << "keyboard service" << (m_osk->present() ? "present" : "gone");
        emit changed();
    });
    connect(m_osk, &DBusPropertyMirror::propertiesChanged, this, [this] {
        qCInfo(lcOsk) << "keyboard visible" << visible();
        emit changed();
    });
}

bool OnScreenKeyboard::available() const { return m_osk->present(); }
bool OnScreenKeyboard::visible() const { return m_osk->value(QStringLiteral("Visible")).toBool(); }

void OnScreenKeyboard::setVisible(bool visible)
{
    if (!available())
        return;
    auto msg = QDBusMessage::createMethodCall(m_osk->service(), m_osk->path(), m_osk->interface(),
                                              QStringLiteral("SetVisible"));
    msg << visible;
    m_osk->bus().asyncCall(msg, 3000);
}
