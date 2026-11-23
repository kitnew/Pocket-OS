// OutputConfig: reads and changes the transform (rotation) of the built-in output
// through wlr-output-management-v1 (Phoc 0.59 serves v4). Only the transform is
// changed; every other head property keeps its current value (wlroots initialises a
// configuration head from the current state).
#pragma once

#include <QList>
#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

class OutputManagerExt;
class OutputHead;

class OutputConfig : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available NOTIFY changed)
    // Output being controlled: the first enabled head (on lavender: DSI-1).
    Q_PROPERTY(QString outputName READ outputName NOTIFY changed)
    // Rotation in degrees, counter-clockwise as wl_output.transform: 0, 90, 180, 270.
    Q_PROPERTY(int rotation READ rotation NOTIFY changed)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)

public:
    explicit OutputConfig(QObject *parent = nullptr);
    ~OutputConfig() override;

    bool available() const;
    QString outputName() const;
    int rotation() const;
    bool busy() const { return m_busy; }

    Q_INVOKABLE void setRotation(int degrees);

signals:
    void changed();
    void busyChanged();
    void applied(bool ok, const QString &detail);

private:
    friend class OutputManagerExt;
    friend class OutputHead;
    friend class OutputConfiguration;
    OutputHead *primary() const;
    void finish(bool ok, const QString &detail);

    OutputManagerExt *m_manager = nullptr;
    QList<OutputHead *> m_heads;
    uint32_t m_serial = 0;
    bool m_busy = false;
    int m_pendingRotation = -1;
};
