// DisplayPower: turns the outputs off and on through wlr-output-power-management-v1
// (Phoc 0.59: zwlr_output_power_manager_v1). Phoc ignores touch input while the
// built-in output is off, so a blanked screen does not react to touches.
//
// This is display power management only. It is NOT a lock: nothing is authenticated
// and the session stays accessible (see docs/decisions/0005-applications-and-window-management.md).
#pragma once

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QtQml/qqmlregistration.h>

class QScreen;
class PowerManagerExt;
class OutputPower;

class DisplayPower : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available NOTIFY availableChanged)
    // true when every known output reports mode "on".
    Q_PROPERTY(bool on READ on NOTIFY onChanged)

public:
    explicit DisplayPower(QObject *parent = nullptr);
    ~DisplayPower() override;

    bool available() const;
    bool on() const;

    Q_INVOKABLE void setOn(bool on);

signals:
    void availableChanged();
    void onChanged();
    void failed(const QString &output);

private:
    void attach(QScreen *screen);
    void detach(QScreen *screen);
    void recompute();

    PowerManagerExt *m_manager = nullptr;
    QHash<QScreen *, OutputPower *> m_outputs;
    bool m_on = true;
};
