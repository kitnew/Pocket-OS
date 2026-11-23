#include "outputconfig.h"

#include <QLoggingCategory>
#include <QtWaylandClient/QWaylandClientExtensionTemplate>

#include "qwayland-wlr-output-management-unstable-v1.h"
#include <wayland-client-protocol.h>

Q_LOGGING_CATEGORY(lcOutput, "pocket.platform.output", QtInfoMsg)

namespace {
int transformToDegrees(int32_t t)
{
    switch (t) {
    case WL_OUTPUT_TRANSFORM_90: return 90;
    case WL_OUTPUT_TRANSFORM_180: return 180;
    case WL_OUTPUT_TRANSFORM_270: return 270;
    default: return 0; // flipped variants are not used by Pocket
    }
}
int32_t degreesToTransform(int d)
{
    switch (d) {
    case 90: return WL_OUTPUT_TRANSFORM_90;
    case 180: return WL_OUTPUT_TRANSFORM_180;
    case 270: return WL_OUTPUT_TRANSFORM_270;
    default: return WL_OUTPUT_TRANSFORM_NORMAL;
    }
}
} // namespace

// Modes are announced as new objects; Pocket keeps none of their data, it only has to
// release them.
class OutputMode : public QtWayland::zwlr_output_mode_v1 {
public:
    using QtWayland::zwlr_output_mode_v1::zwlr_output_mode_v1;
    ~OutputMode() override
    {
        if (object()) {
            if (version() >= 3)
                release();
            else
                zwlr_output_mode_v1_destroy(object());
        }
    }

protected:
    void zwlr_output_mode_v1_finished() override { delete this; }
};

class OutputHead : public QtWayland::zwlr_output_head_v1 {
public:
    OutputHead(struct ::zwlr_output_head_v1 *obj, OutputConfig *owner)
        : QtWayland::zwlr_output_head_v1(obj), m_owner(owner) {}
    ~OutputHead() override
    {
        if (object()) {
            if (version() >= 3)
                release();
            else
                zwlr_output_head_v1_destroy(object());
        }
    }

    QString name;
    bool enabled = false;
    int32_t transform = WL_OUTPUT_TRANSFORM_NORMAL;

protected:
    void zwlr_output_head_v1_name(const QString &n) override { name = n; }
    void zwlr_output_head_v1_enabled(int32_t e) override { enabled = e; }
    void zwlr_output_head_v1_transform(int32_t t) override { transform = t; }
    void zwlr_output_head_v1_mode(struct ::zwlr_output_mode_v1 *mode) override
    {
        new OutputMode(mode);
    }
    void zwlr_output_head_v1_finished() override
    {
        m_owner->m_heads.removeAll(this);
        emit m_owner->changed();
        delete this;
    }

private:
    OutputConfig *m_owner;
};

class OutputManagerExt
    : public QWaylandClientExtensionTemplate<OutputManagerExt>,
      public QtWayland::zwlr_output_manager_v1 {
public:
    explicit OutputManagerExt(OutputConfig *owner)
        : QWaylandClientExtensionTemplate(4), m_owner(owner)
    {
        initialize();
    }

protected:
    void zwlr_output_manager_v1_head(struct ::zwlr_output_head_v1 *head) override
    {
        m_owner->m_heads.append(new OutputHead(head, m_owner));
    }
    void zwlr_output_manager_v1_done(uint32_t serial) override
    {
        m_owner->m_serial = serial;
        if (auto *h = m_owner->primary())
            qCInfo(lcOutput).noquote() << "output" << h->name << "rotation"
                                       << transformToDegrees(h->transform) << "serial" << serial;
        emit m_owner->changed();
    }
    void zwlr_output_manager_v1_finished() override {}

private:
    OutputConfig *m_owner;
};

class OutputConfiguration : public QtWayland::zwlr_output_configuration_v1 {
public:
    OutputConfiguration(struct ::zwlr_output_configuration_v1 *obj, OutputConfig *owner)
        : QtWayland::zwlr_output_configuration_v1(obj), m_owner(owner) {}
    ~OutputConfiguration() override
    {
        destroy();
        // Configuration heads have no destructor request; free the client proxies.
        for (auto *h : std::as_const(heads))
            zwlr_output_configuration_head_v1_destroy(h);
    }

    QList<struct ::zwlr_output_configuration_head_v1 *> heads;

protected:
    void zwlr_output_configuration_v1_succeeded() override { done(true, QStringLiteral("applied")); }
    void zwlr_output_configuration_v1_failed() override { done(false, QStringLiteral("rejected by the compositor")); }
    void zwlr_output_configuration_v1_cancelled() override { done(false, QStringLiteral("outputs changed meanwhile")); }

private:
    void done(bool ok, const QString &detail)
    {
        m_owner->finish(ok, detail);
        delete this;
    }
    OutputConfig *m_owner;
};

OutputConfig::OutputConfig(QObject *parent) : QObject(parent), m_manager(new OutputManagerExt(this))
{
    m_manager->setParent(this);
    connect(m_manager, &QWaylandClientExtension::activeChanged, this, [this] {
        if (!m_manager->isActive()) {
            qDeleteAll(m_heads);
            m_heads.clear();
        }
        emit changed();
    });
}

OutputConfig::~OutputConfig()
{
    qDeleteAll(m_heads);
}

bool OutputConfig::available() const { return m_manager->isActive() && primary(); }

OutputHead *OutputConfig::primary() const
{
    for (auto *h : m_heads)
        if (h->enabled)
            return h;
    return nullptr;
}

QString OutputConfig::outputName() const
{
    auto *h = primary();
    return h ? h->name : QString();
}

int OutputConfig::rotation() const
{
    auto *h = primary();
    return h ? transformToDegrees(h->transform) : 0;
}

void OutputConfig::setRotation(int degrees)
{
    degrees = ((degrees % 360) + 360) % 360;
    if (degrees % 90) {
        emit applied(false, QStringLiteral("rotation must be a multiple of 90"));
        return;
    }
    if (!available()) {
        emit applied(false, QStringLiteral("output management unavailable"));
        return;
    }
    if (m_busy) { // one configuration at a time; apply the latest request afterwards
        m_pendingRotation = degrees;
        return;
    }
    if (degrees == rotation()) {
        emit applied(true, QStringLiteral("unchanged"));
        return;
    }
    qCInfo(lcOutput) << "request rotation" << degrees;
    auto *config = new OutputConfiguration(m_manager->create_configuration(m_serial), this);
    for (auto *h : std::as_const(m_heads)) {
        if (!h->enabled) {
            config->disable_head(h->object());
            continue;
        }
        auto *ch = config->enable_head(h->object());
        config->heads.append(ch);
        if (h == primary())
            zwlr_output_configuration_head_v1_set_transform(ch, degreesToTransform(degrees));
    }
    config->apply();
    m_busy = true;
    emit busyChanged();
}

void OutputConfig::finish(bool ok, const QString &detail)
{
    qCInfo(lcOutput).noquote() << "configuration" << (ok ? "applied" : "failed:") << (ok ? "" : detail);
    m_busy = false;
    emit busyChanged();
    emit applied(ok, detail);
    if (m_pendingRotation >= 0) {
        const int next = m_pendingRotation;
        m_pendingRotation = -1;
        setRotation(next);
    }
}
