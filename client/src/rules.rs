use {
    crate::path::SanitizedLocalPath,
    anyhow::Result,
    regex::Regex,
    serde::{Deserialize, Serialize},
    std::collections::HashMap,
};

#[derive(Debug, Clone)]
pub struct Rules {
    rules: Vec<Rule>,
    root: SanitizedLocalPath,
    cache: HashMap<SanitizedLocalPath, bool>,
}

impl Rules {
    #[must_use]
    #[inline]
    pub fn new(rules: &[&[Rule]], root: SanitizedLocalPath) -> Self {
        let mut vec = Vec::new();
        for &rules_item in rules {
            vec.extend_from_slice(rules_item);
        }
        Self {
            rules: vec,
            root,
            cache: HashMap::new(),
        }
    }

    #[inline]
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
        if path
            .file_name()
            .is_some_and(|name| name.ends_with(".rammingen.part"))
        {
            return Ok(true);
        }
        if let Some(parent) = path.parent()?
            && self.matches(&parent)?
        {
            return Ok(true);
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
            } =>
            {
                #[expect(clippy::print_stdout, reason = "tmp")]
                if let Some(parent) = path.parent()? {
                    println!("path={path:?}, parent={parent:?}, rule_path={rule_path:?}");
                    (rule_path == &parent) && !except.iter().any(|ex| ex == name)
                } else {
                    false
                }
            }
        };
        Ok(r)
    }
}

// TODO: handle path delimiters properly on Windows, add tests
#[cfg(test)]
#[cfg(not(target_os = "windows"))]
mod tests {
    use {super::*, std::path::Path};

    #[track_caller]
    fn p(s: &str) -> SanitizedLocalPath {
        let path = Path::new("/data").join(s);
        SanitizedLocalPath::new(&path).unwrap()
    }
    #[track_caller]
    fn rules(rules: &str) -> Rules {
        Rules::new(&[&json5::from_str::<Vec<Rule>>(rules).unwrap()], p("1"))
    }
    #[track_caller]
    fn i(rules: &mut Rules, path: &str) {
        assert!(!rules.matches(&p(path)).unwrap());
    }
    #[track_caller]
    fn e(rules: &mut Rules, path: &str) {
        assert!(rules.matches(&p(path)).unwrap());
    }

    #[test]
    fn empty() {
        let mut rules = rules("[]");
        i(&mut rules, "1");
        i(&mut rules, "1/abc");
        i(&mut rules, "1/abc/def");
    }

    #[test]
    fn rules1() {
        let mut rules = rules(
            r#"[
            { name_equals: "abc" },
            { name_matches: "\\..*" },
        ]"#,
        );
        i(&mut rules, "1");
        e(&mut rules, "1/abc");
        e(&mut rules, "1/.a");
        i(&mut rules, "1/abd");
        e(&mut rules, "1/other/abc");
        e(&mut rules, "1/other/.a");
        i(&mut rules, "1/other/abd");
        e(&mut rules, "1/abc/other");
        e(&mut rules, "1/.a/other");
        i(&mut rules, "1/abd/other");
    }

    #[test]
    fn with_final() {
        let mut rules = rules(
            r#"[
                { name_equals: "target" },
                { path_equals: "/data/1/target/2" },
            ]"#,
        );
        i(&mut rules, "1");
        e(&mut rules, "1/target");
        e(&mut rules, "1/target/2");
        e(&mut rules, "1/target/2/a");
    }

    #[test]
    fn with_subdirs() {
        let mut rules = rules(
            r#"[
                { subdirs_of: { path: "/data/1/projects", except: ["p1", "p2"] } },
            ]"#,
        );
        i(&mut rules, "1");
        i(&mut rules, "1/projects");
        i(&mut rules, "1/projects/p1");
        i(&mut rules, "1/projects/p2");
        e(&mut rules, "1/projects/p3");
        i(&mut rules, "1/projects/p1/abc");
        e(&mut rules, "1/projects/p3/abc");
        i(&mut rules, "1/projects_2");
    }
}
