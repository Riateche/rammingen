use serde::Deserialize;

use crate::{
    config::{Rule, RuleInput, RuleOperator, RuleOutcome},
    path::SanitizedLocalPath,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Rules(pub Vec<Rule>);

impl Rules {
    pub fn eval(&self, path: &SanitizedLocalPath) -> bool {
        let name = path.file_name().unwrap_or(path.as_str());
        let mut outcome = RuleOutcome::Include;
        for rule in &self.0 {
            let input = match rule.input {
                RuleInput::Name => name,
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
        outcome == RuleOutcome::Include
    }
}
