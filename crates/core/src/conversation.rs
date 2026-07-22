use std::collections::{BTreeMap, BTreeSet};

use pptalk_protocol::{
    CausalFrontier, ConversationEvent, ConversationId, DeviceId, EventBody, EventId, IdentityId,
    MessageContent, PROTOCOL_VERSION, WireEncode,
};
use rand::{CryptoRng, RngCore};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub owner: IdentityId,
    pub members: BTreeSet<IdentityId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedMessage {
    pub id: EventId,
    pub author: IdentityId,
    pub content: MessageContent,
    pub deleted: bool,
    pub reactions: BTreeMap<(IdentityId, String), bool>,
}

#[derive(Debug, Clone)]
pub struct Conversation {
    id: ConversationId,
    membership: Membership,
    frontier: CausalFrontier,
    events: BTreeMap<EventId, ConversationEvent>,
    messages: BTreeMap<EventId, MaterializedMessage>,
    name: String,
}

impl Conversation {
    pub fn new(id: ConversationId, owner: IdentityId, name: impl Into<String>) -> Self {
        Self {
            id,
            membership: Membership {
                owner,
                members: BTreeSet::from([owner]),
            },
            frontier: CausalFrontier::new(),
            events: BTreeMap::new(),
            messages: BTreeMap::new(),
            name: name.into(),
        }
    }

    pub const fn id(&self) -> ConversationId {
        self.id
    }

    pub fn membership(&self) -> &Membership {
        &self.membership
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn frontier(&self) -> &CausalFrontier {
        &self.frontier
    }

    pub fn events(&self) -> impl Iterator<Item = &ConversationEvent> {
        self.events.values()
    }

    pub fn messages(&self) -> impl Iterator<Item = &MaterializedMessage> {
        self.messages.values()
    }

    pub fn apply(&mut self, event: ConversationEvent) -> Result<(), ApplyError> {
        if event.version != PROTOCOL_VERSION || event.conversation_id != self.id {
            return Err(ApplyError::WrongConversation);
        }
        if self.events.contains_key(&event.event_id) {
            return Ok(());
        }
        let expected = self
            .frontier
            .get(&event.author_device)
            .copied()
            .unwrap_or(0)
            + 1;
        if event.device_sequence != expected {
            return Err(ApplyError::MissingPredecessor {
                device: event.author_device,
                expected,
                actual: event.device_sequence,
            });
        }
        if !self.membership.members.contains(&event.author_identity)
            && !matches!(event.body, EventBody::MembershipAdd { .. })
        {
            return Err(ApplyError::NotMember(event.author_identity));
        }

        match &event.body {
            EventBody::MessageCreate { content } => {
                validate_content(content)?;
                self.messages.insert(
                    event.event_id,
                    MaterializedMessage {
                        id: event.event_id,
                        author: event.author_identity,
                        content: content.clone(),
                        deleted: false,
                        reactions: BTreeMap::new(),
                    },
                );
            }
            EventBody::MessageEdit { target, content } => {
                validate_content(content)?;
                let message = self
                    .messages
                    .get_mut(target)
                    .ok_or(ApplyError::UnknownMessage(*target))?;
                if message.author != event.author_identity {
                    return Err(ApplyError::NotAuthor);
                }
                message.content.clone_from(content);
            }
            EventBody::MessageDelete { target } => {
                let message = self
                    .messages
                    .get_mut(target)
                    .ok_or(ApplyError::UnknownMessage(*target))?;
                if message.author != event.author_identity {
                    return Err(ApplyError::NotAuthor);
                }
                message.deleted = true;
            }
            EventBody::ReactionSet {
                target,
                emoji,
                active,
            } => {
                if emoji.chars().count() > 16 {
                    return Err(ApplyError::InvalidContent("reaction is too long"));
                }
                let message = self
                    .messages
                    .get_mut(target)
                    .ok_or(ApplyError::UnknownMessage(*target))?;
                message
                    .reactions
                    .insert((event.author_identity, emoji.clone()), *active);
            }
            EventBody::GroupRename { name } => {
                require_owner(&self.membership, event.author_identity)?;
                if name.is_empty() || name.chars().count() > 128 {
                    return Err(ApplyError::InvalidContent(
                        "group name must contain 1-128 characters",
                    ));
                }
                self.name.clone_from(name);
            }
            EventBody::MembershipAdd { identity, .. } => {
                require_owner(&self.membership, event.author_identity)?;
                self.membership.members.insert(*identity);
            }
            EventBody::MembershipRemove { identity, .. } => {
                require_owner(&self.membership, event.author_identity)?;
                if *identity == self.membership.owner {
                    return Err(ApplyError::OwnerMustTransfer);
                }
                self.membership.members.remove(identity);
            }
            EventBody::OwnershipTransfer { from, to } => {
                require_owner(&self.membership, event.author_identity)?;
                if *from != self.membership.owner || !self.membership.members.contains(to) {
                    return Err(ApplyError::InvalidOwnershipTransfer);
                }
                self.membership.owner = *to;
            }
            EventBody::Receipt { .. }
            | EventBody::GroupAvatar { .. }
            | EventBody::BlobManifest(_)
            | EventBody::CallEnded { .. } => {}
        }

        self.frontier
            .insert(event.author_device, event.device_sequence);
        self.events.insert(event.event_id, event);
        Ok(())
    }
}

fn require_owner(membership: &Membership, author: IdentityId) -> Result<(), ApplyError> {
    if membership.owner == author {
        Ok(())
    } else {
        Err(ApplyError::OwnerRequired)
    }
}

fn validate_content(content: &MessageContent) -> Result<(), ApplyError> {
    if content.text.len() > 64 * 1024 {
        return Err(ApplyError::InvalidContent("message exceeds 64 KiB"));
    }
    if content.attachment_ids.len() > 64 {
        return Err(ApplyError::InvalidContent(
            "message has too many attachments",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct ConversationBuilder {
    conversation_id: ConversationId,
    identity_id: IdentityId,
    device_id: DeviceId,
    next_sequence: u64,
    frontier: CausalFrontier,
}

impl ConversationBuilder {
    pub fn new(
        conversation_id: ConversationId,
        identity_id: IdentityId,
        device_id: DeviceId,
        frontier: CausalFrontier,
    ) -> Self {
        let next_sequence = frontier.get(&device_id).copied().unwrap_or(0) + 1;
        Self {
            conversation_id,
            identity_id,
            device_id,
            next_sequence,
            frontier,
        }
    }

    pub fn build(
        &mut self,
        body: EventBody,
        logical_time_ms: i64,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Result<ConversationEvent, ApplyError> {
        let random_id = EventId::random(rng);
        let mut event = ConversationEvent {
            version: PROTOCOL_VERSION,
            conversation_id: self.conversation_id,
            event_id: random_id,
            author_identity: self.identity_id,
            author_device: self.device_id,
            device_sequence: self.next_sequence,
            causal_frontier: self.frontier.clone(),
            logical_time_ms,
            body,
        };
        let mut material = event.to_wire().map_err(ApplyError::Codec)?;
        material.extend_from_slice(random_id.as_bytes());
        event.event_id = EventId::from_bytes(*blake3::hash(&material).as_bytes());
        self.frontier.insert(self.device_id, self.next_sequence);
        self.next_sequence += 1;
        Ok(event)
    }
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("event belongs to another conversation or protocol version")]
    WrongConversation,
    #[error("event from {device} expected sequence {expected}, got {actual}")]
    MissingPredecessor {
        device: DeviceId,
        expected: u64,
        actual: u64,
    },
    #[error("identity {0} is not a member")]
    NotMember(IdentityId),
    #[error("only the message author can perform this action")]
    NotAuthor,
    #[error("only the group owner can perform this action")]
    OwnerRequired,
    #[error("group ownership must be transferred before removing the owner")]
    OwnerMustTransfer,
    #[error("ownership transfer is invalid")]
    InvalidOwnershipTransfer,
    #[error("message {0} does not exist")]
    UnknownMessage(EventId),
    #[error("invalid content: {0}")]
    InvalidContent(&'static str),
    #[error("could not encode event: {0}")]
    Codec(#[source] pptalk_protocol::CodecError),
}

#[cfg(test)]
mod tests {
    use pptalk_protocol::MessageContent;
    use rand::rngs::OsRng;

    use super::*;

    #[test]
    fn owner_controls_membership_and_authors_control_messages() {
        let owner = IdentityId::from_bytes([1; 32]);
        let device = DeviceId::from_bytes([2; 32]);
        let member = IdentityId::from_bytes([3; 32]);
        let conversation_id = ConversationId::from_bytes([4; 32]);
        let mut conversation = Conversation::new(conversation_id, owner, "friends");
        let mut builder =
            ConversationBuilder::new(conversation_id, owner, device, CausalFrontier::new());

        let add = builder
            .build(
                EventBody::MembershipAdd {
                    identity: member,
                    welcome: vec![],
                },
                1,
                &mut OsRng,
            )
            .expect("add");
        conversation.apply(add).expect("apply add");
        assert!(conversation.membership().members.contains(&member));

        let create = builder
            .build(
                EventBody::MessageCreate {
                    content: MessageContent {
                        text: "hello".into(),
                        reply_to: None,
                        attachment_ids: vec![],
                    },
                },
                2,
                &mut OsRng,
            )
            .expect("message");
        let target = create.event_id;
        conversation.apply(create).expect("apply message");
        let delete = builder
            .build(EventBody::MessageDelete { target }, 3, &mut OsRng)
            .expect("delete");
        conversation.apply(delete).expect("apply delete");
        assert!(conversation.messages().next().expect("message").deleted);
    }
}
