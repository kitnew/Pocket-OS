// AppCatalog: the installed applications served by Pocket Core
// (org.pocketos.Core1.Apps on /org/pocketos/Core1/Apps, protocol >= 1.1), as a list
// model, plus launching.
//
// It has no D-Bus connection of its own: it uses the `core` CoreClient's connection and
// follows its status, so one shell process keeps one logical link to the core. The
// catalog is re-read whenever the core (re)connects and on AppsChanged. When the core
// is down the last catalog stays visible (`stale` is true) but launching fails visibly.
#pragma once

#include <QAbstractListModel>
#include <QPointer>
#include <QSortFilterProxyModel>
#include <QString>
#include <QStringList>
#include <QtQml/qqmlregistration.h>

#include "coreclient.h"

class QDBusPendingCallWatcher;

class AppCatalog : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(CoreClient *core READ core WRITE setCore NOTIFY coreChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
    Q_PROPERTY(bool loaded READ loaded NOTIFY loadedChanged)
    Q_PROPERTY(bool stale READ stale NOTIFY loadedChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)

public:
    enum Role {
        AppIdRole = Qt::UserRole + 1,
        NameRole,
        GenericNameRole,
        CommentRole,
        IconNameRole,
        IconSourceRole, // file URL of the resolved icon, or empty
        CategoriesRole,
        KeywordsRole,
        WmClassRole,
    };
    Q_ENUM(Role)

    explicit AppCatalog(QObject *parent = nullptr);

    CoreClient *core() const { return m_core; }
    void setCore(CoreClient *core);
    bool loaded() const { return m_loaded; }
    bool stale() const { return m_loaded && (!m_core || m_core->status() != CoreClient::Connected); }
    QString lastError() const { return m_lastError; }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    // Map of one entry (keys: appId, name, iconSource, ...), or empty if unknown.
    Q_INVOKABLE QVariantMap get(const QString &appId) const;
    // ID of the entry at `row` (catalog order), or empty.
    Q_INVOKABLE QString idAt(int row) const
    {
        return row >= 0 && row < m_entries.size() ? m_entries[row].id : QString();
    }
    // ID of the entry a Wayland app_id belongs to (ID or StartupWMClass,
    // case-insensitive), or empty.
    Q_INVOKABLE QString idForWindow(const QString &waylandAppId) const;
    // Starts the app. Result: launched(id, pid) or launchFailed(id, error).
    Q_INVOKABLE void launch(const QString &appId);
    Q_INVOKABLE void reload();

signals:
    void coreChanged();
    void countChanged();
    void loadedChanged();
    void lastErrorChanged();
    void launched(const QString &appId, uint pid);
    void launchFailed(const QString &appId, const QString &error);
    // A process started by launch() ended. A failing exit soon after launch usually
    // means the app could not start (missing display, crash).
    void exited(const QString &appId, uint pid, bool success, const QString &detail);

private slots:
    void onAppsChanged();
    void onAppExited(const QString &id, uint pid, bool success, const QString &detail);

private:
    struct Entry {
        QString id, name, genericName, comment, iconName, iconSource, wmClass;
        QStringList categories, keywords;
    };
    void subscribe();
    void fetch();
    void setError(const QString &e);
    int indexOf(const QString &id) const;

    QPointer<CoreClient> m_core;
    QList<Entry> m_entries;
    QString m_subscribedBus;
    bool m_loaded = false;
    int m_generation = 0;
    QString m_lastError;
};

// Case-insensitive search over name, generic name, keywords and ID of an AppCatalog.
class AppFilter : public QSortFilterProxyModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString query READ query WRITE setQuery NOTIFY queryChanged)
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    explicit AppFilter(QObject *parent = nullptr);
    QString query() const { return m_query; }
    void setQuery(const QString &q);
    int count() const { return rowCount(); }

signals:
    void queryChanged();
    void countChanged();

protected:
    bool filterAcceptsRow(int row, const QModelIndex &parent) const override;

private:
    QString m_query;
};
