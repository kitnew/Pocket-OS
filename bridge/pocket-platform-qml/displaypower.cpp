#include "displaypower.h"

#include <QGuiApplication>
#include <QLoggingCategory>
#include <QScreen>
#include <QtGui/qscreen_platform.h>
#include <QtWaylandClient/QWaylandClientExtensionTemplate>

#include "qwayland-wlr-output-power-management-unstable-v1.h"

Q_LOGGING_CATEGORY(lcPower, "pocket.platform.display", QtInfoMsg)

class PowerManagerExt
    : public QWaylandClientExtensionTemplate<PowerManagerExt,
                                             &QtWayland::zwlr_output_power_manager_v1::destroy>,
      public QtWayland::zwlr_output_power_manager_v1 {
public:
    PowerManagerExt() : QWaylandClientExtensionTemplate(1) { initialize(); }
};

class OutputPower : public QtWayland::zwlr_output_power_v1 {
public:
    OutputPower(struct ::zwlr_output_power_v1 *obj, QString name, DisplayPower *owner,
                std::function<void()> changed)
        : QtWayland::zwlr_output_power_v1(obj), name(std::move(name)), m_owner(owner),
          m_changed(std::move(changed)) {}
    ~OutputPower() override { destroy(); }

    QString name;
    bool on = true;
    bool valid = true;

protected:
    void zwlr_output_power_v1_mode(uint32_t mode) override
    {
        on = mode == ZWLR_OUTPUT_POWER_V1_MODE_ON;
        qCInfo(lcPower).noquote() << "output" << name << "power" << (on ? "on" : "off");
        m_changed();
    }
    void zwlr_output_power_v1_failed() override
    {
        // The output went away or another client holds power control.
        qCWarning(lcPower).noquote() << "output" << name << "power control failed";
        valid = false;
        emit m_owner->failed(name);
        m_changed();
    }

private:
    DisplayPower *m_owner;
    std::function<void()> m_changed;
};

DisplayPower::DisplayPower(QObject *parent) : QObject(parent), m_manager(new PowerManagerExt)
{
    m_manager->setParent(this);
    connect(m_manager, &QWaylandClientExtension::activeChanged, this, [this] {
        qCInfo(lcPower) << "output power manager" << (m_manager->isActive() ? "bound" : "gone");
        if (m_manager->isActive()) {
            for (QScreen *s : QGuiApplication::screens())
                attach(s);
        } else {
            qDeleteAll(m_outputs);
            m_outputs.clear();
        }
        emit availableChanged();
        recompute();
    });
    connect(qGuiApp, &QGuiApplication::screenAdded, this, &DisplayPower::attach);
    connect(qGuiApp, &QGuiApplication::screenRemoved, this, &DisplayPower::detach);
    if (m_manager->isActive())
        for (QScreen *s : QGuiApplication::screens())
            attach(s);
}

DisplayPower::~DisplayPower()
{
    qDeleteAll(m_outputs);
}

bool DisplayPower::available() const { return m_manager->isActive(); }
bool DisplayPower::on() const { return m_on; }

void DisplayPower::attach(QScreen *screen)
{
    if (!m_manager->isActive() || m_outputs.contains(screen))
        return;
    auto *ws = screen->nativeInterface<QNativeInterface::QWaylandScreen>();
    if (!ws || !ws->output())
        return;
    auto *obj = m_manager->get_output_power(ws->output());
    m_outputs.insert(screen, new OutputPower(obj, screen->name(), this, [this] { recompute(); }));
}

void DisplayPower::detach(QScreen *screen)
{
    delete m_outputs.take(screen);
    recompute();
}

void DisplayPower::recompute()
{
    bool on = true;
    for (auto *o : std::as_const(m_outputs))
        on = on && (!o->valid || o->on);
    if (on != m_on) {
        m_on = on;
        emit onChanged();
    }
}

void DisplayPower::setOn(bool on)
{
    if (!m_manager->isActive()) {
        qCWarning(lcPower) << "setOn: output power management unavailable";
        return;
    }
    qCInfo(lcPower) << "request display" << (on ? "on" : "off");
    for (auto *o : std::as_const(m_outputs))
        if (o->valid)
            o->set_mode(on ? ZWLR_OUTPUT_POWER_V1_MODE_ON : ZWLR_OUTPUT_POWER_V1_MODE_OFF);
}
