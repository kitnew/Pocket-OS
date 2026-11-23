// Audio: whether the kernel exposes any sound card (/proc/asound/cards). Volume and mute
// go through PipeWire (apps/shell/services/AudioService.qml); this only lets the UI tell
// "no sound hardware" apart from "audio service not running".
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

class Audio : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(int cards READ cards NOTIFY cardsChanged)

public:
    explicit Audio(QObject *parent = nullptr);
    int cards() const { return m_cards; }
    Q_INVOKABLE void refresh();

signals:
    void cardsChanged();

private:
    int m_cards = 0;
};
