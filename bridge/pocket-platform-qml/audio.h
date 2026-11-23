// Audio: whether the kernel exposes any sound card (/proc/asound/cards). Pocket has no
// volume control yet; Quick Settings uses this to say so truthfully (on lavender the
// mainline kernel currently registers no sound card).
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
