//! Mojang's `rules` blocks on libraries and arguments.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    pub action: Action,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Disallow,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    /// Old-style regex on the OS version (only used by ancient versions).
    pub version: Option<String>,
    #[serde(rename = "versionRange")]
    pub version_range: Option<VersionRange>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionRange {
    pub min: Option<String>,
    pub max: Option<String>,
}

/// The machine the rules are checked against.
#[derive(Clone, Debug)]
pub struct Env {
    /// `windows`, `osx` or `linux`.
    pub os: &'static str,
    /// `x86_64` or `arm64`.
    pub arch: &'static str,
    pub features: HashMap<String, bool>,
}

impl Env {
    pub fn current() -> Self {
        let os = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "osx",
            _ => "linux",
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            _ => "x86_64",
        };
        Self {
            os,
            arch,
            features: HashMap::new(),
        }
    }

    pub fn with_feature(mut self, name: &str, on: bool) -> Self {
        self.features.insert(name.to_owned(), on);
        self
    }

    fn os_matches(&self, r: &OsRule) -> bool {
        if r.name.as_deref().is_some_and(|n| n != self.os) {
            return false;
        }
        if let Some(arch) = r.arch.as_deref() {
            // "x86" in Mojang's files means a 32-bit JVM; we only ship 64-bit.
            let ok = match arch {
                "x86" => false,
                "x86_64" | "amd64" => self.arch == "x86_64",
                "arm64" | "aarch64" => self.arch == "arm64",
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        if r.version.is_some() {
            return false;
        }
        if let Some(range) = &r.version_range {
            // We assume a current OS: every "at least X" holds, no "at most X" does.
            if range.max.is_some() {
                return false;
            }
        }
        true
    }

    fn rule_matches(&self, r: &Rule) -> bool {
        if r.os.as_ref().is_some_and(|os| !self.os_matches(os)) {
            return false;
        }
        if let Some(features) = &r.features {
            for (name, want) in features {
                if self.features.get(name).copied().unwrap_or(false) != *want {
                    return false;
                }
            }
        }
        true
    }

    /// No rules = allowed. Otherwise the last matching rule decides.
    pub fn allows(&self, rules: Option<&[Rule]>) -> bool {
        let Some(rules) = rules else { return true };
        let mut allowed = false;
        for r in rules {
            if self.rule_matches(r) {
                allowed = r.action == Action::Allow;
            }
        }
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(json: &str) -> Vec<Rule> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn os_and_arch() {
        let env = Env {
            os: "windows",
            arch: "x86_64",
            features: HashMap::new(),
        };
        assert!(env.allows(Some(&rules(
            r#"[{"action":"allow","os":{"name":"windows"}}]"#
        ))));
        assert!(!env.allows(Some(&rules(r#"[{"action":"allow","os":{"name":"osx"}}]"#))));
        assert!(!env.allows(Some(&rules(r#"[{"action":"allow","os":{"arch":"x86"}}]"#))));
        assert!(env.allows(None));
    }

    #[test]
    fn windows_gc_picks_one() {
        let env = Env {
            os: "windows",
            arch: "x86_64",
            features: HashMap::new(),
        };
        let zgc = rules(
            r#"[{"action":"allow","os":{"name":"windows","versionRange":{"min":"10.0.17134"}}}]"#,
        );
        let g1 = rules(
            r#"[{"action":"allow","os":{"name":"windows","versionRange":{"max":"10.0.17134"}}}]"#,
        );
        assert!(env.allows(Some(&zgc)));
        assert!(!env.allows(Some(&g1)));
    }

    #[test]
    fn features() {
        let env = Env::current().with_feature("is_quick_play_multiplayer", true);
        let qp = rules(r#"[{"action":"allow","features":{"is_quick_play_multiplayer":true}}]"#);
        let demo = rules(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#);
        assert!(env.allows(Some(&qp)));
        assert!(!env.allows(Some(&demo)));
    }
}
