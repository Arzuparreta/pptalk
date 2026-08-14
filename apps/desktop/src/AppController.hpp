#pragma once

#include <QMap>
#include <QObject>
#include <QPointer>
#include <QQuickItem>
#include <QQuickWindow>
#include <QString>
#include <QVariantList>
#include <QWindow>
#include <QUrl>

class QProcess;
class QSystemTrayIcon;
class QTimer;
class QEvent;

class AppController final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantList contacts READ contacts NOTIFY contactsChanged)
    Q_PROPERTY(QVariantList messages READ messages NOTIFY messagesChanged)
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(QVariantList directContacts READ directContacts NOTIFY contactsChanged)
    Q_PROPERTY(QVariantList mediaDevices READ mediaDevices NOTIFY mediaDevicesChanged)
    Q_PROPERTY(QVariantList callParticipants READ callParticipants NOTIFY callChanged)
    Q_PROPERTY(QVariantList transfers READ transfers NOTIFY transfersChanged)
    Q_PROPERTY(QVariantList searchResults READ searchResults NOTIFY searchResultsChanged)
    Q_PROPERTY(QString profileName READ profileName NOTIFY profileChanged)
    Q_PROPERTY(QString profileAvatar READ profileAvatar NOTIFY profileChanged)
    Q_PROPERTY(QString conversationName READ conversationName NOTIFY conversationChanged)
    Q_PROPERTY(int selectedConversationIndex READ selectedConversationIndex NOTIFY conversationChanged)
    Q_PROPERTY(QString presence READ presence NOTIFY conversationChanged)
    Q_PROPERTY(QString connectionLabel READ connectionLabel NOTIFY connectionChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY connectionChanged)
    Q_PROPERTY(QString focusedMessageId READ focusedMessageId NOTIFY messagesChanged)
    Q_PROPERTY(bool conversationIsGroup READ conversationIsGroup NOTIFY conversationChanged)
    Q_PROPERTY(bool currentGroupOwned READ currentGroupOwned NOTIFY conversationChanged)
    Q_PROPERTY(bool currentGroupAdmin READ currentGroupAdmin NOTIFY conversationChanged)
    Q_PROPERTY(bool currentContactBlocked READ currentContactBlocked NOTIFY conversationChanged)
    Q_PROPERTY(bool currentContactPrivacyHidden READ currentContactPrivacyHidden NOTIFY conversationChanged)
    Q_PROPERTY(bool currentConversationPinned READ currentConversationPinned NOTIFY conversationChanged)
    Q_PROPERTY(bool currentConversationArchived READ currentConversationArchived NOTIFY conversationChanged)
    Q_PROPERTY(bool currentConversationMuted READ currentConversationMuted NOTIFY conversationChanged)
    Q_PROPERTY(QString inviteLink READ inviteLink NOTIFY inviteLinkChanged)
    Q_PROPERTY(QString inviteQr READ inviteQr NOTIFY inviteLinkChanged)
    Q_PROPERTY(QString invitePreviewName READ invitePreviewName NOTIFY invitePreviewChanged)
    Q_PROPERTY(QString invitePreviewExpiry READ invitePreviewExpiry NOTIFY invitePreviewChanged)
    Q_PROPERTY(QString deviceLink READ deviceLink NOTIFY deviceLinkChanged)
    Q_PROPERTY(QString currentContactFingerprint READ currentContactFingerprint NOTIFY conversationChanged)
    Q_PROPERTY(bool currentContactVerified READ currentContactVerified NOTIFY conversationChanged)
    Q_PROPERTY(bool onboardingRequired READ onboardingRequired NOTIFY onboardingChanged)
    Q_PROPERTY(QString onboardingLink READ onboardingLink NOTIFY onboardingChanged)
    Q_PROPERTY(QString backupStatus READ backupStatus NOTIFY backupStatusChanged)
    Q_PROPERTY(bool secureStorageEnabled READ secureStorageEnabled NOTIFY settingsChanged)
    Q_PROPERTY(QString mailboxUrl READ mailboxUrl NOTIFY settingsChanged)
    Q_PROPERTY(QString mailboxStatus READ mailboxStatus NOTIFY settingsChanged)
    Q_PROPERTY(QString microphoneTestStatus READ microphoneTestStatus NOTIFY settingsChanged)
    Q_PROPERTY(bool archivedVisible READ archivedVisible WRITE setArchivedVisible NOTIFY settingsChanged)
    Q_PROPERTY(bool callActive READ callActive NOTIFY callChanged)
    Q_PROPERTY(bool callOngoing READ callOngoing NOTIFY callChanged)
    Q_PROPERTY(QString callContact READ callContact NOTIFY callChanged)
    Q_PROPERTY(bool microphoneEnabled READ microphoneEnabled NOTIFY callChanged)
    Q_PROPERTY(bool cameraEnabled READ cameraEnabled NOTIFY callChanged)
    Q_PROPERTY(bool sharingScreen READ sharingScreen NOTIFY callChanged)
    Q_PROPERTY(bool remoteCamera READ remoteCamera NOTIFY callChanged)
    Q_PROPERTY(bool remoteScreen READ remoteScreen NOTIFY callChanged)
    Q_PROPERTY(bool hasCamera READ hasCamera NOTIFY mediaDevicesChanged)
    Q_PROPERTY(bool incomingCallPending READ incomingCallPending NOTIFY callChanged)
    Q_PROPERTY(bool incomingCallRinging READ incomingCallRinging NOTIFY callChanged)
    Q_PROPERTY(QString incomingCallContact READ incomingCallContact NOTIFY callChanged)
    Q_PROPERTY(QString callState READ callState NOTIFY callChanged)
    Q_PROPERTY(bool doNotDisturb READ doNotDisturb WRITE setDoNotDisturb NOTIFY settingsChanged)
    Q_PROPERTY(QString voiceMode READ voiceMode WRITE setVoiceMode NOTIFY settingsChanged)
    Q_PROPERTY(QString pushToTalkShortcut READ pushToTalkShortcut WRITE setPushToTalkShortcut NOTIFY settingsChanged)
    Q_PROPERTY(bool platformSupportsAutostart READ platformSupportsAutostart CONSTANT)
    Q_PROPERTY(bool autostartEnabled READ autostartEnabled WRITE setAutostartEnabled NOTIFY settingsChanged)
    Q_PROPERTY(bool updateAvailable READ updateAvailable NOTIFY updateChanged)
    Q_PROPERTY(QString updateVersion READ updateVersion NOTIFY updateChanged)
    Q_PROPERTY(int videoQualityPreset READ videoQualityPreset NOTIFY settingsChanged)

public:
    explicit AppController(QObject *parent = nullptr);
    ~AppController() override;

    [[nodiscard]] QVariantList contacts() const;
    [[nodiscard]] QVariantList messages() const;
    [[nodiscard]] QVariantList devices() const;
    [[nodiscard]] QVariantList directContacts() const;
    [[nodiscard]] QVariantList mediaDevices() const;
    [[nodiscard]] QVariantList callParticipants() const;
    [[nodiscard]] QVariantList transfers() const;
    [[nodiscard]] QVariantList searchResults() const;
    [[nodiscard]] QString profileName() const;
    [[nodiscard]] QString profileAvatar() const;
    [[nodiscard]] QString conversationName() const;
    [[nodiscard]] int selectedConversationIndex() const;
    [[nodiscard]] QString presence() const;
    [[nodiscard]] QString connectionLabel() const;
    [[nodiscard]] QString lastError() const;
    [[nodiscard]] QString focusedMessageId() const;
    [[nodiscard]] bool conversationIsGroup() const;
    [[nodiscard]] bool currentGroupOwned() const;
    [[nodiscard]] bool currentGroupAdmin() const;
    [[nodiscard]] bool currentContactBlocked() const;
    [[nodiscard]] bool currentContactPrivacyHidden() const;
    [[nodiscard]] bool currentConversationPinned() const;
    [[nodiscard]] bool currentConversationArchived() const;
    [[nodiscard]] bool currentConversationMuted() const;
    [[nodiscard]] QString inviteLink() const;
    [[nodiscard]] QString inviteQr() const;
    [[nodiscard]] QString invitePreviewName() const;
    [[nodiscard]] QString invitePreviewExpiry() const;
    [[nodiscard]] QString deviceLink() const;
    [[nodiscard]] QString currentContactFingerprint() const;
    [[nodiscard]] bool currentContactVerified() const;
    [[nodiscard]] bool onboardingRequired() const;
    [[nodiscard]] QString onboardingLink() const;
    [[nodiscard]] QString backupStatus() const;
    [[nodiscard]] bool secureStorageEnabled() const;
    [[nodiscard]] QString mailboxUrl() const;
    [[nodiscard]] QString mailboxStatus() const;
    [[nodiscard]] QString microphoneTestStatus() const;
    [[nodiscard]] bool archivedVisible() const;
    [[nodiscard]] bool callActive() const;
    [[nodiscard]] bool callOngoing() const;
    [[nodiscard]] QString callContact() const;
    [[nodiscard]] bool microphoneEnabled() const;
    [[nodiscard]] bool cameraEnabled() const;
    [[nodiscard]] bool sharingScreen() const;
    [[nodiscard]] bool remoteCamera() const;
    [[nodiscard]] bool remoteScreen() const;
    [[nodiscard]] bool hasCamera() const;
    [[nodiscard]] bool incomingCallPending() const;
    [[nodiscard]] bool incomingCallRinging() const;
    [[nodiscard]] QString incomingCallContact() const;
    [[nodiscard]] QString callState() const;
    [[nodiscard]] bool doNotDisturb() const;
    [[nodiscard]] QString voiceMode() const;
    [[nodiscard]] QString pushToTalkShortcut() const;
    [[nodiscard]] bool platformSupportsAutostart() const;
    [[nodiscard]] bool autostartEnabled() const;
    [[nodiscard]] bool updateAvailable() const;
    [[nodiscard]] QString updateVersion() const;
    [[nodiscard]] int videoQualityPreset() const;

    Q_INVOKABLE void selectConversation(int index);
    Q_INVOKABLE void sendMessage(const QString &body);
    Q_INVOKABLE void replyToMessage(const QString &messageId, const QString &body);
    Q_INVOKABLE void editMessage(const QString &messageId, const QString &body);
    Q_INVOKABLE void deleteMessage(const QString &messageId);
    Q_INVOKABLE void deleteMessageLocal(const QString &messageId);
    Q_INVOKABLE void search(const QString &query);
    Q_INVOKABLE void clearSearch();
    Q_INVOKABLE void openSearchResult(const QString &conversationKey,
                                      const QString &messageId);
    Q_INVOKABLE void openMessageFile(const QString &path);
    Q_INVOKABLE void sendFile(const QUrl &file);
    Q_INVOKABLE void cancelTransfer(const QString &transferId);
    Q_INVOKABLE void initializeProfile(const QString &name);
    Q_INVOKABLE void importDeviceLink(const QString &link);
    Q_INVOKABLE void exportIdentityBackup(const QUrl &file, const QString &passphrase);
    Q_INVOKABLE void restoreIdentityBackup(const QUrl &file, const QString &passphrase);
    Q_INVOKABLE void protectLocalSecrets();
    Q_INVOKABLE void setMailbox(const QString &url);
    Q_INVOKABLE void clearMailbox();
    Q_INVOKABLE void testMicrophone();
    Q_INVOKABLE QString draft() const;
    Q_INVOKABLE void saveDraft(const QString &body);
    Q_INVOKABLE void createInvite();
    Q_INVOKABLE void createGroup(const QString &name, const QString &members);
    Q_INVOKABLE void configureVideoQuality(int preset);
    Q_INVOKABLE void selectMediaDevice(const QString &kind, const QString &deviceId);
    Q_INVOKABLE QString selectedMediaDevice(const QString &kind) const;
    Q_INVOKABLE void createDeviceLink(const QString &label);
    Q_INVOKABLE void revokeDevice(const QString &deviceId);
    Q_INVOKABLE void copyDeviceLink();
    Q_INVOKABLE void updateProfile(const QString &name, const QUrl &avatarFile);
    Q_INVOKABLE void clearProfileAvatar();
    Q_INVOKABLE void downloadUpdate();
    Q_INVOKABLE void addGroupMember(const QString &contact);
    Q_INVOKABLE void removeGroupMember(const QString &contact);
    Q_INVOKABLE void setGroupAdministrator(const QString &contact, bool administrator);
    Q_INVOKABLE void transferGroupOwnership(const QString &contact);
    Q_INVOKABLE void dissolveCurrentGroup();
    Q_INVOKABLE void acceptInvite(const QString &url);
    Q_INVOKABLE void confirmInvite();
    Q_INVOKABLE void copyInvite();
    Q_INVOKABLE void startCall(bool ringEveryone);
    Q_INVOKABLE void leaveCall();
    Q_INVOKABLE void acceptIncomingCall();
    Q_INVOKABLE void declineIncomingCall();
    Q_INVOKABLE void toggleMicrophone();
    Q_INVOKABLE void toggleCamera();
    Q_INVOKABLE void toggleScreenShare();
    Q_INVOKABLE void attachVideoSurface(const QString &surface, QQuickItem *item);
    Q_INVOKABLE void detachVideoSurface(const QString &surface);
    Q_INVOKABLE void setParticipantVolume(const QString &deviceId, double volume);
    Q_INVOKABLE void removeCurrentContact();
    Q_INVOKABLE void setCurrentContactBlocked(bool blocked);
    Q_INVOKABLE void setCurrentContactPrivacy(bool hidden);
    Q_INVOKABLE void setCurrentContactVerified(bool verified);
    Q_INVOKABLE void setCurrentConversationPreferences(bool pinned, bool archived, bool muted);
    Q_INVOKABLE void holdCall();
    Q_INVOKABLE void resumeHeldCall();
    Q_INVOKABLE void dismissError();
    Q_INVOKABLE void handleExternalLink(const QString &link);
    void setDoNotDisturb(bool enabled);
    void setVoiceMode(const QString &mode);
    void setPushToTalkShortcut(const QString &shortcut);
    void setAutostartEnabled(bool enabled);
    void setArchivedVisible(bool visible);

signals:
    void contactsChanged();
    void messagesChanged();
    void conversationChanged();
    void connectionChanged();
    void inviteLinkChanged();
    void invitePreviewChanged();
    void deviceLinkChanged();
    void devicesChanged();
    void mediaDevicesChanged();
    void transfersChanged();
    void searchResultsChanged();
    void profileChanged();
    void callChanged();
    void settingsChanged();
    void updateChanged();
    void onboardingChanged();
    void backupStatusChanged();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void startBackend();
    void launchBackend();
    [[nodiscard]] QString cliProgram() const;
    [[nodiscard]] QString currentConversationKey() const;
    void processBackendOutput();
    void sendBackendCommand(const QVariantMap &command);
    void setMedia(const QString &kind, bool enabled);
    void rebuildConversations();
    [[nodiscard]] bool isSelectedDirect(const QString &name) const;
    [[nodiscard]] bool isSelectedGroup(const QString &groupId) const;
    void recordActivity(bool isGroup, const QString &key, const QString &summary, bool unread);
    void showNotification(const QString &title, const QString &body, bool ring,
                          const QString &conversationKey = {});
    [[nodiscard]] bool conversationMuted(bool isGroup, const QString &key) const;
    void syncVideoSurfaces();
    void resendVideoWindow(const QString &surface);
    void clearRemoteMedia();
    [[nodiscard]] static bool videoOverlaySupported();
    [[nodiscard]] static QString mediaErrorText(const QString &code, const QString &fallback);

    struct VideoSurfaceState {
        QPointer<QQuickItem> item;
        QPointer<QQuickWindow> window;
    };
    QMap<QString, VideoSurfaceState> m_videoSurfaces;

    QVariantList m_contacts;
    QVariantList m_directContacts;
    QVariantList m_groups;
    QVariantList m_messages;
    QVariantList m_devices;
    QVariantList m_mediaDevices;
    QVariantList m_callParticipants;
    QVariantList m_transfers;
    QVariantList m_searchResults;
    QVariantMap m_conversationPreferences;
    int m_selectedConversation = 0;
    QString m_inviteLink;
    QString m_inviteQr;
    QString m_invitePreviewUrl;
    QString m_invitePreviewName;
    QString m_invitePreviewExpiry;
    QString m_deviceLink;
    QString m_onboardingLink;
    QString m_backupStatus;
    QString m_profileName = QStringLiteral("Tú");
    QString m_profileAvatar;
    bool m_callActive = false;
    bool m_microphoneEnabled = false;
    bool m_cameraEnabled = false;
    bool m_sharingScreen = false;
    bool m_remoteCamera = false;
    bool m_remoteScreen = false;
    bool m_hasCamera = false;
    QTimer *m_videoSyncTimer = nullptr;
    bool m_manualVideoQuality = false;
    int m_videoWidth = 1280;
    int m_videoHeight = 720;
    int m_videoFramesPerSecond = 30;
    int m_videoBitrateKbps = 2500;
    QProcess *m_backend = nullptr;
    QSystemTrayIcon *m_tray = nullptr;
    QByteArray m_backendBuffer;
    QString m_profilePath;
    QString m_lastError;
    QString m_focusedMessageId;
    QString m_callId;
    QString m_callContact;
    QString m_pendingCallId;
    QString m_pendingCallContact;
    bool m_pendingCallRinging = false;
    QString m_callState = QStringLiteral("idle");
    QString m_heldCallId;
    QString m_heldCallContact;
    QVariantList m_heldCallParticipants;
    bool m_doNotDisturb = false;
    QString m_voiceMode = QStringLiteral("open");
    QString m_pushToTalkShortcut = QStringLiteral("Ctrl+Space");
    QString m_startupInvite;
    QString m_notificationConversationKey;
    bool m_onboardingRequired = false;
    bool m_archivedVisible = false;
    bool m_secureStorageEnabled = false;
    QString m_mailboxUrl;
    QString m_mailboxStatus;
    bool m_mailboxPending = false;
    QString m_microphoneTestStatus;
    int m_backendRestartAttempts = 0;
    bool m_shuttingDown = false;
    bool m_pushToTalkPressed = false;
    bool m_updateAvailable = false;
    QString m_updateVersion;
    QUrl m_updateUrl;
    int m_videoQualityPreset = 0;
};
