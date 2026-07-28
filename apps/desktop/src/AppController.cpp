#include "AppController.hpp"

#include <QClipboard>
#include <QApplication>
#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QDesktopServices>
#include <QEvent>
#include <QFileInfo>
#include <QFile>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMimeDatabase>
#include <QMenu>
#include <QIcon>
#include <QKeyEvent>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSettings>
#include <QStandardPaths>
#include <QSystemTrayIcon>
#include <QTimer>
#include <QVariantMap>
#include <QWindow>
#include <algorithm>

namespace {
QVariantMap contact(const QString &name, const QString &summary, const QString &presence,
                    const QString &accent, const int unread, const QString &identityId,
                    const bool blocked, const bool hidePresence, const QString &avatar,
                    const QString &fingerprint, const bool manuallyVerified)
{
    return {{QStringLiteral("name"), name},
            {QStringLiteral("summary"), summary},
            {QStringLiteral("presence"), presence},
            {QStringLiteral("accent"), accent},
            {QStringLiteral("unread"), unread},
            {QStringLiteral("identityId"), identityId},
            {QStringLiteral("blocked"), blocked},
            {QStringLiteral("hidePresence"), hidePresence},
            {QStringLiteral("avatar"), avatar},
            {QStringLiteral("fingerprint"), fingerprint},
            {QStringLiteral("manuallyVerified"), manuallyVerified},
            {QStringLiteral("pinned"), false},
            {QStringLiteral("archived"), false},
            {QStringLiteral("muted"), false}};
}

QVariantMap message(const QString &author, const QString &body, const QString &time, const bool own,
                    const QString &id = {}, const QString &delivery = {},
                    const bool edited = false, const bool deleted = false,
                    const QString &replyTo = {}, const QString &filePath = {})
{
    return {{QStringLiteral("author"), author},
            {QStringLiteral("body"), body},
            {QStringLiteral("time"), time},
            {QStringLiteral("own"), own},
            {QStringLiteral("messageId"), id},
            {QStringLiteral("delivery"), delivery},
            {QStringLiteral("edited"), edited},
            {QStringLiteral("deleted"), deleted},
            {QStringLiteral("replyTo"), replyTo},
            {QStringLiteral("filePath"), filePath}};
}

QVariantMap group(const QString &name, const QString &id, const int members,
                  const bool owned, const bool administrator)
{
    return {{QStringLiteral("name"), name},
            {QStringLiteral("summary"), QStringLiteral("grupo MLS · %1 miembros").arg(members)},
            {QStringLiteral("presence"), QStringLiteral("cifrado MLS")},
            {QStringLiteral("accent"), QStringLiteral("#D091FF")},
            {QStringLiteral("unread"), 0},
            {QStringLiteral("group"), true},
            {QStringLiteral("groupId"), id},
            {QStringLiteral("owned"), owned},
            {QStringLiteral("administrator"), administrator},
            {QStringLiteral("pinned"), false},
            {QStringLiteral("archived"), false},
            {QStringLiteral("muted"), false}};
}

void applyConversationPreference(QVariantMap &item, const QVariantMap &preferences)
{
    if (preferences.isEmpty()) return;
    item[QStringLiteral("pinned")] = preferences.value(QStringLiteral("pinned"));
    item[QStringLiteral("archived")] = preferences.value(QStringLiteral("archived"));
    item[QStringLiteral("muted")] = preferences.value(QStringLiteral("muted"));
    item[QStringLiteral("unread")] = preferences.value(
        QStringLiteral("unread"), item.value(QStringLiteral("unread")));
    const auto summary = preferences.value(QStringLiteral("last_summary")).toString();
    if (!summary.isEmpty()) item[QStringLiteral("summary")] = summary;
    item[QStringLiteral("lastActivity")] = preferences.value(QStringLiteral("last_activity"), 0);
}

QVariantList participantListFromJson(const QJsonArray &values)
{
    QVariantList participants;
    for (const auto &value : values) {
        const auto participant = value.toObject();
        participants.append(QVariantMap{
            {QStringLiteral("name"), participant.value(QStringLiteral("name")).toString()},
            {QStringLiteral("deviceId"),
             participant.value(QStringLiteral("device_id")).toString()},
            {QStringLiteral("volume"),
             participant.value(QStringLiteral("volume")).toDouble(1.0)},
        });
    }
    return participants;
}
} // namespace

AppController::AppController(QObject *parent)
    : QObject(parent)
{
    QSettings settings;
    m_doNotDisturb = settings.value(QStringLiteral("notifications/doNotDisturb"), false).toBool();
    m_voiceMode = settings.value(QStringLiteral("calls/voiceMode"), QStringLiteral("open")).toString();
    m_pushToTalkShortcut =
        settings.value(QStringLiteral("calls/pushToTalkShortcut"),
                       QStringLiteral("Ctrl+Space")).toString();
    m_archivedVisible = settings.value(QStringLiteral("conversations/showArchived"), false).toBool();
    if (m_voiceMode != QStringLiteral("push_to_talk")) m_voiceMode = QStringLiteral("open");
    qApp->installEventFilter(this);
    if (QSystemTrayIcon::isSystemTrayAvailable()) {
        m_tray = new QSystemTrayIcon(QIcon::fromTheme(QStringLiteral("dialog-information")), this);
        m_tray->setToolTip(QStringLiteral("pptalk"));
        connect(m_tray, &QSystemTrayIcon::messageClicked, this, [this]() {
            for (auto *window : QGuiApplication::topLevelWindows()) {
                window->show();
                window->raise();
                window->requestActivate();
            }
            if (!m_notificationConversationKey.isEmpty()) {
                for (qsizetype index = 0; index < m_contacts.size(); ++index) {
                    const auto item = m_contacts.at(index).toMap();
                    const auto matches =
                        item.value(QStringLiteral("name")).toString()
                            .compare(m_notificationConversationKey, Qt::CaseInsensitive) == 0 ||
                        item.value(QStringLiteral("identityId")).toString() ==
                            m_notificationConversationKey ||
                        item.value(QStringLiteral("groupId")).toString() ==
                            m_notificationConversationKey;
                    if (matches) {
                        selectConversation(static_cast<int>(index));
                        break;
                    }
                }
            }
        });
        auto *menu = new QMenu;
        auto *openAction = menu->addAction(QStringLiteral("Abrir pptalk"));
        connect(openAction, &QAction::triggered, this, [] {
            for (auto *window : QGuiApplication::topLevelWindows()) {
                window->show();
                window->raise();
                window->requestActivate();
            }
        });
        auto *dndAction = menu->addAction(QStringLiteral("No molestar"));
        dndAction->setCheckable(true);
        dndAction->setChecked(m_doNotDisturb);
        connect(dndAction, &QAction::toggled, this, &AppController::setDoNotDisturb);
        menu->addSeparator();
        auto *quitAction = menu->addAction(QStringLiteral("Salir"));
        connect(quitAction, &QAction::triggered, qApp, &QCoreApplication::quit);
        m_tray->setContextMenu(menu);
        m_tray->show();
    }
    startBackend();
}

AppController::~AppController()
{
    m_shuttingDown = true;
    if (m_backend == nullptr || m_backend->state() == QProcess::NotRunning) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("shutdown")}});
    m_backend->closeWriteChannel();
    if (!m_backend->waitForFinished(1500)) {
        m_backend->terminate();
        if (!m_backend->waitForFinished(1000)) m_backend->kill();
    }
}

QVariantList AppController::contacts() const { return m_contacts; }
QVariantList AppController::messages() const { return m_messages; }
QVariantList AppController::devices() const { return m_devices; }
QVariantList AppController::directContacts() const { return m_directContacts; }
QVariantList AppController::mediaDevices() const { return m_mediaDevices; }
QVariantList AppController::callParticipants() const { return m_callParticipants; }
QVariantList AppController::transfers() const { return m_transfers; }
QVariantList AppController::searchResults() const { return m_searchResults; }
QString AppController::profileName() const { return m_profileName; }
QString AppController::profileAvatar() const { return m_profileAvatar; }

QString AppController::conversationName() const
{
    if (m_contacts.isEmpty()) {
        return QStringLiteral("Invita a alguien");
    }
    return m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("name")).toString();
}

QString AppController::presence() const
{
    if (m_contacts.isEmpty()) {
        return QStringLiteral("sin cuenta central");
    }
    return m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("presence")).toString();
}

QString AppController::connectionLabel() const
{
    if (!m_lastError.isEmpty()) {
        return QStringLiteral("revisa el error");
    }
    const auto currentPresence = presence();
    return currentPresence.contains(QStringLiteral("directo")) ? QStringLiteral("P2P directo")
                                                                : QStringLiteral("E2EE · ruta automática");
}

QString AppController::lastError() const { return m_lastError; }
QString AppController::focusedMessageId() const { return m_focusedMessageId; }

bool AppController::conversationIsGroup() const
{
    return !m_contacts.isEmpty() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("group")).toBool();
}

bool AppController::currentGroupOwned() const
{
    return conversationIsGroup() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("owned")).toBool();
}

bool AppController::currentGroupAdmin() const
{
    return conversationIsGroup() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("administrator")).toBool();
}

bool AppController::currentContactBlocked() const
{
    return !m_contacts.isEmpty() && !conversationIsGroup() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("blocked")).toBool();
}

bool AppController::currentContactPrivacyHidden() const
{
    return !m_contacts.isEmpty() && !conversationIsGroup() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("hidePresence")).toBool();
}

bool AppController::currentConversationPinned() const
{
    return !m_contacts.isEmpty() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("pinned")).toBool();
}

bool AppController::currentConversationArchived() const
{
    return !m_contacts.isEmpty() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("archived")).toBool();
}

bool AppController::currentConversationMuted() const
{
    return !m_contacts.isEmpty() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("muted")).toBool();
}

QString AppController::inviteLink() const { return m_inviteLink; }
QString AppController::inviteQr() const { return m_inviteQr; }
QString AppController::invitePreviewName() const { return m_invitePreviewName; }
QString AppController::invitePreviewExpiry() const { return m_invitePreviewExpiry; }
QString AppController::deviceLink() const { return m_deviceLink; }
QString AppController::currentContactFingerprint() const
{
    if (m_contacts.isEmpty() || conversationIsGroup()) return {};
    return m_contacts.value(m_selectedConversation).toMap()
        .value(QStringLiteral("fingerprint")).toString();
}
bool AppController::currentContactVerified() const
{
    return !m_contacts.isEmpty() && !conversationIsGroup() &&
           m_contacts.value(m_selectedConversation).toMap()
               .value(QStringLiteral("manuallyVerified")).toBool();
}
bool AppController::onboardingRequired() const { return m_onboardingRequired; }
QString AppController::onboardingLink() const { return m_onboardingLink; }
QString AppController::backupStatus() const { return m_backupStatus; }
bool AppController::secureStorageEnabled() const { return m_secureStorageEnabled; }
QString AppController::mailboxUrl() const { return m_mailboxUrl; }
QString AppController::mailboxStatus() const { return m_mailboxStatus; }
QString AppController::microphoneTestStatus() const { return m_microphoneTestStatus; }
bool AppController::archivedVisible() const { return m_archivedVisible; }
bool AppController::callActive() const { return m_callActive; }
bool AppController::microphoneEnabled() const { return m_microphoneEnabled; }
bool AppController::cameraEnabled() const { return m_cameraEnabled; }
bool AppController::sharingScreen() const { return m_sharingScreen; }
bool AppController::incomingCallPending() const { return !m_pendingCallId.isEmpty(); }
bool AppController::incomingCallRinging() const { return m_pendingCallRinging; }
QString AppController::incomingCallContact() const { return m_pendingCallContact; }
QString AppController::callState() const { return m_callState; }
bool AppController::doNotDisturb() const { return m_doNotDisturb; }
QString AppController::voiceMode() const { return m_voiceMode; }
QString AppController::pushToTalkShortcut() const { return m_pushToTalkShortcut; }

bool AppController::platformSupportsAutostart() const
{
#if defined(Q_OS_WIN) || (defined(Q_OS_UNIX) && !defined(Q_OS_MACOS))
    return true;
#else
    return false;
#endif
}

bool AppController::autostartEnabled() const
{
#ifdef Q_OS_WIN
    QSettings run(QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                  QSettings::NativeFormat);
    return run.contains(QStringLiteral("pptalk"));
#elif defined(Q_OS_UNIX) && !defined(Q_OS_MACOS)
    const auto path = QStandardPaths::writableLocation(QStandardPaths::ConfigLocation) +
                      QStringLiteral("/autostart/pptalk.desktop");
    return QFileInfo::exists(path);
#else
    return false;
#endif
}

bool AppController::updateAvailable() const { return m_updateAvailable; }
QString AppController::updateVersion() const { return m_updateVersion; }

void AppController::downloadUpdate()
{
    if (m_updateUrl.isValid()) QDesktopServices::openUrl(m_updateUrl);
}

void AppController::selectConversation(const int index)
{
    if (index < 0 || index >= m_contacts.size()) return;
    const bool changed = index != m_selectedConversation;
    m_selectedConversation = index;
    auto selected = m_contacts[index].toMap();
    selected[QStringLiteral("unread")] = 0;
    m_contacts[index] = selected;
    auto &source = selected.value(QStringLiteral("group")).toBool() ? m_groups : m_directContacts;
    const auto keyName = selected.value(QStringLiteral("group")).toBool()
        ? QStringLiteral("groupId") : QStringLiteral("identityId");
    const auto key = selected.value(keyName).toString();
    QSettings().setValue(QStringLiteral("ui/lastConversation"), key);
    for (qsizetype sourceIndex = 0; sourceIndex < source.size(); ++sourceIndex) {
        auto item = source.at(sourceIndex).toMap();
        if (item.value(keyName).toString() != key) continue;
        item[QStringLiteral("unread")] = 0;
        source[sourceIndex] = item;
        break;
    }
    emit contactsChanged();
    emit conversationChanged();
    emit connectionChanged();
    sendBackendCommand({
        {QStringLiteral("command"), QStringLiteral("record_conversation_activity")},
        {QStringLiteral("conversation_key"), key},
        {QStringLiteral("summary"), QString()},
        {QStringLiteral("unread"), false},
        {QStringLiteral("clear_unread"), true},
    });
    if (!changed) return;
    m_messages.clear();
    emit messagesChanged();
    const auto selectedConversation = m_contacts.at(index).toMap();
    if (selectedConversation.value(QStringLiteral("group")).toBool()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_history")},
                            {QStringLiteral("group_id"), selectedConversation.value(QStringLiteral("groupId"))}});
    } else {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("history")},
                            {QStringLiteral("contact"), conversationName()}});
    }
}

void AppController::sendMessage(const QString &body)
{
    const auto trimmed = body.trimmed();
    if (trimmed.isEmpty()) {
        return;
    }
    if (m_backend != nullptr && m_backend->state() == QProcess::Running && !m_contacts.isEmpty()) {
        const auto selected = m_contacts.at(m_selectedConversation).toMap();
        if (selected.value(QStringLiteral("group")).toBool()) {
            sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_send")},
                                {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                                {QStringLiteral("message"), trimmed}});
        } else {
            sendBackendCommand({{QStringLiteral("command"), QStringLiteral("send")},
                                {QStringLiteral("contact"), conversationName()},
                                {QStringLiteral("message"), trimmed}});
        }
        return;
    }
}

void AppController::replyToMessage(const QString &messageId, const QString &body)
{
    const auto trimmed = body.trimmed();
    if (trimmed.isEmpty() || messageId.isEmpty() || m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    if (conversationIsGroup()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_send")},
                            {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                            {QStringLiteral("message"), trimmed},
                            {QStringLiteral("reply_to"), messageId}});
    } else {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("send")},
                            {QStringLiteral("contact"), conversationName()},
                            {QStringLiteral("message"), trimmed},
                            {QStringLiteral("reply_to"), messageId}});
    }
}

void AppController::editMessage(const QString &messageId, const QString &body)
{
    const auto trimmed = body.trimmed();
    if (trimmed.isEmpty() || messageId.isEmpty() || m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    sendBackendCommand({{QStringLiteral("command"), conversationIsGroup()
                            ? QStringLiteral("group_edit_message") : QStringLiteral("edit_message")},
                        {conversationIsGroup() ? QStringLiteral("group_id") : QStringLiteral("contact"),
                         conversationIsGroup() ? selected.value(QStringLiteral("groupId"))
                                               : QVariant(conversationName())},
                        {QStringLiteral("message_id"), messageId},
                        {QStringLiteral("message"), trimmed}});
}

void AppController::deleteMessage(const QString &messageId)
{
    if (messageId.isEmpty() || m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    sendBackendCommand({{QStringLiteral("command"), conversationIsGroup()
                            ? QStringLiteral("group_delete_message") : QStringLiteral("delete_message")},
                        {conversationIsGroup() ? QStringLiteral("group_id") : QStringLiteral("contact"),
                         conversationIsGroup() ? selected.value(QStringLiteral("groupId"))
                                               : QVariant(conversationName())},
                        {QStringLiteral("message_id"), messageId}});
}

void AppController::deleteMessageLocal(const QString &messageId)
{
    if (messageId.isEmpty() || m_contacts.isEmpty() || conversationIsGroup()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("delete_message_local")},
                        {QStringLiteral("contact"), conversationName()},
                        {QStringLiteral("message_id"), messageId}});
}

void AppController::search(const QString &query)
{
    const auto trimmed = query.trimmed();
    if (trimmed.size() < 2) {
        clearSearch();
        return;
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("search")},
                        {QStringLiteral("query"), trimmed}});
}

void AppController::clearSearch()
{
    if (m_searchResults.isEmpty()) return;
    m_searchResults.clear();
    emit searchResultsChanged();
}

void AppController::openSearchResult(const QString &conversationKey,
                                     const QString &messageId)
{
    for (qsizetype index = 0; index < m_contacts.size(); ++index) {
        const auto item = m_contacts.at(index).toMap();
        const auto key = item.value(QStringLiteral("group")).toBool()
            ? item.value(QStringLiteral("groupId")).toString()
            : item.value(QStringLiteral("identityId")).toString();
        if (key != conversationKey) continue;
        m_focusedMessageId = messageId;
        selectConversation(static_cast<int>(index));
        clearSearch();
        emit messagesChanged();
        QTimer::singleShot(3500, this, [this, messageId]() {
            if (m_focusedMessageId != messageId) return;
            m_focusedMessageId.clear();
            emit messagesChanged();
        });
        return;
    }
}

void AppController::openMessageFile(const QString &path)
{
    if (!path.isEmpty()) QDesktopServices::openUrl(QUrl::fromLocalFile(path));
}

void AppController::sendFile(const QUrl &file)
{
    if (!file.isLocalFile() || m_contacts.isEmpty()) {
        return;
    }
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    if (selected.value(QStringLiteral("group")).toBool()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_send_file")},
                            {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                            {QStringLiteral("path"), file.toLocalFile()}});
    } else {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("send_file")},
                            {QStringLiteral("contact"), conversationName()},
                            {QStringLiteral("path"), file.toLocalFile()}});
    }
}

void AppController::cancelTransfer(const QString &transferId)
{
    if (transferId.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("cancel_transfer")},
                        {QStringLiteral("transfer_id"), transferId}});
}

void AppController::initializeProfile(const QString &name)
{
    if (!m_onboardingRequired || QFileInfo::exists(m_profilePath)) return;
    const auto trimmed = name.trimmed();
    if (trimmed.isEmpty() || trimmed.size() > 64) {
        m_lastError = QStringLiteral("Escribe un nombre de entre 1 y 64 caracteres.");
        emit connectionChanged();
        return;
    }
    const auto exitCode = QProcess::execute(
        cliProgram(), {QStringLiteral("init"), QStringLiteral("--profile"), m_profilePath,
                       QStringLiteral("--name"), trimmed});
    if (exitCode != 0) {
        m_lastError = QStringLiteral("No se pudo crear la identidad local.");
        emit connectionChanged();
        return;
    }
    m_onboardingRequired = false;
    emit onboardingChanged();
    launchBackend();
}

void AppController::importDeviceLink(const QString &link)
{
    if (!m_onboardingRequired || QFileInfo::exists(m_profilePath)) return;
    const auto trimmed = link.trimmed();
    if (!trimmed.startsWith(QStringLiteral("pptalk://device/v1"))) {
        m_lastError = QStringLiteral("El enlace de dispositivo no es válido.");
        emit connectionChanged();
        return;
    }
    const auto exitCode = QProcess::execute(
        cliProgram(), {QStringLiteral("import-device"), QStringLiteral("--profile"),
                       m_profilePath, trimmed});
    if (exitCode != 0) {
        m_lastError = QStringLiteral("No se pudo vincular el dispositivo. Comprueba que el enlace no haya caducado.");
        emit connectionChanged();
        return;
    }
    m_onboardingRequired = false;
    m_onboardingLink.clear();
    emit onboardingChanged();
    launchBackend();
}

void AppController::exportIdentityBackup(const QUrl &file, const QString &passphrase)
{
    if (!file.isLocalFile() || passphrase.size() < 10 || m_backend == nullptr) {
        m_backupStatus = QStringLiteral("Elige un archivo y una frase de al menos 10 caracteres.");
        emit backupStatusChanged();
        return;
    }
    m_backupStatus = QStringLiteral("Creando copia cifrada…");
    emit backupStatusChanged();
    auto path = file.toLocalFile();
    if (!path.endsWith(QStringLiteral(".pptalk-backup"), Qt::CaseInsensitive)) {
        path += QStringLiteral(".pptalk-backup");
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("export_backup")},
                        {QStringLiteral("path"), path},
                        {QStringLiteral("passphrase"), passphrase}});
}

void AppController::restoreIdentityBackup(const QUrl &file, const QString &passphrase)
{
    if (!m_onboardingRequired || !file.isLocalFile() || passphrase.size() < 10) {
        m_lastError = QStringLiteral("Elige una copia y una frase de al menos 10 caracteres.");
        emit connectionChanged();
        return;
    }
    QProcess process;
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.insert(QStringLiteral("PPTALK_BACKUP_PASSPHRASE"), passphrase);
    process.setProcessEnvironment(environment);
    process.start(cliProgram(),
                  {QStringLiteral("import-backup"), QStringLiteral("--profile"), m_profilePath,
                   QStringLiteral("--input"), file.toLocalFile()});
    if (!process.waitForStarted(5000) || !process.waitForFinished(30000) ||
        process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) {
        m_lastError = QStringLiteral("No se pudo restaurar la copia. Revisa la frase o el archivo.");
        emit connectionChanged();
        return;
    }
    m_onboardingRequired = false;
    emit onboardingChanged();
    launchBackend();
}

void AppController::protectLocalSecrets()
{
    if (m_secureStorageEnabled) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("protect_local_secrets")}});
}

void AppController::setMailbox(const QString &url)
{
    const QString trimmed = url.trimmed();
    if (trimmed.isEmpty()) {
        clearMailbox();
        return;
    }
    m_mailboxStatus = QStringLiteral("Comprobando el buzón…");
    m_mailboxPending = true;
    emit settingsChanged();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_mailbox")},
                        {QStringLiteral("url"), trimmed}});
}

void AppController::clearMailbox()
{
    m_mailboxStatus = QStringLiteral("Quitando el buzón…");
    m_mailboxPending = true;
    emit settingsChanged();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_mailbox")},
                        {QStringLiteral("url"), QVariant()}});
}

void AppController::testMicrophone()
{
    m_microphoneTestStatus = QStringLiteral("Escuchando el micrófono…");
    emit settingsChanged();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("test_microphone")}});
}

QString AppController::draft() const
{
    const auto key = currentConversationKey();
    return key.isEmpty() ? QString() :
        QSettings().value(QStringLiteral("drafts/") + key).toString();
}

void AppController::saveDraft(const QString &body)
{
    const auto key = currentConversationKey();
    if (key.isEmpty()) return;
    QSettings settings;
    if (body.isEmpty()) settings.remove(QStringLiteral("drafts/") + key);
    else settings.setValue(QStringLiteral("drafts/") + key, body);
}

void AppController::createInvite()
{
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("invite")},
                        {QStringLiteral("expires_seconds"), 3600}});
}

void AppController::createGroup(const QString &name, const QString &members)
{
    QVariantList memberNames;
    for (const auto &member : members.split(QLatin1Char(','), Qt::SkipEmptyParts)) {
        const auto trimmed = member.trimmed();
        if (!trimmed.isEmpty()) {
            memberNames.append(trimmed);
        }
    }
    if (name.trimmed().isEmpty() || memberNames.isEmpty()) {
        return;
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("create_group")},
                        {QStringLiteral("name"), name.trimmed()},
                        {QStringLiteral("members"), memberNames}});
}

void AppController::configureVideoQuality(const int preset)
{
    m_manualVideoQuality = preset > 0;
    switch (preset) {
    case 1:
        m_videoWidth = 1280; m_videoHeight = 720; m_videoFramesPerSecond = 30;
        m_videoBitrateKbps = 2500;
        break;
    case 2:
        m_videoWidth = 1920; m_videoHeight = 1080; m_videoFramesPerSecond = 30;
        m_videoBitrateKbps = 5000;
        break;
    case 3:
        m_videoWidth = 2560; m_videoHeight = 1440; m_videoFramesPerSecond = 60;
        m_videoBitrateKbps = 12000;
        break;
    case 4:
        m_videoWidth = 3840; m_videoHeight = 2160; m_videoFramesPerSecond = 60;
        m_videoBitrateKbps = 30000;
        break;
    default:
        m_videoWidth = 1280; m_videoHeight = 720; m_videoFramesPerSecond = 30;
        m_videoBitrateKbps = 2500;
        break;
    }
}

void AppController::selectMediaDevice(const QString &kind, const QString &deviceId)
{
    if (kind != QStringLiteral("audio_input") &&
        kind != QStringLiteral("audio_output") &&
        kind != QStringLiteral("camera")) return;
    QSettings().setValue(QStringLiteral("media/") + kind, deviceId);
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("select_media_device")},
                        {QStringLiteral("kind"), kind},
                        {QStringLiteral("device_id"),
                         deviceId.isEmpty() ? QVariant() : QVariant(deviceId)}});
}

QString AppController::selectedMediaDevice(const QString &kind) const
{
    return QSettings().value(QStringLiteral("media/") + kind).toString();
}

void AppController::createDeviceLink(const QString &label)
{
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("link_device")},
                        {QStringLiteral("label"), label.trimmed().isEmpty()
                             ? QStringLiteral("Nuevo dispositivo") : label.trimmed()}});
}

void AppController::revokeDevice(const QString &deviceId)
{
    if (deviceId.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("revoke_device")},
                        {QStringLiteral("device_id"), deviceId},
                        {QStringLiteral("reason"), QStringLiteral("revocado desde el cliente")}});
}

void AppController::copyDeviceLink()
{
    if (!m_deviceLink.isEmpty()) QGuiApplication::clipboard()->setText(m_deviceLink);
}

void AppController::updateProfile(const QString &name, const QUrl &avatarFile)
{
    const auto trimmed = name.trimmed();
    if (trimmed.isEmpty()) return;
    QVariantMap command{{QStringLiteral("command"), QStringLiteral("update_profile")},
                        {QStringLiteral("name"), trimmed}};
    if (!avatarFile.isEmpty()) {
        QFile file(avatarFile.toLocalFile());
        if (!file.open(QIODevice::ReadOnly)) return;
        const auto bytes = file.read(512 * 1024 + 1);
        if (bytes.size() > 512 * 1024) return;
        const auto mime = QMimeDatabase().mimeTypeForFile(file.fileName()).name();
        command[QStringLiteral("avatar")] = QStringLiteral("data:%1;base64,%2")
            .arg(mime, QString::fromLatin1(bytes.toBase64()));
    }
    sendBackendCommand(command);
}

void AppController::clearProfileAvatar()
{
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("update_profile")},
                        {QStringLiteral("name"), m_profileName},
                        {QStringLiteral("avatar"), QVariant()}});
}

void AppController::addGroupMember(const QString &contact)
{
    if (m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    if (!selected.value(QStringLiteral("group")).toBool()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_add_member")},
                        {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                        {QStringLiteral("contact"), contact.trimmed()}});
}

void AppController::removeGroupMember(const QString &contact)
{
    if (m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    if (!selected.value(QStringLiteral("group")).toBool()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_remove_member")},
                        {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                        {QStringLiteral("contact"), contact.trimmed()}});
}

void AppController::setGroupAdministrator(const QString &contact, const bool administrator)
{
    if (!currentGroupOwned() || contact.trimmed().isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_set_admin")},
                        {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                        {QStringLiteral("contact"), contact.trimmed()},
                        {QStringLiteral("admin"), administrator}});
}

void AppController::transferGroupOwnership(const QString &contact)
{
    if (!currentGroupOwned() || contact.trimmed().isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_transfer_ownership")},
                        {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))},
                        {QStringLiteral("contact"), contact.trimmed()}});
}

void AppController::dissolveCurrentGroup()
{
    if (!currentGroupOwned()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("group_dissolve")},
                        {QStringLiteral("group_id"), selected.value(QStringLiteral("groupId"))}});
}

void AppController::acceptInvite(const QString &url)
{
    if (url.trimmed().isEmpty()) {
        return;
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("preview_invite")},
                        {QStringLiteral("url"), url.trimmed()}});
}

void AppController::confirmInvite()
{
    if (m_invitePreviewUrl.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("accept")},
                        {QStringLiteral("url"), m_invitePreviewUrl}});
    m_invitePreviewUrl.clear();
    m_invitePreviewName.clear();
    m_invitePreviewExpiry.clear();
    emit invitePreviewChanged();
}

void AppController::copyInvite()
{
    if (!m_inviteLink.isEmpty()) {
        QGuiApplication::clipboard()->setText(m_inviteLink);
    }
}

void AppController::startCall(const bool ringEveryone)
{
    if (m_contacts.isEmpty()) {
        return;
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("start_call")},
                        {QStringLiteral("contact"), conversationName()},
                        {QStringLiteral("ring"), ringEveryone}});
}

void AppController::leaveCall()
{
    if (!m_callId.isEmpty() && !m_contacts.isEmpty()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("leave_call")},
                            {QStringLiteral("contact"), conversationName()},
                            {QStringLiteral("call_id"), m_callId},
                            {QStringLiteral("missed"), false}});
    }
    m_callActive = false;
    m_callId.clear();
    m_microphoneEnabled = false;
    m_cameraEnabled = false;
    m_sharingScreen = false;
    m_callParticipants.clear();
    emit callChanged();
}

void AppController::toggleMicrophone()
{
    setMedia(QStringLiteral("voice"), !m_microphoneEnabled);
}

void AppController::toggleCamera()
{
    setMedia(QStringLiteral("camera"), !m_cameraEnabled);
}

void AppController::toggleScreenShare()
{
    setMedia(QStringLiteral("screen"), !m_sharingScreen);
}

void AppController::setParticipantVolume(const QString &deviceId, const double volume)
{
    if (m_callId.isEmpty() || deviceId.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_participant_volume")},
                        {QStringLiteral("call_id"), m_callId},
                        {QStringLiteral("device_id"), deviceId},
                        {QStringLiteral("volume"), qBound(0.0, volume, 2.0)}});
}

void AppController::removeCurrentContact()
{
    if (m_contacts.isEmpty() || conversationIsGroup()) return;
    const auto identity = m_contacts.at(m_selectedConversation).toMap().value(QStringLiteral("identityId")).toString();
    if (!identity.isEmpty()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("remove_contact")},
                            {QStringLiteral("identity_id"), identity}});
    }
}

void AppController::setCurrentContactBlocked(const bool blocked)
{
    if (m_contacts.isEmpty() || conversationIsGroup()) return;
    const auto identity = m_contacts.at(m_selectedConversation).toMap().value(QStringLiteral("identityId")).toString();
    if (!identity.isEmpty()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_contact_blocked")},
                            {QStringLiteral("identity_id"), identity},
                            {QStringLiteral("blocked"), blocked}});
    }
}

void AppController::setCurrentContactPrivacy(const bool hidden)
{
    if (m_contacts.isEmpty() || conversationIsGroup()) return;
    const auto identity = m_contacts.at(m_selectedConversation).toMap().value(QStringLiteral("identityId")).toString();
    if (!identity.isEmpty()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_contact_privacy")},
                            {QStringLiteral("identity_id"), identity},
                            {QStringLiteral("hide_presence"), hidden}});
    }
}

void AppController::setCurrentContactVerified(const bool verified)
{
    if (m_contacts.isEmpty() || conversationIsGroup()) return;
    const auto identity = m_contacts.at(m_selectedConversation).toMap()
        .value(QStringLiteral("identityId")).toString();
    if (identity.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_contact_verified")},
                        {QStringLiteral("identity_id"), identity},
                        {QStringLiteral("verified"), verified}});
}

void AppController::setCurrentConversationPreferences(const bool pinned, const bool archived,
                                                      const bool muted)
{
    if (m_contacts.isEmpty()) return;
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    const auto key = selected.value(selected.value(QStringLiteral("group")).toBool()
                                        ? QStringLiteral("groupId")
                                        : QStringLiteral("identityId")).toString();
    if (key.isEmpty()) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_conversation_preference")},
                        {QStringLiteral("conversation_key"), key},
                        {QStringLiteral("pinned"), pinned},
                        {QStringLiteral("archived"), archived},
                        {QStringLiteral("muted"), muted}});
}

void AppController::holdCall()
{
    if (m_callId.isEmpty() || m_callState != QStringLiteral("connected")) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("hold_call")},
                        {QStringLiteral("call_id"), m_callId}});
}

void AppController::resumeHeldCall()
{
    if (m_heldCallId.isEmpty() || m_callActive) return;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("resume_call")},
                        {QStringLiteral("call_id"), m_heldCallId}});
}

void AppController::setDoNotDisturb(const bool enabled)
{
    if (m_doNotDisturb == enabled) return;
    m_doNotDisturb = enabled;
    QSettings().setValue(QStringLiteral("notifications/doNotDisturb"), enabled);
    emit settingsChanged();
}

void AppController::setArchivedVisible(const bool visible)
{
    if (m_archivedVisible == visible) return;
    m_archivedVisible = visible;
    QSettings().setValue(QStringLiteral("conversations/showArchived"), visible);
    rebuildConversations();
    emit settingsChanged();
}

void AppController::dismissError()
{
    if (m_lastError.isEmpty()) return;
    m_lastError.clear();
    emit connectionChanged();
}

void AppController::handleExternalLink(const QString &link)
{
    const auto trimmed = link.trimmed();
    if (trimmed.startsWith(QStringLiteral("pptalk://contact/"))) {
        if (m_onboardingRequired) {
            m_startupInvite = trimmed;
        } else {
            acceptInvite(trimmed);
        }
        return;
    }
    if (trimmed.startsWith(QStringLiteral("pptalk://device/v1"))) {
        if (m_onboardingRequired) {
            m_onboardingLink = trimmed;
            emit onboardingChanged();
        } else {
            m_lastError = QStringLiteral(
                "Este equipo ya tiene una identidad. Un enlace de dispositivo solo puede abrirse en una instalación nueva.");
            emit connectionChanged();
        }
    }
}

void AppController::setVoiceMode(const QString &mode)
{
    if (mode != QStringLiteral("open") && mode != QStringLiteral("push_to_talk")) return;
    if (m_voiceMode == mode) return;
    m_voiceMode = mode;
    QSettings().setValue(QStringLiteral("calls/voiceMode"), mode);
    if (m_callActive) {
        setMedia(QStringLiteral("voice"), mode != QStringLiteral("push_to_talk"));
    }
    emit settingsChanged();
}

void AppController::setPushToTalkShortcut(const QString &shortcut)
{
    if (shortcut != QStringLiteral("Ctrl+Space") &&
        shortcut != QStringLiteral("Alt+Space") &&
        shortcut != QStringLiteral("F8")) return;
    if (m_pushToTalkShortcut == shortcut) return;
    m_pushToTalkShortcut = shortcut;
    QSettings().setValue(QStringLiteral("calls/pushToTalkShortcut"), shortcut);
    emit settingsChanged();
}

bool AppController::eventFilter(QObject *watched, QEvent *event)
{
    Q_UNUSED(watched);
    if (!m_callActive || m_voiceMode != QStringLiteral("push_to_talk")) return false;
    if (event->type() == QEvent::KeyPress) {
        const auto *key = static_cast<QKeyEvent *>(event);
        const auto matches = m_pushToTalkShortcut == QStringLiteral("F8")
            ? key->key() == Qt::Key_F8
            : key->key() == Qt::Key_Space &&
                key->modifiers().testFlag(
                    m_pushToTalkShortcut == QStringLiteral("Alt+Space")
                        ? Qt::AltModifier : Qt::ControlModifier);
        if (matches &&
            !key->isAutoRepeat() && !m_pushToTalkPressed) {
            m_pushToTalkPressed = true;
            setMedia(QStringLiteral("voice"), true);
            return true;
        }
    } else if (event->type() == QEvent::KeyRelease) {
        const auto *key = static_cast<QKeyEvent *>(event);
        const auto released = m_pushToTalkShortcut == QStringLiteral("F8")
            ? key->key() == Qt::Key_F8 : key->key() == Qt::Key_Space;
        if (released && !key->isAutoRepeat() && m_pushToTalkPressed) {
            m_pushToTalkPressed = false;
            setMedia(QStringLiteral("voice"), false);
            return true;
        }
    }
    return false;
}

void AppController::setAutostartEnabled(const bool enabled)
{
#ifdef Q_OS_WIN
    QSettings run(QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                  QSettings::NativeFormat);
    if (enabled) {
        run.setValue(QStringLiteral("pptalk"), QStringLiteral("\"") +
                     QCoreApplication::applicationFilePath() +
                     QStringLiteral("\" --minimized"));
    } else {
        run.remove(QStringLiteral("pptalk"));
    }
    run.sync();
    emit settingsChanged();
#elif defined(Q_OS_UNIX) && !defined(Q_OS_MACOS)
    const auto directory = QStandardPaths::writableLocation(QStandardPaths::ConfigLocation) +
                           QStringLiteral("/autostart");
    const auto path = directory + QStringLiteral("/pptalk.desktop");
    if (enabled) {
        QDir().mkpath(directory);
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text)) {
            m_lastError = QStringLiteral("No se pudo activar el inicio automático.");
            emit connectionChanged();
            return;
        }
        const auto executable = QCoreApplication::applicationFilePath();
        const auto contents = QStringLiteral(
            "[Desktop Entry]\nType=Application\nName=pptalk\nExec=\"%1\" --minimized\n"
            "Icon=pptalk\nTerminal=false\nX-GNOME-Autostart-enabled=true\n")
            .arg(executable);
        file.write(contents.toUtf8());
        file.close();
        QFile::setPermissions(path, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    } else {
        QFile::remove(path);
    }
    emit settingsChanged();
#else
    Q_UNUSED(enabled);
#endif
}

void AppController::acceptIncomingCall()
{
    if (m_pendingCallId.isEmpty() || m_pendingCallContact.isEmpty()) {
        return;
    }
    for (qsizetype index = 0; index < m_contacts.size(); ++index) {
        if (m_contacts.at(index).toMap().value(QStringLiteral("name")).toString() ==
            m_pendingCallContact) {
            selectConversation(static_cast<int>(index));
            break;
        }
    }
    m_callId = m_pendingCallId;
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("join_call")},
                        {QStringLiteral("contact"), m_pendingCallContact},
                        {QStringLiteral("call_id"), m_pendingCallId}});
    m_pendingCallId.clear();
    m_pendingCallContact.clear();
    m_pendingCallRinging = false;
    emit callChanged();
}

void AppController::declineIncomingCall()
{
    if (!m_pendingCallId.isEmpty() && !m_pendingCallContact.isEmpty()) {
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("reject_call")},
                            {QStringLiteral("contact"), m_pendingCallContact},
                            {QStringLiteral("call_id"), m_pendingCallId},
                            {QStringLiteral("missed"), false}});
    }
    m_pendingCallId.clear();
    m_pendingCallContact.clear();
    m_pendingCallRinging = false;
    emit callChanged();
}

void AppController::setMedia(const QString &kind, const bool enabled)
{
    if (!m_callActive || m_callId.isEmpty() || m_contacts.isEmpty()) {
        return;
    }
    QVariantMap command{{QStringLiteral("command"), QStringLiteral("set_media")},
                        {QStringLiteral("contact"), conversationName()},
                        {QStringLiteral("call_id"), m_callId},
                        {QStringLiteral("kind"), kind},
                        {QStringLiteral("enabled"), enabled}};
    if (enabled && m_manualVideoQuality &&
        (kind == QStringLiteral("camera") || kind == QStringLiteral("screen"))) {
        command[QStringLiteral("profile")] = QVariantMap{
            {QStringLiteral("mode"), QStringLiteral("Manual")},
            {QStringLiteral("width"), m_videoWidth},
            {QStringLiteral("height"), m_videoHeight},
            {QStringLiteral("frames_per_second"), m_videoFramesPerSecond},
            {QStringLiteral("bitrate_kbps"), m_videoBitrateKbps},
            {QStringLiteral("codec"), QStringLiteral("h264")},
        };
    }
    sendBackendCommand(command);
}

void AppController::startBackend()
{
    const auto dataDirectory = QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
    QDir().mkpath(dataDirectory);
    m_profilePath = qEnvironmentVariable("PPTALK_PROFILE");
    if (m_profilePath.isEmpty()) {
        m_profilePath = dataDirectory + QStringLiteral("/profile.json");
    }
    for (const auto &argument : QCoreApplication::arguments()) {
        if (argument.startsWith(QStringLiteral("pptalk://contact/"))) {
            m_startupInvite = argument;
            break;
        }
        if (argument.startsWith(QStringLiteral("pptalk://device/v1"))) {
            m_onboardingLink = argument;
            break;
        }
    }

    if (!QFileInfo::exists(m_profilePath)) {
        const auto deviceLink = qEnvironmentVariable("PPTALK_DEVICE_LINK");
        if (m_onboardingLink.isEmpty() && !deviceLink.isEmpty()) m_onboardingLink = deviceLink;
        m_onboardingRequired = true;
        emit onboardingChanged();
        return;
    }
    launchBackend();
}

QString AppController::cliProgram() const
{
    auto program = qEnvironmentVariable("PPTALK_CLI");
    if (program.isEmpty()) {
        program = QCoreApplication::applicationDirPath() + QStringLiteral("/pptalk-cli");
#ifdef Q_OS_WIN
        program += QStringLiteral(".exe");
#endif
    }
    if (!QFileInfo::exists(program)) program = QStringLiteral("pptalk-cli");
    return program;
}

void AppController::launchBackend()
{
    if (m_backend != nullptr) return;
    m_backend = new QProcess(this);
#ifdef Q_OS_WIN
    auto environment = QProcessEnvironment::systemEnvironment();
    const auto applicationDir = QCoreApplication::applicationDirPath();
    const auto bundledPluginDirectory =
        applicationDir + QStringLiteral("/lib/gstreamer-1.0");
    const auto bundledPluginScanner =
        applicationDir + QStringLiteral("/libexec/gstreamer-1.0/gst-plugin-scanner.exe");
    if (QDir(bundledPluginDirectory).exists()) {
        environment.insert(QStringLiteral("GST_PLUGIN_SYSTEM_PATH_1_0"),
                           bundledPluginDirectory);
    }
    if (QFileInfo::exists(bundledPluginScanner)) {
        environment.insert(QStringLiteral("GST_PLUGIN_SCANNER"), bundledPluginScanner);
    }
    m_backend->setProcessEnvironment(environment);
#endif
    m_backend->setProcessChannelMode(QProcess::SeparateChannels);
    connect(m_backend, &QProcess::readyReadStandardOutput, this,
            &AppController::processBackendOutput);
    connect(m_backend, &QProcess::readyReadStandardError, this, [this]() {
        const auto diagnostic = QString::fromUtf8(m_backend->readAllStandardError()).trimmed();
        if (!diagnostic.isEmpty()) {
            m_lastError = diagnostic.section(QLatin1Char('\n'), -1);
            emit connectionChanged();
        }
    });
    connect(m_backend, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
        m_lastError = m_backend->errorString();
        emit connectionChanged();
    });
    connect(m_backend, qOverload<int, QProcess::ExitStatus>(&QProcess::finished), this,
            [this](int, QProcess::ExitStatus) {
        auto *finished = m_backend;
        m_backend = nullptr;
        if (finished != nullptr) finished->deleteLater();
        if (m_shuttingDown || m_onboardingRequired) return;
        const auto delay = std::min(10'000, 500 * (1 << std::min(m_backendRestartAttempts, 4)));
        ++m_backendRestartAttempts;
        m_lastError = QStringLiteral("El servicio local se ha detenido. Reconectando…");
        emit connectionChanged();
        QTimer::singleShot(delay, this, [this]() {
            if (!m_shuttingDown && m_backend == nullptr) launchBackend();
        });
    });
    m_backend->start(cliProgram(),
                     {QStringLiteral("daemon"), QStringLiteral("--profile"), m_profilePath});
}

QString AppController::currentConversationKey() const
{
    if (m_contacts.isEmpty() || m_selectedConversation < 0 ||
        m_selectedConversation >= m_contacts.size()) return {};
    const auto selected = m_contacts.at(m_selectedConversation).toMap();
    return selected.value(selected.value(QStringLiteral("group")).toBool()
                              ? QStringLiteral("groupId")
                              : QStringLiteral("identityId")).toString();
}

void AppController::processBackendOutput()
{
    m_backendBuffer.append(m_backend->readAllStandardOutput());
    while (true) {
        const auto newline = m_backendBuffer.indexOf('\n');
        if (newline < 0) {
            break;
        }
        const auto line = m_backendBuffer.left(newline);
        m_backendBuffer.remove(0, newline + 1);
        QJsonParseError parseError;
        const auto document = QJsonDocument::fromJson(line, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
            continue;
        }
        const auto object = document.object();
        const auto event = object.value(QStringLiteral("event")).toString();
        if (event == QStringLiteral("contacts")) {
            QVariantList contacts;
            const auto values = object.value(QStringLiteral("contacts")).toArray();
            const QStringList accents = {QStringLiteral("#8B7CFF"), QStringLiteral("#FF8DA1"),
                                         QStringLiteral("#60D6A7"), QStringLiteral("#68B7F5")};
            for (qsizetype index = 0; index < values.size(); ++index) {
                const auto value = values.at(index).toObject();
                const auto deviceCount = value.value(QStringLiteral("device_count")).toInt(1);
                auto item = contact(value.value(QStringLiteral("name")).toString(),
                                        value.value(QStringLiteral("verified")).toBool()
                                            ? QStringLiteral("contacto verificado · %1 dispositivo%2")
                                                  .arg(deviceCount)
                                                  .arg(deviceCount == 1 ? QString() : QStringLiteral("s"))
                                            : QStringLiteral("verificación pendiente"),
                                        QStringLiteral("P2P · disponible al conectar"),
                                        accents.at(index % accents.size()), 0,
                                        value.value(QStringLiteral("identity_id")).toString(),
                                        value.value(QStringLiteral("blocked")).toBool(),
                                        value.value(QStringLiteral("hide_presence")).toBool(),
                                        value.value(QStringLiteral("avatar")).toString(),
                                        value.value(QStringLiteral("fingerprint")).toString(),
                                        value.value(QStringLiteral("manually_verified")).toBool());
                applyConversationPreference(
                    item, m_conversationPreferences.value(
                        value.value(QStringLiteral("identity_id")).toString()).toMap());
                contacts.append(item);
            }
            m_directContacts = contacts;
            rebuildConversations();
            if (m_selectedConversation >= m_contacts.size()) {
                m_selectedConversation = 0;
            }
            emit contactsChanged();
            emit conversationChanged();
            if (!m_contacts.isEmpty()) {
                sendBackendCommand({{QStringLiteral("command"), QStringLiteral("history")},
                                    {QStringLiteral("contact"), conversationName()}});
            }
        } else if (event == QStringLiteral("groups")) {
            QVariantList groups;
            for (const auto &value : object.value(QStringLiteral("groups")).toArray()) {
                const auto item = value.toObject();
                auto groupItem = group(item.value(QStringLiteral("name")).toString(),
                                    item.value(QStringLiteral("id")).toString(),
                                    item.value(QStringLiteral("member_count")).toInt(),
                                    item.value(QStringLiteral("owned")).toBool(),
                                    item.value(QStringLiteral("admin")).toBool());
                applyConversationPreference(
                    groupItem, m_conversationPreferences.value(
                        item.value(QStringLiteral("id")).toString()).toMap());
                groups.append(groupItem);
            }
            m_groups = groups;
            rebuildConversations();
        } else if (event == QStringLiteral("history") ||
                   event == QStringLiteral("group_history")) {
            const auto matchesSelection = event == QStringLiteral("history")
                ? isSelectedDirect(object.value(QStringLiteral("contact")).toString())
                : isSelectedGroup(object.value(QStringLiteral("group_id")).toString());
            if (!matchesSelection) {
                continue;
            }
            QVariantList messages;
            const auto values = object.value(QStringLiteral("messages")).toArray();
            for (const auto &value : values) {
                const auto item = value.toObject();
                const auto timestamp = QDateTime::fromSecsSinceEpoch(
                    item.value(QStringLiteral("sent_at")).toInteger());
                messages.append(message(item.value(QStringLiteral("author")).toString(),
                                        item.value(QStringLiteral("deleted")).toBool()
                                            ? QStringLiteral("Mensaje eliminado")
                                            : item.value(QStringLiteral("body")).toString(),
                                        timestamp.toString(QStringLiteral("HH:mm")),
                                        item.value(QStringLiteral("outgoing")).toBool(),
                                        item.value(QStringLiteral("message_id")).toString(),
                                        item.value(QStringLiteral("delivery")).toString(),
                                        item.value(QStringLiteral("edited")).toBool(),
                                        item.value(QStringLiteral("deleted")).toBool(),
                                        item.value(QStringLiteral("reply_to")).toString(),
                                        item.value(QStringLiteral("file_path")).toString()));
            }
            m_messages = messages;
            emit messagesChanged();
        } else if (event == QStringLiteral("message") || event == QStringLiteral("message_sent")) {
            const auto own = event == QStringLiteral("message_sent");
            const auto contactName = object.value(own ? QStringLiteral("to")
                                                      : QStringLiteral("from")).toString();
            const auto body = object.value(QStringLiteral("body")).toString();
            if (isSelectedDirect(contactName)) {
                m_messages.append(message(own ? QStringLiteral("Tú") : contactName, body,
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), own,
                                          object.value(QStringLiteral("message_id")).toString(),
                                          object.value(QStringLiteral("delivery")).toString(), false, false,
                                          object.value(QStringLiteral("reply_to")).toString()));
                emit messagesChanged();
            }
            recordActivity(false, contactName, body,
                           !own && (!isSelectedDirect(contactName) ||
                                    QGuiApplication::applicationState() != Qt::ApplicationActive));
        } else if (event == QStringLiteral("call_history")) {
            const auto conversation = object.value(QStringLiteral("conversation")).toString();
            if (conversationName().compare(conversation, Qt::CaseInsensitive) != 0) continue;
            for (const auto &value : object.value(QStringLiteral("calls")).toArray()) {
                const auto call = value.toObject();
                const auto outcome = call.value(QStringLiteral("outcome")).toString();
                QString label;
                if (outcome == QStringLiteral("missed")) label = QStringLiteral("Llamada perdida");
                else if (outcome == QStringLiteral("rejected")) label = QStringLiteral("Llamada rechazada");
                else if (outcome == QStringLiteral("answered") || outcome == QStringLiteral("ended"))
                    label = QStringLiteral("Llamada finalizada");
                else if (outcome == QStringLiteral("room") || outcome == QStringLiteral("joined"))
                    label = QStringLiteral("Sala de voz");
                else label = QStringLiteral("Llamada");
                const auto timestamp = QDateTime::fromSecsSinceEpoch(
                    call.value(QStringLiteral("started_at")).toInteger());
                m_messages.append(message(QStringLiteral("pptalk"), label,
                                          timestamp.toString(QStringLiteral("HH:mm")), false));
            }
            emit messagesChanged();
        } else if (event == QStringLiteral("group_message")) {
            const auto groupId = object.value(QStringLiteral("group_id")).toString();
            const auto outgoing = object.value(QStringLiteral("outgoing")).toBool();
            const auto body = object.value(QStringLiteral("body")).toString();
            if (isSelectedGroup(groupId)) {
                m_messages.append(message(outgoing ? QStringLiteral("Tú")
                                                   : object.value(QStringLiteral("author")).toString(),
                                          body,
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), outgoing,
                                          object.value(QStringLiteral("message_id")).toString(),
                                          object.value(QStringLiteral("delivery")).toString(), false, false,
                                          object.value(QStringLiteral("reply_to")).toString()));
                emit messagesChanged();
            }
            recordActivity(true, groupId, body,
                           !outgoing && (!isSelectedGroup(groupId) ||
                                         QGuiApplication::applicationState() != Qt::ApplicationActive));
        } else if (event == QStringLiteral("message_edited") ||
                   event == QStringLiteral("group_message_edited")) {
            const auto id = object.value(QStringLiteral("message_id")).toString();
            for (qsizetype index = 0; index < m_messages.size(); ++index) {
                auto item = m_messages.at(index).toMap();
                if (item.value(QStringLiteral("messageId")).toString() != id) continue;
                item[QStringLiteral("body")] = object.value(QStringLiteral("body")).toString();
                item[QStringLiteral("edited")] = true;
                m_messages[index] = item;
                emit messagesChanged();
                break;
            }
        } else if (event == QStringLiteral("message_deleted") ||
                   event == QStringLiteral("group_message_deleted")) {
            const auto id = object.value(QStringLiteral("message_id")).toString();
            for (qsizetype index = 0; index < m_messages.size(); ++index) {
                auto item = m_messages.at(index).toMap();
                if (item.value(QStringLiteral("messageId")).toString() != id) continue;
                item[QStringLiteral("body")] = QStringLiteral("Mensaje eliminado");
                item[QStringLiteral("deleted")] = true;
                m_messages[index] = item;
                emit messagesChanged();
                break;
            }
        } else if (event == QStringLiteral("message_delivered")) {
            const auto id = object.value(QStringLiteral("message_id")).toString();
            for (qsizetype index = 0; index < m_messages.size(); ++index) {
                auto item = m_messages.at(index).toMap();
                if (item.value(QStringLiteral("messageId")).toString() != id) continue;
                item[QStringLiteral("delivery")] = QStringLiteral("delivered");
                m_messages[index] = item;
                emit messagesChanged();
                break;
            }
        } else if (event == QStringLiteral("search_results")) {
            QVariantList results;
            for (const auto &value : object.value(QStringLiteral("results")).toArray()) {
                const auto item = value.toObject();
                results.append(QVariantMap{
                    {QStringLiteral("messageId"), item.value(QStringLiteral("message_id")).toString()},
                    {QStringLiteral("conversationKey"), item.value(QStringLiteral("conversation_key")).toString()},
                    {QStringLiteral("author"), item.value(QStringLiteral("author")).toString()},
                    {QStringLiteral("body"), item.value(QStringLiteral("body")).toString()},
                    {QStringLiteral("sentAt"), item.value(QStringLiteral("sent_at")).toInteger()},
                });
            }
            m_searchResults = results;
            emit searchResultsChanged();
        } else if (event == QStringLiteral("conversation_settings")) {
            const auto settings = object.value(QStringLiteral("settings")).toArray();
            m_conversationPreferences.clear();
            for (const auto &value : settings) {
                const auto setting = value.toObject();
                m_conversationPreferences.insert(
                    setting.value(QStringLiteral("conversation_key")).toString(),
                    setting.toVariantMap());
            }
            auto applySettings = [&settings](QVariantList &items, const QString &keyName) {
                for (qsizetype index = 0; index < items.size(); ++index) {
                    auto item = items.at(index).toMap();
                    const auto key = item.value(keyName).toString();
                    for (const auto &value : settings) {
                        const auto setting = value.toObject();
                        if (setting.value(QStringLiteral("conversation_key")).toString() != key) continue;
                        item[QStringLiteral("pinned")] = setting.value(QStringLiteral("pinned")).toBool();
                        item[QStringLiteral("archived")] = setting.value(QStringLiteral("archived")).toBool();
                        item[QStringLiteral("muted")] = setting.value(QStringLiteral("muted")).toBool();
                        item[QStringLiteral("unread")] = setting.value(QStringLiteral("unread")).toInt();
                        const auto summary = setting.value(QStringLiteral("last_summary")).toString();
                        if (!summary.isEmpty()) item[QStringLiteral("summary")] = summary;
                        item[QStringLiteral("lastActivity")] =
                            setting.value(QStringLiteral("last_activity")).toInteger();
                        break;
                    }
                    items[index] = item;
                }
            };
            applySettings(m_directContacts, QStringLiteral("identityId"));
            applySettings(m_groups, QStringLiteral("groupId"));
            rebuildConversations();
        } else if (event == QStringLiteral("transfer_progress")) {
            const auto transferId = object.value(QStringLiteral("transfer_id")).toString();
            const auto total = object.value(QStringLiteral("total_chunks")).toInteger();
            const auto sent = object.value(QStringLiteral("sent_chunks")).toInteger();
            QVariantMap transfer{
                {QStringLiteral("id"), transferId},
                {QStringLiteral("fileName"),
                 object.value(QStringLiteral("file_name")).toString()},
                {QStringLiteral("progress"),
                 total > 0 ? static_cast<double>(sent) / static_cast<double>(total) : 0.0},
                {QStringLiteral("cancelable"),
                 object.value(QStringLiteral("cancelable")).toBool()},
            };
            bool updated = false;
            for (qsizetype index = 0; index < m_transfers.size(); ++index) {
                if (m_transfers.at(index).toMap().value(QStringLiteral("id")).toString() !=
                    transferId) continue;
                m_transfers[index] = transfer;
                updated = true;
                break;
            }
            if (!updated) m_transfers.append(transfer);
            emit transfersChanged();
            if (sent >= total && total > 0) {
                QTimer::singleShot(1200, this, [this, transferId]() {
                    for (qsizetype index = m_transfers.size(); index-- > 0;) {
                        if (m_transfers.at(index).toMap()
                                .value(QStringLiteral("id")).toString() == transferId) {
                            m_transfers.removeAt(index);
                        }
                    }
                    emit transfersChanged();
                });
            }
        } else if (event == QStringLiteral("transfer_cancelled")) {
            const auto transferId = object.value(QStringLiteral("transfer_id")).toString();
            for (qsizetype index = m_transfers.size(); index-- > 0;) {
                if (m_transfers.at(index).toMap().value(QStringLiteral("id")).toString() ==
                    transferId) {
                    m_transfers.removeAt(index);
                }
            }
            emit transfersChanged();
        } else if (event == QStringLiteral("file_received") ||
                   event == QStringLiteral("file_sent")) {
            const auto own = event == QStringLiteral("file_sent");
            const auto contactName = object.value(own ? QStringLiteral("to")
                                                      : QStringLiteral("from")).toString();
            const auto body = QStringLiteral("📎 ") + object.value(QStringLiteral("file_name")).toString();
            if (isSelectedDirect(contactName)) {
                m_messages.append(message(own ? QStringLiteral("Tú") : contactName, body,
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), own));
                emit messagesChanged();
            }
            recordActivity(false, contactName, body,
                           !own && (!isSelectedDirect(contactName) ||
                                    QGuiApplication::applicationState() != Qt::ApplicationActive));
        } else if (event == QStringLiteral("group_file_received") ||
                   event == QStringLiteral("group_file_sent")) {
            const auto own = event == QStringLiteral("group_file_sent");
            const auto groupId = object.value(QStringLiteral("group_id")).toString();
            const auto body = QStringLiteral("📎 ") + object.value(QStringLiteral("file_name")).toString();
            if (isSelectedGroup(groupId)) {
                m_messages.append(message(own ? QStringLiteral("Tú")
                                              : object.value(QStringLiteral("from")).toString(),
                                          body,
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), own));
                emit messagesChanged();
            }
            recordActivity(true, groupId, body,
                           !own && (!isSelectedGroup(groupId) ||
                                    QGuiApplication::applicationState() != Qt::ApplicationActive));
        } else if (event == QStringLiteral("invite")) {
            m_inviteLink = object.value(QStringLiteral("url")).toString();
            m_inviteQr = QStringLiteral("data:image/svg+xml;base64,") +
                QString::fromLatin1(
                    object.value(QStringLiteral("qr_svg")).toString().toUtf8().toBase64());
            emit inviteLinkChanged();
        } else if (event == QStringLiteral("invite_preview")) {
            m_invitePreviewUrl = object.value(QStringLiteral("url")).toString();
            m_invitePreviewName = object.value(QStringLiteral("name")).toString();
            const auto expiry = QDateTime::fromSecsSinceEpoch(
                object.value(QStringLiteral("expires_unix")).toInteger());
            m_invitePreviewExpiry = expiry.toString(QStringLiteral("dd/MM/yyyy HH:mm"));
            emit invitePreviewChanged();
        } else if (event == QStringLiteral("device_link")) {
            m_deviceLink = object.value(QStringLiteral("url")).toString();
            emit deviceLinkChanged();
        } else if (event == QStringLiteral("devices")) {
            QVariantList devices;
            for (const auto &value : object.value(QStringLiteral("devices")).toArray()) {
                const auto item = value.toObject();
                devices.append(QVariantMap{
                    {QStringLiteral("id"), item.value(QStringLiteral("id")).toString()},
                    {QStringLiteral("label"), item.value(QStringLiteral("label")).toString()},
                    {QStringLiteral("active"), item.value(QStringLiteral("active")).toBool()},
                    {QStringLiteral("current"), item.value(QStringLiteral("current")).toBool()},
                });
            }
            m_devices = devices;
            emit devicesChanged();
        } else if (event == QStringLiteral("media_capabilities")) {
            QVariantList devices;
            for (const auto &value : object.value(QStringLiteral("devices")).toArray()) {
                const auto item = value.toObject();
                devices.append(QVariantMap{
                    {QStringLiteral("id"), item.value(QStringLiteral("id")).toString()},
                    {QStringLiteral("label"), item.value(QStringLiteral("label")).toString()},
                    {QStringLiteral("kind"), item.value(QStringLiteral("kind")).toString()},
                    {QStringLiteral("default"), item.value(QStringLiteral("default")).toBool()},
                });
            }
            m_mediaDevices = devices;
            emit mediaDevicesChanged();
            for (const auto &kind : {QStringLiteral("audio_input"),
                                     QStringLiteral("audio_output"),
                                     QStringLiteral("camera")}) {
                const auto selected = selectedMediaDevice(kind);
                if (!selected.isEmpty()) selectMediaDevice(kind, selected);
            }
        } else if (event == QStringLiteral("backup_exported")) {
            m_backupStatus = QStringLiteral("Copia cifrada guardada en %1")
                .arg(object.value(QStringLiteral("path")).toString());
            emit backupStatusChanged();
        } else if (event == QStringLiteral("secure_storage")) {
            m_secureStorageEnabled = object.value(QStringLiteral("enabled")).toBool();
            emit settingsChanged();
        } else if (event == QStringLiteral("mailbox_configured")) {
            m_mailboxPending = false;
            m_mailboxUrl = object.value(QStringLiteral("mailbox_url")).toString();
            if (m_mailboxUrl.isEmpty()) {
                m_mailboxStatus =
                    QStringLiteral("Sin buzón. Los mensajes esperarán en este equipo hasta que "
                                   "ambos estéis conectados a la vez.");
            } else if (object.value(QStringLiteral("reachable")).toBool()) {
                const int announced = object.value(QStringLiteral("announced")).toInt();
                m_mailboxStatus =
                    QStringLiteral("Buzón activo. Se avisó a %1 contacto(s).").arg(announced);
            } else {
                m_mailboxStatus =
                    QStringLiteral("Guardado, pero el buzón no respondió. Revisa la dirección: "
                                   "hasta que responda, los mensajes seguirán esperando aquí.");
            }
            emit settingsChanged();
        } else if (event == QStringLiteral("microphone_test")) {
            m_microphoneTestStatus =
                object.value(QStringLiteral("detected")).toBool()
                ? QStringLiteral("Micrófono listo.")
                : QStringLiteral("No se recibió audio. Revisa permisos y dispositivo.");
            emit settingsChanged();
        } else if (event == QStringLiteral("profile")) {
            m_profileName = object.value(QStringLiteral("name")).toString();
            m_profileAvatar = object.value(QStringLiteral("avatar")).toString();
            m_secureStorageEnabled =
                object.value(QStringLiteral("secure_storage")).toBool();
            emit profileChanged();
            emit settingsChanged();
        } else if (event == QStringLiteral("update")) {
            m_updateAvailable = object.value(QStringLiteral("available")).toBool();
            m_updateVersion = object.value(QStringLiteral("version")).toString();
            m_updateUrl = QUrl(object.value(QStringLiteral("url")).toString());
            emit updateChanged();
        } else if (event == QStringLiteral("error")) {
            m_lastError = object.value(QStringLiteral("message")).toString();
            // A rejected address never produces mailbox_configured, so the pending
            // status would otherwise sit there claiming it is still checking.
            if (m_mailboxPending) {
                m_mailboxPending = false;
                m_mailboxStatus =
                    QStringLiteral("No se pudo guardar el buzón: %1").arg(m_lastError);
                emit settingsChanged();
            }
            emit connectionChanged();
        } else if (event == QStringLiteral("ready")) {
            m_lastError.clear();
            m_backendRestartAttempts = 0;
            m_profileName = object.value(QStringLiteral("name")).toString(m_profileName);
            m_profileAvatar = object.value(QStringLiteral("avatar")).toString();
            m_mailboxUrl = object.value(QStringLiteral("mailbox_url")).toString();
            m_mailboxStatus.clear();
            m_mailboxPending = false;
            emit profileChanged();
            emit settingsChanged();
            sendBackendCommand({{QStringLiteral("command"), QStringLiteral("check_update")}});
            sendBackendCommand({{QStringLiteral("command"), QStringLiteral("media_capabilities")}});
            emit connectionChanged();
            if (!m_startupInvite.isEmpty()) {
                acceptInvite(m_startupInvite);
                m_startupInvite.clear();
            }
        } else if (event == QStringLiteral("call_invite")) {
            m_pendingCallId = object.value(QStringLiteral("call_id")).toString();
            m_pendingCallContact = object.value(QStringLiteral("contact")).toString();
            m_pendingCallRinging = object.value(QStringLiteral("ring")).toBool();
            if (m_pendingCallRinging &&
                !conversationMuted(false, m_pendingCallContact) &&
                !conversationMuted(true, m_pendingCallContact)) {
                showNotification(QStringLiteral("Llamada entrante"),
                                 QStringLiteral("%1 te está llamando").arg(m_pendingCallContact), true);
            }
            const auto pendingId = m_pendingCallId;
            QTimer::singleShot(30000, this, [this, pendingId]() {
                if (m_pendingCallId != pendingId) return;
                sendBackendCommand({{QStringLiteral("command"), QStringLiteral("reject_call")},
                                    {QStringLiteral("contact"), m_pendingCallContact},
                                    {QStringLiteral("call_id"), m_pendingCallId},
                                    {QStringLiteral("missed"), true}});
                m_pendingCallId.clear();
                m_pendingCallContact.clear();
                m_pendingCallRinging = false;
                emit callChanged();
            });
            emit callChanged();
        } else if (event == QStringLiteral("call_started")) {
            m_callId = object.value(QStringLiteral("call_id")).toString();
            m_callActive = true;
            const auto ringing = object.value(QStringLiteral("ring")).toBool();
            m_callState = ringing ? QStringLiteral("calling") : QStringLiteral("connected");
            m_callParticipants = participantListFromJson(
                object.value(QStringLiteral("participants")).toArray());
            emit callChanged();
            if (!ringing && m_voiceMode != QStringLiteral("push_to_talk"))
                setMedia(QStringLiteral("voice"), true);
            if (ringing) {
                const auto outgoingId = m_callId;
                QTimer::singleShot(30000, this, [this, outgoingId]() {
                    if (m_callId != outgoingId || m_callState != QStringLiteral("calling")) return;
                    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("leave_call")},
                                        {QStringLiteral("contact"), conversationName()},
                                        {QStringLiteral("call_id"), m_callId},
                                        {QStringLiteral("missed"), true}});
                });
            }
        } else if (event == QStringLiteral("call_joined") ||
                   event == QStringLiteral("call_connected")) {
            m_callId = object.value(QStringLiteral("call_id")).toString();
            m_callActive = true;
            m_callState = QStringLiteral("connected");
            const auto participants = object.value(QStringLiteral("participants")).toArray();
            if (!participants.isEmpty()) {
                m_callParticipants = participantListFromJson(participants);
            }
            const auto deviceId = object.value(QStringLiteral("device_id")).toString();
            const auto participantName = object.value(QStringLiteral("contact")).toString();
            if (!deviceId.isEmpty()) {
                const auto exists = std::any_of(
                    m_callParticipants.cbegin(), m_callParticipants.cend(),
                    [&deviceId](const QVariant &value) {
                        return value.toMap().value(QStringLiteral("deviceId")).toString() == deviceId;
                    });
                if (!exists) {
                    m_callParticipants.append(QVariantMap{
                        {QStringLiteral("name"), participantName},
                        {QStringLiteral("deviceId"), deviceId},
                        {QStringLiteral("volume"), 1.0},
                    });
                }
            }
            emit callChanged();
            if (m_voiceMode != QStringLiteral("push_to_talk"))
                setMedia(QStringLiteral("voice"), true);
        } else if (event == QStringLiteral("call_held")) {
            m_heldCallId = object.value(QStringLiteral("call_id")).toString();
            m_heldCallContact = object.value(QStringLiteral("contact")).toString();
            if (m_callId == m_heldCallId) {
                m_callId.clear();
                m_callActive = false;
            }
            m_callState = QStringLiteral("held");
            emit callChanged();
        } else if (event == QStringLiteral("call_resumed")) {
            m_callId = object.value(QStringLiteral("call_id")).toString();
            m_heldCallId.clear();
            m_heldCallContact.clear();
            m_callActive = true;
            m_callState = QStringLiteral("connected");
            emit callChanged();
            if (m_voiceMode != QStringLiteral("push_to_talk"))
                setMedia(QStringLiteral("voice"), true);
        } else if (event == QStringLiteral("call_rejected")) {
            m_callId.clear();
            m_callActive = false;
            m_callState = object.value(QStringLiteral("outcome")).toString() == QStringLiteral("missed")
                ? QStringLiteral("missed") : QStringLiteral("rejected");
            m_pushToTalkPressed = false;
            m_microphoneEnabled = false;
            m_callParticipants.clear();
            emit callChanged();
        } else if (event == QStringLiteral("media_changed")) {
            const auto kind = object.value(QStringLiteral("kind")).toString();
            const auto enabled = object.value(QStringLiteral("enabled")).toBool();
            if (kind == QStringLiteral("voice")) m_microphoneEnabled = enabled;
            if (kind == QStringLiteral("camera")) m_cameraEnabled = enabled;
            if (kind == QStringLiteral("screen")) m_sharingScreen = enabled;
            emit callChanged();
        } else if (event == QStringLiteral("participant_volume")) {
            const auto deviceId = object.value(QStringLiteral("device_id")).toString();
            for (qsizetype index = 0; index < m_callParticipants.size(); ++index) {
                auto item = m_callParticipants.at(index).toMap();
                if (item.value(QStringLiteral("deviceId")).toString() != deviceId) continue;
                item[QStringLiteral("volume")] =
                    object.value(QStringLiteral("volume")).toDouble(1.0);
                m_callParticipants[index] = item;
                emit callChanged();
                break;
            }
        } else if (event == QStringLiteral("call_participant_leave")) {
            const auto deviceId = object.value(QStringLiteral("device_id")).toString();
            for (qsizetype index = m_callParticipants.size(); index-- > 0;) {
                if (m_callParticipants.at(index).toMap()
                        .value(QStringLiteral("deviceId")).toString() == deviceId) {
                    m_callParticipants.removeAt(index);
                }
            }
            emit callChanged();
        } else if (event == QStringLiteral("call_left") ||
                   event == QStringLiteral("call_leave")) {
            m_callId.clear();
            m_callActive = false;
            m_microphoneEnabled = false;
            m_cameraEnabled = false;
            m_sharingScreen = false;
            m_callParticipants.clear();
            m_pushToTalkPressed = false;
            m_callState = object.value(QStringLiteral("outcome")).toString() == QStringLiteral("missed")
                ? QStringLiteral("missed") : QStringLiteral("idle");
            emit callChanged();
        }
    }
}

void AppController::sendBackendCommand(const QVariantMap &command)
{
    if (m_backend == nullptr || m_backend->state() != QProcess::Running) {
        return;
    }
    m_backend->write(QJsonDocument::fromVariant(command).toJson(QJsonDocument::Compact));
    m_backend->write("\n");
}

void AppController::rebuildConversations()
{
    auto selectedKey = currentConversationKey();
    if (selectedKey.isEmpty()) {
        selectedKey = QSettings().value(QStringLiteral("ui/lastConversation")).toString();
    }
    QVariantList conversations;
    auto appendVisible = [this, &conversations](const QVariantList &items) {
        for (const auto &value : items) {
            const auto item = value.toMap();
            if (!m_archivedVisible && item.value(QStringLiteral("archived")).toBool()) continue;
            conversations.append(item);
        }
    };
    appendVisible(m_directContacts);
    appendVisible(m_groups);
    std::stable_sort(conversations.begin(), conversations.end(),
                     [](const QVariant &leftValue, const QVariant &rightValue) {
        const auto left = leftValue.toMap();
        const auto right = rightValue.toMap();
        if (left.value(QStringLiteral("pinned")).toBool() !=
            right.value(QStringLiteral("pinned")).toBool()) {
            return left.value(QStringLiteral("pinned")).toBool();
        }
        const auto leftActivity = left.value(QStringLiteral("lastActivity")).toLongLong();
        const auto rightActivity = right.value(QStringLiteral("lastActivity")).toLongLong();
        if (leftActivity != rightActivity) return leftActivity > rightActivity;
        return left.value(QStringLiteral("name")).toString().localeAwareCompare(
                   right.value(QStringLiteral("name")).toString()) < 0;
    });
    m_contacts = conversations;
    m_selectedConversation = 0;
    if (!selectedKey.isEmpty()) {
        for (qsizetype index = 0; index < m_contacts.size(); ++index) {
            const auto item = m_contacts.at(index).toMap();
            const auto key = item.value(item.value(QStringLiteral("group")).toBool()
                                            ? QStringLiteral("groupId")
                                            : QStringLiteral("identityId")).toString();
            if (key == selectedKey) {
                m_selectedConversation = static_cast<int>(index);
                break;
            }
        }
    }
    if (m_tray != nullptr) {
        int unread = 0;
        for (const auto &value : m_contacts) {
            unread += value.toMap().value(QStringLiteral("unread")).toInt();
        }
        m_tray->setToolTip(unread > 0
            ? QStringLiteral("pptalk · %1 sin leer").arg(unread)
            : QStringLiteral("pptalk"));
    }
    emit contactsChanged();
    emit conversationChanged();
}

bool AppController::isSelectedDirect(const QString &name) const
{
    if (m_contacts.isEmpty()) return false;
    const auto selected = m_contacts.value(m_selectedConversation).toMap();
    return !selected.value(QStringLiteral("group")).toBool() &&
           selected.value(QStringLiteral("name")).toString().compare(name, Qt::CaseInsensitive) == 0;
}

bool AppController::isSelectedGroup(const QString &groupId) const
{
    if (m_contacts.isEmpty()) return false;
    const auto selected = m_contacts.value(m_selectedConversation).toMap();
    return selected.value(QStringLiteral("group")).toBool() &&
           selected.value(QStringLiteral("groupId")).toString() == groupId;
}

void AppController::recordActivity(const bool isGroup, const QString &key,
                                   const QString &summary, const bool unread)
{
    auto &items = isGroup ? m_groups : m_directContacts;
    for (qsizetype index = 0; index < items.size(); ++index) {
        auto item = items.at(index).toMap();
        const auto candidate = item.value(isGroup ? QStringLiteral("groupId")
                                                   : QStringLiteral("name")).toString();
        const auto matches = isGroup ? candidate == key
                                     : candidate.compare(key, Qt::CaseInsensitive) == 0;
        if (!matches) continue;
        item[QStringLiteral("summary")] = summary;
        item[QStringLiteral("lastActivity")] = QDateTime::currentSecsSinceEpoch();
        if (unread) {
            item[QStringLiteral("unread")] = item.value(QStringLiteral("unread")).toInt() + 1;
            if (!item.value(QStringLiteral("muted")).toBool()) {
                showNotification(item.value(QStringLiteral("name")).toString(), summary, false, key);
            }
        }
        items[index] = item;
        const auto storageKey = item.value(isGroup ? QStringLiteral("groupId")
                                                   : QStringLiteral("identityId")).toString();
        sendBackendCommand({
            {QStringLiteral("command"), QStringLiteral("record_conversation_activity")},
            {QStringLiteral("conversation_key"), storageKey},
            {QStringLiteral("summary"), summary},
            {QStringLiteral("unread"), unread},
            {QStringLiteral("clear_unread"), false},
        });
        rebuildConversations();
        return;
    }
}

void AppController::showNotification(const QString &title, const QString &body, const bool ring,
                                     const QString &conversationKey)
{
    if (m_doNotDisturb) return;
    m_notificationConversationKey = conversationKey;
    if (m_tray != nullptr) {
        m_tray->showMessage(title, body, QSystemTrayIcon::Information, 8000);
    }
    if (ring) QApplication::beep();
}

bool AppController::conversationMuted(const bool isGroup, const QString &key) const
{
    const auto &items = isGroup ? m_groups : m_directContacts;
    for (const auto &value : items) {
        const auto item = value.toMap();
        const auto candidate = isGroup
            ? (item.value(QStringLiteral("groupId")).toString() == key ||
               item.value(QStringLiteral("name")).toString().compare(key, Qt::CaseInsensitive) == 0)
            : item.value(QStringLiteral("name")).toString().compare(key, Qt::CaseInsensitive) == 0;
        if (candidate) return item.value(QStringLiteral("muted")).toBool();
    }
    return false;
}
