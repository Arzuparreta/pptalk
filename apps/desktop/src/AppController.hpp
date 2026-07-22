#pragma once

#include <QObject>
#include <QString>
#include <QVariantList>
#include <QUrl>

class QProcess;

class AppController final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantList contacts READ contacts NOTIFY contactsChanged)
    Q_PROPERTY(QVariantList messages READ messages NOTIFY messagesChanged)
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(QString conversationName READ conversationName NOTIFY conversationChanged)
    Q_PROPERTY(QString presence READ presence NOTIFY conversationChanged)
    Q_PROPERTY(QString connectionLabel READ connectionLabel NOTIFY connectionChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY connectionChanged)
    Q_PROPERTY(bool conversationIsGroup READ conversationIsGroup NOTIFY conversationChanged)
    Q_PROPERTY(QString inviteLink READ inviteLink NOTIFY inviteLinkChanged)
    Q_PROPERTY(QString deviceLink READ deviceLink NOTIFY deviceLinkChanged)
    Q_PROPERTY(bool callActive READ callActive NOTIFY callChanged)
    Q_PROPERTY(bool microphoneEnabled READ microphoneEnabled NOTIFY callChanged)
    Q_PROPERTY(bool cameraEnabled READ cameraEnabled NOTIFY callChanged)
    Q_PROPERTY(bool sharingScreen READ sharingScreen NOTIFY callChanged)
    Q_PROPERTY(bool incomingCallPending READ incomingCallPending NOTIFY callChanged)
    Q_PROPERTY(bool incomingCallRinging READ incomingCallRinging NOTIFY callChanged)
    Q_PROPERTY(QString incomingCallContact READ incomingCallContact NOTIFY callChanged)

public:
    explicit AppController(QObject *parent = nullptr);

    [[nodiscard]] QVariantList contacts() const;
    [[nodiscard]] QVariantList messages() const;
    [[nodiscard]] QVariantList devices() const;
    [[nodiscard]] QString conversationName() const;
    [[nodiscard]] QString presence() const;
    [[nodiscard]] QString connectionLabel() const;
    [[nodiscard]] QString lastError() const;
    [[nodiscard]] bool conversationIsGroup() const;
    [[nodiscard]] QString inviteLink() const;
    [[nodiscard]] QString deviceLink() const;
    [[nodiscard]] bool callActive() const;
    [[nodiscard]] bool microphoneEnabled() const;
    [[nodiscard]] bool cameraEnabled() const;
    [[nodiscard]] bool sharingScreen() const;
    [[nodiscard]] bool incomingCallPending() const;
    [[nodiscard]] bool incomingCallRinging() const;
    [[nodiscard]] QString incomingCallContact() const;

    Q_INVOKABLE void selectConversation(int index);
    Q_INVOKABLE void sendMessage(const QString &body);
    Q_INVOKABLE void sendFile(const QUrl &file);
    Q_INVOKABLE void createInvite();
    Q_INVOKABLE void createGroup(const QString &name, const QString &members);
    Q_INVOKABLE void configureMailbox(const QString &url);
    Q_INVOKABLE void configureVideoQuality(int preset);
    Q_INVOKABLE void createDeviceLink(const QString &label);
    Q_INVOKABLE void revokeDevice(const QString &deviceId);
    Q_INVOKABLE void copyDeviceLink();
    Q_INVOKABLE void addGroupMember(const QString &contact);
    Q_INVOKABLE void removeGroupMember(const QString &contact);
    Q_INVOKABLE void acceptInvite(const QString &url);
    Q_INVOKABLE void copyInvite();
    Q_INVOKABLE void startCall(bool ringEveryone);
    Q_INVOKABLE void leaveCall();
    Q_INVOKABLE void acceptIncomingCall();
    Q_INVOKABLE void declineIncomingCall();
    Q_INVOKABLE void toggleMicrophone();
    Q_INVOKABLE void toggleCamera();
    Q_INVOKABLE void toggleScreenShare();

signals:
    void contactsChanged();
    void messagesChanged();
    void conversationChanged();
    void connectionChanged();
    void inviteLinkChanged();
    void deviceLinkChanged();
    void devicesChanged();
    void callChanged();

private:
    void startBackend();
    void processBackendOutput();
    void sendBackendCommand(const QVariantMap &command);
    void setMedia(const QString &kind, bool enabled);
    void rebuildConversations();
    [[nodiscard]] bool isSelectedDirect(const QString &name) const;
    [[nodiscard]] bool isSelectedGroup(const QString &groupId) const;
    void recordActivity(bool isGroup, const QString &key, const QString &summary, bool unread);

    QVariantList m_contacts;
    QVariantList m_directContacts;
    QVariantList m_groups;
    QVariantList m_messages;
    QVariantList m_devices;
    int m_selectedConversation = 0;
    QString m_inviteLink;
    QString m_deviceLink;
    bool m_callActive = false;
    bool m_microphoneEnabled = false;
    bool m_cameraEnabled = false;
    bool m_sharingScreen = false;
    bool m_manualVideoQuality = false;
    int m_videoWidth = 1280;
    int m_videoHeight = 720;
    int m_videoFramesPerSecond = 30;
    int m_videoBitrateKbps = 2500;
    QProcess *m_backend = nullptr;
    QByteArray m_backendBuffer;
    QString m_profilePath;
    QString m_lastError;
    QString m_callId;
    QString m_pendingCallId;
    QString m_pendingCallContact;
    bool m_pendingCallRinging = false;
};
