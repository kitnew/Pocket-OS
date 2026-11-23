#include "audio.h"

#include <QFile>
#include <QRegularExpression>

Audio::Audio(QObject *parent) : QObject(parent) { refresh(); }

void Audio::refresh()
{
    int n = 0;
    QFile f(QStringLiteral("/proc/asound/cards"));
    if (f.open(QIODevice::ReadOnly)) {
        static const QRegularExpression card(QStringLiteral("^\\s*\\d+\\s+\\["));
        for (const QByteArray &line : f.readAll().split('\n'))
            if (card.match(QString::fromUtf8(line)).hasMatch())
                ++n;
    }
    if (n != m_cards) {
        m_cards = n;
        emit cardsChanged();
    }
}
