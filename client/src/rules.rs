use std::path::Path;

use serde::Deserialize;

use crate::{
    config::{Rule, RuleInput, RuleOperator, RuleOutcome},
    term::error,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Rules(pub Vec<Rule>);

impl Rules {
    pub fn eval(&self, path: &Path) -> bool {
        let Some(path_str) = path.to_str() else {
            error(format!("encountered invalid path {:?} while evaluating rules", path));
            return false;
        };
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            error(format!("encountered invalid path {:?} while evaluating rules", path));
            return false;
        };
        let mut outcome = RuleOutcome::Include;
        for rule in &self.0 {
            let input = match rule.input {
                RuleInput::Name => name,
                RuleInput::Path => path_str,
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
        outcome == RuleOutcome::Include
    }
}
