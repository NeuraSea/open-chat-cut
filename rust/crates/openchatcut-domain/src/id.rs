use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_ID_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    #[error("{kind} cannot be empty")]
    Empty { kind: &'static str },
    #[error("{kind} is longer than {max} bytes")]
    TooLong { kind: &'static str, max: usize },
    #[error("{kind} contains disallowed character '{character}' at byte {index}")]
    InvalidCharacter {
        kind: &'static str,
        character: char,
        index: usize,
    },
}

fn validate(value: &str, kind: &'static str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if value.len() > MAX_ID_LENGTH {
        return Err(IdError::TooLong {
            kind,
            max: MAX_ID_LENGTH,
        });
    }

    for (index, character) in value.char_indices() {
        if !(character.is_ascii_alphanumeric()
            || matches!(character, '-' | '_' | '.' | ':' | '/' | '@'))
        {
            return Err(IdError::InvalidCharacter {
                kind,
                character,
                index,
            });
        }
    }

    Ok(())
}

macro_rules! stable_id {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                validate(&value, $kind)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

stable_id!(ProjectId, "project ID");
stable_id!(SceneId, "scene ID");
stable_id!(TrackId, "track ID");
stable_id!(ItemId, "timeline item ID");
stable_id!(AssetId, "asset ID");
stable_id!(TranscriptId, "transcript ID");
stable_id!(WordId, "transcript word ID");
stable_id!(SegmentId, "transcript segment ID");
stable_id!(SpeakerId, "speaker ID");
stable_id!(StorySequenceId, "story sequence ID");
stable_id!(StoryClipId, "story clip ID");
stable_id!(LinkGroupId, "link group ID");
stable_id!(TransactionId, "transaction ID");
stable_id!(IdempotencyKey, "idempotency key");
stable_id!(ActorId, "actor ID");
stable_id!(CaptionPresetId, "caption preset ID");
stable_id!(EditPlanId, "edit plan ID");
stable_id!(JobId, "job ID");
stable_id!(ProviderId, "provider ID");

#[cfg(test)]
mod tests {
    use super::{IdError, ProjectId};

    #[test]
    fn validates_ids_at_all_input_boundaries() {
        assert_eq!(
            ProjectId::new(""),
            Err(IdError::Empty { kind: "project ID" })
        );
        assert!(ProjectId::new("project:018f/safe_id").is_ok());
        assert!(ProjectId::new("not safe").is_err());
        assert!(serde_json::from_str::<ProjectId>(r#""also unsafe""#).is_err());
    }
}
