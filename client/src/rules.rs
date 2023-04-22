use anyhow::Result;
use regex::Regex;
use serde::{de::Error, Deserialize};
use std::collections::HashMap;

use crate::path::SanitizedLocalPath;

#[derive(Debug, Clone)]
pub struct Rules {
    rules: Vec<Rule>,
    root: SanitizedLocalPath,
    cache: HashMap<SanitizedLocalPath, bool>,
}

impl Rules {
    pub fn new(rules: &[&[Rule]], root: SanitizedLocalPath) -> Self {
        let mut vec = Vec::new();
        for &rules in rules {
            vec.extend_from_slice(rules);
        }
        Self {
            rules: vec,
            root,
            cache: HashMap::new(),
        }
    }

    pub fn matches(&mut self, path: &SanitizedLocalPath) -> Result<bool> {
        if let Some(value) = self.cache.get(path) {
            Ok(*value)
        } else {
            let value = self.matches_inner(path);
            if let Ok(value) = &value {
                self.cache.insert(path.clone(), *value);
            }
            value
        }
    }

    fn matches_inner(&mut self, path: &SanitizedLocalPath) -> Result<bool> {
        if path == &self.root {
            return Ok(false);
        }
        if let Some(parent) = path.parent()? {
            if self.matches(&parent)? {
                return Ok(true);
            }
        }

        for rule in &self.rules {
            if rule.matches(path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Debug, Clone)]
pub enum Rule {
    NameEquals(String),
    NameMatches(Regex),
    PathEquals(SanitizedLocalPath),
    PathMatches(Regex),
    SubdirsOf {
        path: SanitizedLocalPath,
        except: Vec<String>,
    },
}

impl Rule {
    fn matches(&self, path: &SanitizedLocalPath) -> Result<bool> {
        let name = path.file_name().unwrap_or(path.as_str());
        let r = match self {
            Rule::NameEquals(rule) => rule == name,
            Rule::NameMatches(rule) => rule.is_match(name),
            Rule::PathEquals(rule) => rule == path,
            Rule::PathMatches(rule) => rule.is_match(path.as_str()),
            Rule::SubdirsOf {
                path: rule_path,
                except,
            } => {
                if let Some(parent) = path.parent()? {
                    rule_path == &parent && !except.iter().any(|ex| ex == name)
                } else {
                    false
                }
            }
        };
        Ok(r)
    }
}

impl<'de> Deserialize<'de> for Rule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let rule: repr::Rule = Deserialize::deserialize(deserializer)?;
        rule.try_into().map_err(D::Error::custom)
    }
}

mod repr {
    use anyhow::{anyhow, bail, Result};
    use regex::Regex;
    use serde::{Deserialize, Serialize};

    use crate::path::SanitizedLocalPath;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Rule {
        #[serde(default)]
        name_equals: Option<String>,
        #[serde(default, with = "serde_regex")]
        name_matches: Option<Regex>,
        #[serde(default)]
        path_equals: Option<SanitizedLocalPath>,
        #[serde(default, with = "serde_regex")]
        path_matches: Option<Regex>,
        #[serde(default)]
        subdirs_of: Option<SanitizedLocalPath>,
        #[serde(default)]
        except: Option<Vec<String>>,
    }

    impl TryFrom<Rule> for super::Rule {
        type Error = anyhow::Error;
        fn try_from(mut value: Rule) -> Result<Self> {
            let rule = if let Some(v) = value.name_equals.take() {
                super::Rule::NameEquals(v)
            } else if let Some(v) = value.name_matches.take() {
                super::Rule::NameMatches(v)
            } else if let Some(v) = value.path_equals.take() {
                super::Rule::PathEquals(v)
            } else if let Some(v) = value.path_matches.take() {
                super::Rule::PathMatches(v)
            } else if let Some(v) = value.subdirs_of.take() {
                super::Rule::SubdirsOf {
                    path: v,
                    except: value
                        .except
                        .take()
                        .ok_or_else(|| anyhow!("missing 'expect' field near 'subdirs_of' field"))?,
                }
            } else {
                bail!("expected one of 'name_equals', 'name_matches', 'path_equals', 'path_matches', 'subdirs_of'");
            };

            if value.name_equals.is_some()
                || value.name_matches.is_some()
                || value.path_equals.is_some()
                || value.path_matches.is_some()
                || value.subdirs_of.is_some()
            {
                bail!("cannot specify multiple conditions in the same object");
            }
            Ok(rule)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> SanitizedLocalPath {
        SanitizedLocalPath::new(s).unwrap()
    }
    fn rules(rules: &str) -> Rules {
        Rules::new(
            &[&json5::from_str::<Vec<Rule>>(rules).unwrap()],
            p("/tmp/1"),
        )
    }
    fn i(rules: &mut Rules, path: &str) {
        assert!(!rules.matches(&p(path)).unwrap());
    }
    fn e(rules: &mut Rules, path: &str) {
        assert!(rules.matches(&p(path)).unwrap());
    }

    #[test]
    fn empty() {
        let mut rules = rules(r#"[]"#);
        i(&mut rules, "/tmp/1");
        i(&mut rules, "/tmp/1/abc");
        i(&mut rules, "/tmp/1/abc/def");
    }

    #[test]
    fn rules1() {
        let mut rules = rules(
            r#"[
            { name_equals: "abc" },
            { name_matches: "\\..*" },
        ]"#,
        );
        i(&mut rules, "/tmp/1");
        e(&mut rules, "/tmp/1/abc");
        e(&mut rules, "/tmp/1/.a");
        i(&mut rules, "/tmp/1/abd");
        e(&mut rules, "/tmp/1/other/abc");
        e(&mut rules, "/tmp/1/other/.a");
        i(&mut rules, "/tmp/1/other/abd");
        e(&mut rules, "/tmp/1/abc/other");
        e(&mut rules, "/tmp/1/.a/other");
        i(&mut rules, "/tmp/1/abd/other");
    }

    #[test]
    fn with_final() {
        let mut rules = rules(
            r#"[
            { name_equals: "target" },
            { path_equals: "/tmp/1/target/2" },
        ]"#,
        );
        i(&mut rules, "/tmp/1");
        e(&mut rules, "/tmp/1/target");
        e(&mut rules, "/tmp/1/target/2");
        e(&mut rules, "/tmp/1/target/2/a");
    }

    #[test]
    fn with_subdirs() {
        let mut rules = rules(
            r#"[
            { subdirs_of: "/tmp/1/projects", except: ["p1", "p2"] },
        ]"#,
        );
        i(&mut rules, "/tmp/1");
        i(&mut rules, "/tmp/1/projects");
        i(&mut rules, "/tmp/1/projects/p1");
        i(&mut rules, "/tmp/1/projects/p2");
        e(&mut rules, "/tmp/1/projects/p3");
        i(&mut rules, "/tmp/1/projects/p1/abc");
        e(&mut rules, "/tmp/1/projects/p3/abc");
        i(&mut rules, "/tmp/1/projects_2");
    }
}
