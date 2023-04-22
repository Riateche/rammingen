use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use core::fmt;
use derivative::Derivative;
use generic_array::GenericArray;
use rammingen_protocol::ArchivePath;
use regex::Regex;
use serde::de::Error;
use serde::Deserialize;
use typenum::U64;

use crate::path::SanitizedLocalPath;

/*
global_rules: [

]
global rules:

-! name == tmp

{
    local: /a/b
    archive: ar:/data/b
    rules: [
        -! name == "target"
        -! name =~ "^build_"
        +  path == "/a/b/c"
        -  path == "a/b/c/d"
        +  path =~ "^a/b/c/d/prefix_"
        +  path == "c:\users\x"
        +  path =~ "c:\\users\\x"
    ]
},


{
    local: "/a/b",
    archive: "ar:/data/b",
    rules: [
        { definitely_exclude_if: { name_equals: "target" } },
        { definitely_exclude_if: { name_matches: "^build_" } },
        { include_if: { path_equals: "/a/b/c" } },
        { include_if: { path_equals: "c:\\users\\abc" } },
        { exclude_if: { path_matches: "^a/b/c/d/prefix_" } },
    ],
},


*/

#[derive(Debug, Clone)]
pub enum RuleOperator {
    Equals(String),
    Matches(Regex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleOutcome {
    Include,
    Exclude,
}

#[derive(Debug, Clone, Copy)]
pub enum RuleInput {
    Name,
    Path,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub outcome: RuleOutcome,
    pub is_final: bool,
    pub input: RuleInput,
    pub operator: RuleOperator,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MountPoint {
    pub local: SanitizedLocalPath,
    pub archive: ArchivePath,
    pub rules: Vec<Rule>,
}

#[derive(Clone)]
pub struct EncryptionKey(pub GenericArray<u8, U64>);

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptionKey").finish()
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let binary = BASE64_URL_SAFE_NO_PAD
            .decode(string)
            .map_err(D::Error::custom)?;
        let array = <[u8; 64]>::try_from(binary).map_err(|vec| {
            D::Error::custom(format!(
                "invalid encryption key length, expected 64, got {}",
                vec.len()
            ))
        })?;
        Ok(Self(array.into()))
    }
}

#[derive(Derivative, Clone, Deserialize)]
#[derivative(Debug)]
pub struct Config {
    pub global_rules: Vec<Rule>,
    pub mount_points: Vec<MountPoint>,
    pub encryption_key: EncryptionKey,
    pub server_url: String,
    #[derivative(Debug = "ignore")]
    pub token: String,
    #[derivative(Debug = "ignore")]
    pub salt: String,
}

mod repr {
    use regex::Regex;
    use serde::{Deserialize, Serialize};

    use super::{RuleInput, RuleOperator, RuleOutcome};

    #[derive(Debug, Serialize, Deserialize)]
    #[allow(clippy::enum_variant_names)]
    pub enum Rule {
        IncludeIf(RuleCondition),
        ExcludeIf(RuleCondition),
        DefinitelyIncludeIf(RuleCondition),
        DefinitelyExcludeIf(RuleCondition),
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum RuleCondition {
        NameEquals(String),
        NameMatches(#[serde(with = "serde_regex")] Regex),
        PathEquals(String),
        PathMatches(#[serde(with = "serde_regex")] Regex),
    }

    impl From<Rule> for super::Rule {
        fn from(value: Rule) -> Self {
            let (outcome, is_final, input) = match value {
                Rule::IncludeIf(input) => (RuleOutcome::Include, false, input),
                Rule::ExcludeIf(input) => (RuleOutcome::Exclude, false, input),
                Rule::DefinitelyIncludeIf(input) => (RuleOutcome::Include, true, input),
                Rule::DefinitelyExcludeIf(input) => (RuleOutcome::Exclude, true, input),
            };
            let (input, operator) = match input {
                RuleCondition::NameEquals(value) => (RuleInput::Name, RuleOperator::Equals(value)),
                RuleCondition::NameMatches(value) => {
                    (RuleInput::Name, RuleOperator::Matches(value))
                }
                RuleCondition::PathEquals(value) => (RuleInput::Path, RuleOperator::Equals(value)),
                RuleCondition::PathMatches(value) => {
                    (RuleInput::Path, RuleOperator::Matches(value))
                }
            };

            Self {
                outcome,
                is_final,
                input,
                operator,
            }
        }
    }
}

impl<'de> Deserialize<'de> for Rule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <repr::Rule as Deserialize>::deserialize(deserializer).map(Into::into)
    }
}
