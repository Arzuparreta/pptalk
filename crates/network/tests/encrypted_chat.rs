use pptalk_core::{Conversation, ConversationBuilder, GroupSecret};
use pptalk_network::PeerNetwork;
use pptalk_protocol::{
    CausalFrontier, ConversationEvent, ConversationId, EventBody, IdentityId, MessageContent,
    TransportEnvelope, WireDecode, WireEncode,
};
use rand::rngs::OsRng;

#[tokio::test]
async fn encrypted_message_crosses_the_real_transport_and_materializes() {
    let alice = PeerNetwork::start_direct().await.expect("alice network");
    let bob = PeerNetwork::start_direct().await.expect("bob network");
    let conversation_id = ConversationId::from_bytes([3; 32]);
    let identity = IdentityId::from_bytes([4; 32]);
    let device = pptalk_protocol::DeviceId::from_bytes([5; 32]);
    let mut builder =
        ConversationBuilder::new(conversation_id, identity, device, CausalFrontier::new());
    let event = builder
        .build(
            EventBody::MessageCreate {
                content: MessageContent {
                    text: "mensaje privado".into(),
                    reply_to: None,
                    attachment_ids: vec![],
                },
            },
            42,
            &mut OsRng,
        )
        .expect("event");
    let secret = GroupSecret::from_bytes([9; 32]);
    let encrypted = secret
        .encrypt(
            &event.to_wire().expect("encode event"),
            conversation_id.as_bytes(),
        )
        .expect("encrypt");
    let envelope = TransportEnvelope::new(
        [8; 32],
        encrypted.to_wire().expect("encode encrypted payload"),
        1024,
    );

    alice
        .send(&bob.local_address(), &envelope.to_wire().expect("envelope"))
        .await
        .expect("transport");
    let received = bob.receive().await.expect("receive");
    assert!(
        !received
            .bytes
            .windows("mensaje privado".len())
            .any(|window| window == b"mensaje privado")
    );
    let envelope = TransportEnvelope::from_wire(&received.bytes).expect("decode envelope");
    assert!(envelope.verify());
    let encrypted =
        pptalk_core::EncryptedPayload::from_wire(&envelope.ciphertext).expect("payload");
    let plaintext = secret
        .decrypt(&encrypted, conversation_id.as_bytes())
        .expect("decrypt");
    let event = ConversationEvent::from_wire(&plaintext).expect("event");
    let mut conversation = Conversation::new(conversation_id, identity, "private");
    conversation.apply(event).expect("materialize");
    assert_eq!(
        conversation
            .messages()
            .next()
            .expect("message")
            .content
            .text,
        "mensaje privado"
    );

    alice.shutdown().await.expect("alice shutdown");
    bob.shutdown().await.expect("bob shutdown");
}
