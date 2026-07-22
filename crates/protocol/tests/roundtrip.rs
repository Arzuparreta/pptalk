use pptalk_protocol::{
    CausalFrontier, ConversationEvent, ConversationId, DeviceId, EventBody, EventId, IdentityId,
    MessageContent, PROTOCOL_VERSION, WireDecode, WireEncode,
};

#[test]
fn conversation_event_has_stable_roundtrip() {
    let author_device = DeviceId::from_bytes([2; 32]);
    let mut frontier = CausalFrontier::new();
    frontier.insert(author_device, 41);
    let event = ConversationEvent {
        version: PROTOCOL_VERSION,
        conversation_id: ConversationId::from_bytes([1; 32]),
        event_id: EventId::from_bytes([3; 32]),
        author_identity: IdentityId::from_bytes([4; 32]),
        author_device,
        device_sequence: 42,
        causal_frontier: frontier,
        logical_time_ms: 1_700_000_000_000,
        body: EventBody::MessageCreate {
            content: MessageContent {
                text: "hello".into(),
                reply_to: None,
                attachment_ids: Vec::new(),
            },
        },
    };

    let encoded = event.to_wire().expect("encode");
    let decoded = ConversationEvent::from_wire(&encoded).expect("decode");
    assert_eq!(decoded, event);
    assert_eq!(event.to_wire().expect("second encode"), encoded);
}

#[test]
fn oversized_wire_value_is_rejected_before_decode() {
    let oversized = vec![0; pptalk_protocol::MAX_ENVELOPE_BYTES + 1];
    assert!(ConversationEvent::from_wire(&oversized).is_err());
}
