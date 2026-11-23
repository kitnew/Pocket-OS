#include "appcatalog.h"

#include <QDBusArgument>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLoggingCategory>
#include <QUrl>

Q_LOGGING_CATEGORY(lcApps, "pocket.core.apps", QtInfoMsg)

namespace {
// Contract: pocket_ipc::dbus::apps (Rust) / docs/decisions/0002-pocket-core-and-ipc.md.
const QString kService = QStringLiteral("org.pocketos.Core1");
const QString kPath = QStringLiteral("/org/pocketos/Core1/Apps");
const QString kInterface = QStringLiteral("org.pocketos.Core1.Apps");
const int kCallTimeoutMs = 10000;
} // namespace

AppCatalog::AppCatalog(QObject *parent) : QAbstractListModel(parent)
{
    qDBusRegisterMetaType<QList<QVariantMap>>();
    connect(this, &QAbstractItemModel::rowsInserted, this, &AppCatalog::countChanged);
    connect(this, &QAbstractItemModel::rowsRemoved, this, &AppCatalog::countChanged);
    connect(this, &QAbstractItemModel::modelReset, this, &AppCatalog::countChanged);
}

void AppCatalog::setCore(CoreClient *core)
{
    if (core == m_core)
        return;
    if (m_core)
        disconnect(m_core, nullptr, this, nullptr);
    m_core = core;
    m_subscribedBus.clear();
    if (m_core) {
        connect(m_core, &CoreClient::statusChanged, this, [this] {
            emit loadedChanged(); // `stale` depends on the core status
            if (m_core->status() == CoreClient::Connected) {
                subscribe();
                fetch();
            }
        });
        connect(m_core, &CoreClient::busChanged, this, [this] { m_subscribedBus.clear(); });
        if (m_core->status() == CoreClient::Connected) {
            subscribe();
            fetch();
        }
    }
    emit coreChanged();
}

void AppCatalog::subscribe()
{
    QDBusConnection bus = m_core->bus();
    if (m_subscribedBus == bus.name())
        return;
    m_subscribedBus = bus.name();
    // Signals follow the well-known name, so subscriptions survive core restarts.
    bus.connect(kService, kPath, kInterface, QStringLiteral("AppsChanged"), this, SLOT(onAppsChanged()));
    bus.connect(kService, kPath, kInterface, QStringLiteral("AppExited"), this,
                SLOT(onAppExited(QString, uint, bool, QString)));
}

void AppCatalog::fetch()
{
    if (!m_core)
        return;
    const int gen = ++m_generation;
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("ListApps"));
    auto *call = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, gen](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        if (gen != m_generation)
            return; // a newer fetch is in flight
        QDBusPendingReply<QList<QVariantMap>> reply = *c;
        if (reply.isError()) {
            qCWarning(lcApps).noquote() << "ListApps failed:" << reply.error().message();
            setError(reply.error().message());
            return;
        }
        QList<Entry> entries;
        for (const QVariantMap &m : reply.value()) {
            Entry e;
            e.id = m.value(QStringLiteral("id")).toString();
            if (e.id.isEmpty())
                continue;
            e.name = m.value(QStringLiteral("name")).toString();
            e.genericName = m.value(QStringLiteral("generic_name")).toString();
            e.comment = m.value(QStringLiteral("comment")).toString();
            e.iconName = m.value(QStringLiteral("icon_name")).toString();
            const QString iconPath = m.value(QStringLiteral("icon_path")).toString();
            e.iconSource = iconPath.isEmpty() ? QString() : QUrl::fromLocalFile(iconPath).toString();
            e.wmClass = m.value(QStringLiteral("startup_wm_class")).toString();
            e.categories = m.value(QStringLiteral("categories")).toStringList();
            e.keywords = m.value(QStringLiteral("keywords")).toStringList();
            entries.append(e);
        }
        beginResetModel();
        m_entries = entries;
        endResetModel();
        qCInfo(lcApps) << "catalog:" << m_entries.size() << "apps";
        setError(QString());
        if (!m_loaded) {
            m_loaded = true;
        }
        emit loadedChanged();
    });
}

void AppCatalog::reload()
{
    if (!m_core || m_core->status() != CoreClient::Connected)
        return;
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("Reload"));
    m_core->bus().asyncCall(msg, kCallTimeoutMs);
}

void AppCatalog::onAppsChanged()
{
    qCInfo(lcApps) << "catalog changed";
    fetch();
}

void AppCatalog::onAppExited(const QString &id, uint pid, bool success, const QString &detail)
{
    qCInfo(lcApps).noquote() << "exited" << id << pid << (success ? "ok" : "failed") << detail;
    emit exited(id, pid, success, detail);
}

void AppCatalog::launch(const QString &appId)
{
    if (!m_core || m_core->status() != CoreClient::Connected) {
        emit launchFailed(appId, QStringLiteral("Pocket Core is not connected"));
        return;
    }
    auto msg = QDBusMessage::createMethodCall(kService, kPath, kInterface, QStringLiteral("Launch"));
    msg << appId;
    auto *call = new QDBusPendingCallWatcher(m_core->bus().asyncCall(msg, kCallTimeoutMs), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, appId](QDBusPendingCallWatcher *c) {
        c->deleteLater();
        QDBusPendingReply<uint> reply = *c;
        if (reply.isError()) {
            const QString e = reply.error().message();
            qCWarning(lcApps).noquote() << "Launch" << appId << "failed:" << reply.error().name() << e;
            emit launchFailed(appId, e);
        } else {
            qCInfo(lcApps).noquote() << "launched" << appId << "pid" << reply.value();
            emit launched(appId, reply.value());
        }
    });
}

int AppCatalog::indexOf(const QString &id) const
{
    for (int i = 0; i < m_entries.size(); ++i)
        if (m_entries[i].id == id)
            return i;
    return -1;
}

QVariantMap AppCatalog::get(const QString &appId) const
{
    const int i = indexOf(appId);
    if (i < 0)
        return {};
    QVariantMap m;
    const auto roles = roleNames();
    for (auto it = roles.begin(); it != roles.end(); ++it)
        m.insert(QString::fromLatin1(it.value()), data(index(i), it.key()));
    return m;
}

QString AppCatalog::idForWindow(const QString &waylandAppId) const
{
    if (waylandAppId.isEmpty())
        return {};
    for (const Entry &e : m_entries)
        if (e.id.compare(waylandAppId, Qt::CaseInsensitive) == 0
            || (!e.wmClass.isEmpty() && e.wmClass.compare(waylandAppId, Qt::CaseInsensitive) == 0))
            return e.id;
    return {};
}

int AppCatalog::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_entries.size());
}

QVariant AppCatalog::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_entries.size())
        return {};
    const Entry &e = m_entries[index.row()];
    switch (role) {
    case AppIdRole: return e.id;
    case Qt::DisplayRole:
    case NameRole: return e.name;
    case GenericNameRole: return e.genericName;
    case CommentRole: return e.comment;
    case IconNameRole: return e.iconName;
    case IconSourceRole: return e.iconSource;
    case CategoriesRole: return e.categories;
    case KeywordsRole: return e.keywords;
    case WmClassRole: return e.wmClass;
    }
    return {};
}

QHash<int, QByteArray> AppCatalog::roleNames() const
{
    return {
        {AppIdRole, "appId"},       {NameRole, "name"},           {GenericNameRole, "genericName"},
        {CommentRole, "comment"},   {IconNameRole, "iconName"},   {IconSourceRole, "iconSource"},
        {CategoriesRole, "categories"}, {KeywordsRole, "keywords"}, {WmClassRole, "wmClass"},
    };
}

void AppCatalog::setError(const QString &e)
{
    if (e != m_lastError) {
        m_lastError = e;
        emit lastErrorChanged();
    }
}

AppFilter::AppFilter(QObject *parent) : QSortFilterProxyModel(parent)
{
    connect(this, &QAbstractItemModel::rowsInserted, this, &AppFilter::countChanged);
    connect(this, &QAbstractItemModel::rowsRemoved, this, &AppFilter::countChanged);
    connect(this, &QAbstractItemModel::modelReset, this, &AppFilter::countChanged);
    connect(this, &QAbstractItemModel::layoutChanged, this, &AppFilter::countChanged);
}

void AppFilter::setQuery(const QString &q)
{
    const QString t = q.trimmed();
    if (t == m_query)
        return;
    beginFilterChange();
    m_query = t;
    endFilterChange(QSortFilterProxyModel::Direction::Rows);
    emit queryChanged();
    emit countChanged();
}

bool AppFilter::filterAcceptsRow(int row, const QModelIndex &parent) const
{
    if (m_query.isEmpty())
        return true;
    const QModelIndex i = sourceModel()->index(row, 0, parent);
    auto has = [&](int role) {
        return i.data(role).toString().contains(m_query, Qt::CaseInsensitive);
    };
    if (has(AppCatalog::NameRole) || has(AppCatalog::GenericNameRole) || has(AppCatalog::AppIdRole))
        return true;
    for (const QString &k : i.data(AppCatalog::KeywordsRole).toStringList())
        if (k.contains(m_query, Qt::CaseInsensitive))
            return true;
    return false;
}
