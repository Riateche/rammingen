use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    NameEquals(String),
    NameMatches(#[serde(with = "serde_regex")] Regex),
    PathEquals(SanitizedLocalPath),
    PathMatches(#[serde(with = "serde_regex")] Regex),
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
            { subdirs_of: { path: "/tmp/1/projects", except: ["p1", "p2"] } },
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
