use anyhow::Result;
use std::collections::HashMap;

use crate::{
    config::{Rule, RuleInput, RuleOperator, RuleOutcome},
    path::SanitizedLocalPath,
};

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

    pub fn eval(&mut self, path: &SanitizedLocalPath) -> Result<bool> {
        if let Some(value) = self.cache.get(path) {
            Ok(*value)
        } else {
            let value = self.eval_inner(path);
            if let Ok(value) = &value {
                self.cache.insert(path.clone(), *value);
            }
            value
        }
    }

    fn eval_inner(&mut self, path: &SanitizedLocalPath) -> Result<bool> {
        if path == &self.root {
            return Ok(true);
        }
        if let Some(parent) = path.parent()? {
            if !self.eval(&parent)? {
                return Ok(false);
            }
        }

        let mut outcome = RuleOutcome::Include;
        for rule in &self.rules {
            let input = match rule.input {
                RuleInput::Name => path.file_name().unwrap_or(path.as_str()),
                RuleInput::Path => path.as_str(),
            };
            let matches = match &rule.operator {
                RuleOperator::Equals(needle) => input == needle,
                RuleOperator::Matches(regex) => regex.is_match(input),
            };
            if matches {
                outcome = rule.outcome;
                if rule.is_final {
                    break;
                }
            }
        }
        Ok(outcome == RuleOutcome::Include)
    }
}
