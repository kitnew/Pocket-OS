#include "powerkey.h"

#include <QLoggingCategory>
#include <QtWaylandClient/QWaylandClientExtensionTemplate>

#include "qwayland-phosh-private.h"

Q_LOGGING_CATEGORY(lcKey, "pocket.platform.powerkey", QtInfoMsg)

namespace {
const char kAccelerator[] = "XF86PowerOff";
}

class PhoshPrivateExt : public QWaylandClientExtensionTemplate<PhoshPrivateExt>,
                        public QtWayland::phosh_private {
public:
    // v7: keyboard events incl. accelerator release (the interface's own max version).
    PhoshPrivateExt() : QWaylandClientExtensionTemplate(7) { initialize(); }
};

class KeyboardEvent : public QtWayland::phosh_private_keyboard_event {
public:
    KeyboardEvent(struct ::phosh_private_keyboard_event *obj, PowerKey *owner)
        : QtWayland::phosh_private_keyboard_event(obj), m_owner(owner) {}
    ~KeyboardEvent() override { destroy(); }

    uint32_t action = 0;

protected:
    void phosh_private_keyboard_event_grab_success_event(const QString &accel,
                                                         uint32_t action_id) override
    {
        qCInfo(lcKey).noquote() << "grabbed" << accel << "action" << action_id;
        action = action_id;
        m_owner->setActive(true, QString());
    }
    void phosh_private_keyboard_event_grab_failed_event(const QString &accel,
                                                        uint32_t error) override
    {
        qCWarning(lcKey).noquote() << "grab of" << accel << "failed, error" << error;
        m_owner->setActive(false, QStringLiteral("grab failed (error %1)").arg(error));
    }
    void phosh_private_keyboard_event_accelerator_activated_event(uint32_t action_id,
                                                                  uint32_t) override
    {
        if (action_id == action)
            emit m_owner->pressed();
    }
    void phosh_private_keyboard_event_accelerator_released_event(uint32_t action_id,
                                                                 uint32_t) override
    {
        if (action_id == action)
            emit m_owner->released();
    }

private:
    PowerKey *m_owner;
};

PowerKey::PowerKey(QObject *parent) : QObject(parent), m_ext(new PhoshPrivateExt)
{
    m_ext->setParent(this);
    auto setup = [this] {
        delete m_events;
        m_events = nullptr;
        if (!m_ext->isActive()) {
            setActive(false, QStringLiteral("phosh_private not offered by the compositor"));
            return;
        }
        if (m_ext->QWaylandClientExtension::version() < 5) {
            setActive(false, QStringLiteral("phosh_private too old (v%1)").arg(m_ext->QWaylandClientExtension::version()));
            return;
        }
        m_events = new KeyboardEvent(m_ext->get_keyboard_event(), this);
        m_events->grab_accelerator_request(QString::fromLatin1(kAccelerator));
    };
    connect(m_ext, &QWaylandClientExtension::activeChanged, this, setup);
    if (m_ext->isActive())
        setup();
}

PowerKey::~PowerKey()
{
    delete m_events;
}

void PowerKey::setActive(bool active, const QString &error)
{
    if (active == m_active && error == m_error)
        return;
    m_active = active;
    m_error = error;
    emit activeChanged();
}
