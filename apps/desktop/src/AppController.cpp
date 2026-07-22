#include "AppController.hpp"

#include <QClipboard>
#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QStandardPaths>
#include <QVariantMap>

namespace {
QVariantMap contact(const QString &name, const QString &summary, const QString &presence,
                    const QString &accent, const int unread)
{
    return {{QStringLiteral("name"), name},
            {QStringLiteral("summary"), summary},
            {QStringLiteral("presence"), presence},
            {QStringLiteral("accent"), accent},
            {QStringLiteral("unread"), unread}};
}

QVariantMap message(const QString &author, const QString &body, const QString &time, const bool own)
{
    return {{QStringLiteral("author"), author},
            {QStringLiteral("body"), body},
            {QStringLiteral("time"), time},
            {QStringLiteral("own"), own}};
}

QVariantMap group(const QString &name, const QString &id, const int members)
{
    return {{QStringLiteral("name"), name},
            {QStringLiteral("summary"), QStringLiteral("grupo MLS · %1 miembros").arg(members)},
            {QStringLiteral("presence"), QStringLiteral("cifrado MLS")},
            {QStringLiteral("accent"), QStringLiteral("#D091FF")},
            {QStringLiteral("unread"), 0},
            {QStringLiteral("group"), true},
            {QStringLiteral("groupId"), id}};
}
} // namespace

AppController::AppController(QObject *parent)
    : QObject(parent)
{
    startBackend();
}

QVariantList AppController::contacts() const { return m_contacts; }
QVariantList AppController::messages() const { return m_messages; }
QVariantList AppController::devices() const { return m_devices; }

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

bool AppController::conversationIsGroup() const
{
    return !m_contacts.isEmpty() &&
           m_contacts.value(m_selectedConversation).toMap().value(QStringLiteral("group")).toBool();
}

QString AppController::inviteLink() const { return m_inviteLink; }
QString AppController::deviceLink() const { return m_deviceLink; }
bool AppController::callActive() const { return m_callActive; }
bool AppController::microphoneEnabled() const { return m_microphoneEnabled; }
bool AppController::cameraEnabled() const { return m_cameraEnabled; }
bool AppController::sharingScreen() const { return m_sharingScreen; }
bool AppController::incomingCallPending() const { return !m_pendingCallId.isEmpty(); }
bool AppController::incomingCallRinging() const { return m_pendingCallRinging; }
QString AppController::incomingCallContact() const { return m_pendingCallContact; }

void AppController::selectConversation(const int index)
{
    if (index < 0 || index >= m_contacts.size() || index == m_selectedConversation) {
        return;
    }
    m_selectedConversation = index;
    auto selected = m_contacts[index].toMap();
    selected[QStringLiteral("unread")] = 0;
    m_contacts[index] = selected;
    emit contactsChanged();
    emit conversationChanged();
    emit connectionChanged();
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

void AppController::configureMailbox(const QString &url)
{
    const auto trimmed = url.trimmed();
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("set_mailbox")},
                        {QStringLiteral("url"), trimmed.isEmpty() ? QVariant() : QVariant(trimmed)}});
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

void AppController::acceptInvite(const QString &url)
{
    if (url.trimmed().isEmpty()) {
        return;
    }
    sendBackendCommand({{QStringLiteral("command"), QStringLiteral("accept")},
                        {QStringLiteral("url"), url.trimmed()}});
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
                            {QStringLiteral("call_id"), m_callId}});
    }
    m_callActive = false;
    m_callId.clear();
    m_microphoneEnabled = false;
    m_cameraEnabled = false;
    m_sharingScreen = false;
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
        sendBackendCommand({{QStringLiteral("command"), QStringLiteral("leave_call")},
                            {QStringLiteral("contact"), m_pendingCallContact},
                            {QStringLiteral("call_id"), m_pendingCallId}});
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
    m_profilePath = dataDirectory + QStringLiteral("/profile.json");

    auto program = qEnvironmentVariable("PPTALK_CLI");
    if (program.isEmpty()) {
        program = QCoreApplication::applicationDirPath() + QStringLiteral("/pptalk-cli");
#ifdef Q_OS_WIN
        program += QStringLiteral(".exe");
#endif
    }
    if (!QFileInfo::exists(program)) {
        program = QStringLiteral("pptalk-cli");
    }

    if (!QFileInfo::exists(m_profilePath)) {
        const auto deviceLink = qEnvironmentVariable("PPTALK_DEVICE_LINK");
        int exitCode = 0;
        if (!deviceLink.isEmpty()) {
            exitCode = QProcess::execute(program,
                                         {QStringLiteral("import-device"),
                                          QStringLiteral("--profile"), m_profilePath, deviceLink});
        } else {
            auto name = qEnvironmentVariable("USER");
            if (name.isEmpty()) {
                name = qEnvironmentVariable("USERNAME", QStringLiteral("You"));
            }
            QStringList arguments = {QStringLiteral("init"), QStringLiteral("--profile"),
                                     m_profilePath, QStringLiteral("--name"), name};
            const auto mailbox = qEnvironmentVariable("PPTALK_MAILBOX_URL");
            if (!mailbox.isEmpty()) {
                arguments << QStringLiteral("--mailbox-url") << mailbox;
            }
            exitCode = QProcess::execute(program, arguments);
        }
        if (exitCode != 0) {
            m_lastError = QStringLiteral("No se pudo crear la identidad local");
            return;
        }
    }

    m_backend = new QProcess(this);
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
    m_backend->start(program,
                     {QStringLiteral("daemon"), QStringLiteral("--profile"), m_profilePath});
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
                contacts.append(contact(value.value(QStringLiteral("name")).toString(),
                                        value.value(QStringLiteral("verified")).toBool()
                                            ? QStringLiteral("contacto verificado · %1 dispositivo%2")
                                                  .arg(deviceCount)
                                                  .arg(deviceCount == 1 ? QString() : QStringLiteral("s"))
                                            : QStringLiteral("verificación pendiente"),
                                        QStringLiteral("P2P · disponible al conectar"),
                                        accents.at(index % accents.size()), 0));
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
                groups.append(group(item.value(QStringLiteral("name")).toString(),
                                    item.value(QStringLiteral("id")).toString(),
                                    item.value(QStringLiteral("member_count")).toInt()));
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
                                        item.value(QStringLiteral("body")).toString(),
                                        timestamp.toString(QStringLiteral("HH:mm")),
                                        item.value(QStringLiteral("outgoing")).toBool()));
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
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), own));
                emit messagesChanged();
            }
            recordActivity(false, contactName, body, !own && !isSelectedDirect(contactName));
        } else if (event == QStringLiteral("group_message")) {
            const auto groupId = object.value(QStringLiteral("group_id")).toString();
            const auto outgoing = object.value(QStringLiteral("outgoing")).toBool();
            const auto body = object.value(QStringLiteral("body")).toString();
            if (isSelectedGroup(groupId)) {
                m_messages.append(message(outgoing ? QStringLiteral("Tú")
                                                   : object.value(QStringLiteral("author")).toString(),
                                          body,
                                          QDateTime::currentDateTime().toString(QStringLiteral("HH:mm")), outgoing));
                emit messagesChanged();
            }
            recordActivity(true, groupId, body, !outgoing && !isSelectedGroup(groupId));
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
            recordActivity(false, contactName, body, !own && !isSelectedDirect(contactName));
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
            recordActivity(true, groupId, body, !own && !isSelectedGroup(groupId));
        } else if (event == QStringLiteral("invite")) {
            m_inviteLink = object.value(QStringLiteral("url")).toString();
            emit inviteLinkChanged();
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
        } else if (event == QStringLiteral("error")) {
            m_lastError = object.value(QStringLiteral("message")).toString();
            emit connectionChanged();
        } else if (event == QStringLiteral("ready")) {
            m_lastError.clear();
            emit connectionChanged();
        } else if (event == QStringLiteral("call_invite")) {
            m_pendingCallId = object.value(QStringLiteral("call_id")).toString();
            m_pendingCallContact = object.value(QStringLiteral("contact")).toString();
            m_pendingCallRinging = object.value(QStringLiteral("ring")).toBool();
            emit callChanged();
        } else if (event == QStringLiteral("call_started") ||
                   event == QStringLiteral("call_joined")) {
            m_callId = object.value(QStringLiteral("call_id")).toString();
            m_callActive = true;
            emit callChanged();
            setMedia(QStringLiteral("voice"), true);
        } else if (event == QStringLiteral("media_changed")) {
            const auto kind = object.value(QStringLiteral("kind")).toString();
            const auto enabled = object.value(QStringLiteral("enabled")).toBool();
            if (kind == QStringLiteral("voice")) m_microphoneEnabled = enabled;
            if (kind == QStringLiteral("camera")) m_cameraEnabled = enabled;
            if (kind == QStringLiteral("screen")) m_sharingScreen = enabled;
            emit callChanged();
        } else if (event == QStringLiteral("call_left") ||
                   event == QStringLiteral("call_leave")) {
            m_callId.clear();
            m_callActive = false;
            m_microphoneEnabled = false;
            m_cameraEnabled = false;
            m_sharingScreen = false;
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
    m_contacts = m_directContacts;
    for (const auto &group : m_groups) {
        m_contacts.append(group);
    }
    if (m_selectedConversation >= m_contacts.size()) {
        m_selectedConversation = 0;
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
        if (unread) {
            item[QStringLiteral("unread")] = item.value(QStringLiteral("unread")).toInt() + 1;
        }
        items[index] = item;
        rebuildConversations();
        return;
    }
}
