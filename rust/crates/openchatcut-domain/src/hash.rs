use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DomainError, EditTransaction, ProjectDocument};

fn validate_digest(value: &str) -> Result<(), String> {
    if value.len() != 64 {
        return Err("a SHA-256 digest must contain exactly 64 hexadecimal characters".into());
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("a SHA-256 digest must use lowercase hexadecimal characters".into());
    }
    Ok(())
}

macro_rules! digest_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                validate_digest(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn from_digest_bytes(bytes: [u8; 32]) -> Self {
                let mut value = String::with_capacity(64);
                for byte in &bytes {
                    use std::fmt::Write as _;
                    write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
                }
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
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

digest_type!(Sha256Digest);
digest_type!(DocumentHash);
digest_type!(TransactionFingerprint);

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), DomainError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => output.extend_from_slice(number.to_string().as_bytes()),
        Value::String(string) => {
            let encoded =
                serde_json::to_string(string).map_err(|error| DomainError::Serialization {
                    message: error.to_string(),
                })?;
            output.extend_from_slice(encoded.as_bytes());
        }
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                let encoded_key =
                    serde_json::to_string(key).map_err(|error| DomainError::Serialization {
                        message: error.to_string(),
                    })?;
                output.extend_from_slice(encoded_key.as_bytes());
                output.push(b':');
                write_canonical_json(&values[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, DomainError> {
    let value = serde_json::to_value(value).map_err(|error| DomainError::Serialization {
        message: error.to_string(),
    })?;
    let mut bytes = Vec::new();
    write_canonical_json(&value, &mut bytes)?;
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn canonical_document_hash(document: &ProjectDocument) -> Result<DocumentHash, DomainError> {
    Ok(DocumentHash::from_digest_bytes(sha256(&canonical_bytes(
        document,
    )?)))
}

pub fn transaction_fingerprint(
    transaction: &EditTransaction,
) -> Result<TransactionFingerprint, DomainError> {
    Ok(TransactionFingerprint::from_digest_bytes(sha256(
        &canonical_bytes(transaction)?,
    )))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::{ProjectDocument, ProjectId};

    use super::canonical_document_hash;

    #[test]
    fn canonical_hash_is_independent_of_extension_insertion_order() {
        let mut first = ProjectDocument::new(ProjectId::new("project").unwrap(), "Test");
        first.extensions = BTreeMap::from([
            ("b".into(), json!({"z": 1, "a": 2})),
            ("a".into(), json!(true)),
        ]);
        let mut second = ProjectDocument::new(ProjectId::new("project").unwrap(), "Test");
        second.extensions = BTreeMap::from([
            ("a".into(), json!(true)),
            ("b".into(), json!({"a": 2, "z": 1})),
        ]);

        assert_eq!(
            canonical_document_hash(&first).unwrap(),
            canonical_document_hash(&second).unwrap()
        );
    }
}
